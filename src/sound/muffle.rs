//! Engine sounds heard from inside the aircraft: a low-pass filter (the canopy and the helmet
//! take the highs off), its cutoff gliding when the view moves in or out of the cockpit (or the
//! SOUND options turn it off).
//!
//! The filter runs on the audio thread over the decoded samples, so it loops the sound itself:
//! Bevy's looping would replay the first pass as it was filtered then. A looped sound plays at
//! a speed set while it plays (`Pitch`), read from the decoded samples at that rate: the sink's
//! own speed only reaches rodio's resampler when a span starts, and an endless one never does.

use std::collections::HashMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicU32, Ordering};
use std::time::Duration;

use bevy::audio::{AudioSource, ChannelCount, Decodable, SampleRate, Source};
use bevy::prelude::*;

use crate::camera::{View, ViewState};
use crate::settings::Settings;

/// Cutoff (Hz) inside the aircraft, and outside where the filter is all but open.
const INSIDE_HZ: f32 = 1000.0;
const OUTSIDE_HZ: f32 = 18000.0;
/// Time constant (s) of the cutoff's glide.
const GLIDE: f32 = 0.12;
/// Samples between checks of the cutoff on the audio thread.
const REFRESH: u32 = 64;

/// The engine sounds made into filtered ones, and the cutoff they share with the audio thread
/// (Hz, as f32 bits).
#[derive(Resource)]
pub(super) struct MuffledSounds {
    cutoff: Arc<AtomicU32>,
    made: HashMap<AssetId<AudioSource>, Handle<Muffled>>,
    /// The looped sounds, decoded.
    decoded: HashMap<AssetId<AudioSource>, Arc<Decoded>>,
}

impl Default for MuffledSounds {
    fn default() -> Self {
        Self {
            cutoff: Arc::new(AtomicU32::new(INSIDE_HZ.to_bits())),
            made: HashMap::new(),
            decoded: HashMap::new(),
        }
    }
}

/// A sound's samples (channels interleaved).
struct Decoded {
    samples: Vec<f32>,
    channels: ChannelCount,
    rate: SampleRate,
}

/// The playback speed of a looped engine sound (1 = as recorded), set while it plays.
#[derive(Component, Clone)]
pub(super) struct Pitch(Arc<AtomicU32>);

impl Pitch {
    pub(super) fn set(&self, speed: f32) {
        self.0.store(speed.to_bits(), Ordering::Relaxed);
    }
}

/// An engine sound through the low-pass filter: once, or looped at its `Pitch`.
#[derive(Asset, TypePath)]
pub(super) struct Muffled {
    sound: Sound,
    cutoff: Arc<AtomicU32>,
}

enum Sound {
    Once(AudioSource),
    Looped(Arc<Decoded>, Pitch),
}

impl Decodable for Muffled {
    type Decoder = LowPass;

    fn decoder(&self) -> LowPass {
        let (input, channels, rate) = match &self.sound {
            Sound::Once(source) => {
                let inner = source.decoder();
                let (channels, rate) = (inner.channels(), inner.sample_rate());
                (Input::Once(Box::new(inner)), channels, rate)
            }
            Sound::Looped(sound, pitch) => (
                Input::Looped {
                    sound: sound.clone(),
                    pitch: pitch.clone(),
                    speed: f32::from_bits(pitch.0.load(Ordering::Relaxed)) as f64,
                    pos: 0.0,
                    frame: vec![0.0; sound.channels.get() as usize],
                },
                sound.channels,
                sound.rate,
            ),
        };
        LowPass {
            input,
            channels,
            rate,
            cutoff: self.cutoff.clone(),
            hz: 0.0,
            coef: [1.0, 0.0, 0.0, 0.0, 0.0],
            z: vec![[0.0; 2]; channels.get() as usize],
            channel: 0,
            countdown: 0,
        }
    }
}

/// Makes the filtered sounds.
#[derive(bevy::ecs::system::SystemParam)]
pub(super) struct Muffler<'w> {
    sounds: ResMut<'w, MuffledSounds>,
    sources: Res<'w, Assets<AudioSource>>,
    muffled: ResMut<'w, Assets<Muffled>>,
}

impl Muffler<'_> {
    /// The filtered sound of `handle`, played once (None until it has loaded).
    pub(super) fn get(&mut self, handle: &Handle<AudioSource>) -> Option<Handle<Muffled>> {
        if let Some(h) = self.sounds.made.get(&handle.id()) {
            return Some(h.clone());
        }
        let source = self.sources.get(handle)?.clone();
        let sound = Sound::Once(source);
        let h = self.muffled.add(Muffled { sound, cutoff: self.sounds.cutoff.clone() });
        self.sounds.made.insert(handle.id(), h.clone());
        Some(h)
    }

    /// The filtered sound of `handle` looped, at a speed its `Pitch` sets (None until it has
    /// loaded). Each call makes a sound of its own, for one player.
    pub(super) fn looped(
        &mut self,
        handle: &Handle<AudioSource>,
        speed: f32,
    ) -> Option<(Handle<Muffled>, Pitch)> {
        let decoded = match self.sounds.decoded.get(&handle.id()) {
            Some(d) => d.clone(),
            None => {
                let decoder = self.sources.get(handle)?.decoder();
                let (channels, rate) = (decoder.channels(), decoder.sample_rate());
                let d = Arc::new(Decoded { samples: decoder.collect(), channels, rate });
                self.sounds.decoded.insert(handle.id(), d.clone());
                d
            }
        };
        let pitch = Pitch(Arc::new(AtomicU32::new(speed.to_bits())));
        let sound = Sound::Looped(decoded, pitch.clone());
        Some((self.muffled.add(Muffled { sound, cutoff: self.sounds.cutoff.clone() }), pitch))
    }
}

/// The cutoff for the view: muffled inside the aircraft (the cockpit views), unless the
/// low-pass is turned off.
fn cutoff(view: Option<&ViewState>, settings: &Settings) -> f32 {
    let inside = view.is_some_and(|v| {
        matches!(v.view, View::Cockpit | View::LookLeft | View::LookRight | View::LookRear)
    });
    if inside && settings.engine_low_pass { INSIDE_HZ } else { OUTSIDE_HZ }
}

/// The flight starts with the cutoff where the view is.
pub(super) fn reset(
    view: Option<Res<ViewState>>,
    settings: Res<Settings>,
    sounds: Res<MuffledSounds>,
) {
    let hz = cutoff(view.as_deref(), &settings);
    sounds.cutoff.store(hz.to_bits(), Ordering::Relaxed);
}

/// Glides the cutoff to the view's (in the frequency's log).
pub(super) fn follow_view(
    view: Option<Res<ViewState>>,
    settings: Res<Settings>,
    real: Res<Time<Real>>,
    sounds: Res<MuffledSounds>,
) {
    let target = cutoff(view.as_deref(), &settings);
    let hz = f32::from_bits(sounds.cutoff.load(Ordering::Relaxed));
    let k = 1.0 - (-real.delta_secs() / GLIDE).exp();
    let next = (hz.ln() + (target.ln() - hz.ln()) * k).exp();
    let next = if (next - target).abs() < 1.0 { target } else { next };
    sounds.cutoff.store(next.to_bits(), Ordering::Relaxed);
}

/// A second-order low-pass (Butterworth) over the decoded samples, each channel filtered on
/// its own.
pub(super) struct LowPass {
    input: Input,
    channels: ChannelCount,
    rate: SampleRate,
    cutoff: Arc<AtomicU32>,
    /// The cutoff the coefficients are for.
    hz: f32,
    /// b0, b1, b2, a1, a2 (a0 = 1).
    coef: [f32; 5],
    /// Per channel state (transposed direct form II).
    z: Vec<[f32; 2]>,
    channel: usize,
    countdown: u32,
}

/// The samples into the filter: a sound decoded as it plays, or a looped one read at its speed.
enum Input {
    Once(Box<<AudioSource as Decodable>::Decoder>),
    Looped {
        sound: Arc<Decoded>,
        pitch: Pitch,
        speed: f64,
        /// Position (frames) in the sound.
        pos: f64,
        /// The frame being handed out, its channels one by one.
        frame: Vec<f32>,
    },
}

impl LowPass {
    fn update(&mut self) {
        let hz = f32::from_bits(self.cutoff.load(Ordering::Relaxed));
        if hz == self.hz {
            return;
        }
        self.hz = hz;
        let fs = self.rate.get() as f32;
        let w0 = std::f32::consts::TAU * hz.clamp(20.0, 0.45 * fs) / fs;
        let alpha = w0.sin() / std::f32::consts::SQRT_2;
        let (cos, a0) = (w0.cos(), 1.0 + alpha);
        let b1 = (1.0 - cos) / a0;
        self.coef = [b1 / 2.0, b1, b1 / 2.0, -2.0 * cos / a0, (1.0 - alpha) / a0];
    }

    /// The next sample into the filter (channels interleaved).
    fn input(&mut self) -> Option<f32> {
        let channel = self.channel;
        let refresh = channel == 0 && self.countdown == 0;
        match &mut self.input {
            Input::Once(inner) => inner.next(),
            Input::Looped { sound, pitch, speed, pos, frame } => {
                if channel == 0 {
                    if refresh {
                        *speed = f32::from_bits(pitch.0.load(Ordering::Relaxed)).max(0.0) as f64;
                    }
                    // Linear interpolation between the frames around the position, the last
                    // frame running into the first.
                    let n = sound.channels.get() as usize;
                    let frames = sound.samples.len() / n;
                    let i = *pos as usize;
                    let t = (*pos - i as f64) as f32;
                    let j = (i + 1) % frames;
                    for (c, out) in frame.iter_mut().enumerate() {
                        let (a, b) = (sound.samples[i * n + c], sound.samples[j * n + c]);
                        *out = a + (b - a) * t;
                    }
                    *pos = (*pos + *speed) % frames as f64;
                }
                Some(frame[channel])
            }
        }
    }

    /// Filters the next sample (channels interleaved).
    fn step(&mut self, x: f32) -> f32 {
        if self.channel == 0 {
            if self.countdown == 0 {
                self.update();
                self.countdown = REFRESH;
            }
            self.countdown -= 1;
        }
        let [b0, b1, b2, a1, a2] = self.coef;
        let z = &mut self.z[self.channel];
        let y = b0 * x + z[0];
        z[0] = b1 * x - a1 * y + z[1];
        z[1] = b2 * x - a2 * y;
        self.channel = (self.channel + 1) % self.z.len();
        y
    }
}

impl Iterator for LowPass {
    type Item = bevy::audio::Sample;

    fn next(&mut self) -> Option<Self::Item> {
        let x = self.input()?;
        Some(self.step(x))
    }
}

impl Source for LowPass {
    fn current_span_len(&self) -> Option<usize> {
        None
    }

    fn channels(&self) -> ChannelCount {
        self.channels
    }

    fn sample_rate(&self) -> SampleRate {
        self.rate
    }

    fn total_duration(&self) -> Option<Duration> {
        match &self.input {
            Input::Once(inner) => inner.total_duration(),
            Input::Looped { .. } => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Tones through the filter: inside a low one passes and a high one is cut, outside the
    /// high one passes too; the cutoff is followed while playing.
    #[test]
    fn low_pass_cuts_the_highs() {
        let bytes = include_bytes!("../../assets/sounds/sfx_19_gear.flac");
        let cutoff = Arc::new(AtomicU32::new(INSIDE_HZ.to_bits()));
        let sound = Muffled {
            sound: Sound::Once(AudioSource { bytes: Arc::from(&bytes[..]) }),
            cutoff: cutoff.clone(),
        };
        let mut f = sound.decoder();
        let channels = f.channels().get() as usize;
        let rate = f.sample_rate().get() as f32;
        let mut level = |freq: f32| {
            let mut peak: f32 = 0.0;
            for i in 0..rate as usize {
                let x = (i as f32 * std::f32::consts::TAU * freq / rate).sin();
                for _ in 0..channels {
                    let y = f.step(x);
                    if i > rate as usize / 10 {
                        peak = peak.max(y.abs());
                    }
                }
            }
            peak
        };
        let (low, high) = (level(200.0), level(6000.0));
        cutoff.store(OUTSIDE_HZ.to_bits(), Ordering::Relaxed);
        let open = level(6000.0);
        println!("inside: 200 Hz {low:.2}, 6 kHz {high:.3}; outside 6 kHz {open:.2}");
        assert!(low > 0.95 && high < 0.05 && open > 0.95);
    }

    /// A looped sound: `samples` (mono, 44.1 kHz) at `speed`, the filter all but open.
    fn looped(samples: Vec<f32>, speed: f32) -> (LowPass, Pitch) {
        let channels = ChannelCount::new(1).unwrap();
        let rate = SampleRate::new(44_100).unwrap();
        let pitch = Pitch(Arc::new(AtomicU32::new(speed.to_bits())));
        let sound = Muffled {
            sound: Sound::Looped(Arc::new(Decoded { samples, channels, rate }), pitch.clone()),
            cutoff: Arc::new(AtomicU32::new(OUTSIDE_HZ.to_bits())),
        };
        (sound.decoder(), pitch)
    }

    /// A looped sound plays on past its end, through the filter.
    #[test]
    fn looped_sound_plays_on() {
        let bytes = include_bytes!("../../assets/sounds/engine_300.flac");
        let source = AudioSource { bytes: Arc::from(&bytes[..]) };
        let samples: Vec<f32> = source.decoder().collect();
        let once = samples.len();
        let (mut f, _) = looped(samples, 1.0);
        assert_eq!(f.total_duration(), None);
        let played: Vec<f32> = f.by_ref().take(once * 2 + 1000).collect();
        assert_eq!(played.len(), once * 2 + 1000);
        assert!(played.iter().all(|x| x.is_finite() && x.abs() < 2.0));
        assert!(played[once + 500..].iter().any(|x| x.abs() > 0.01), "heard after the loop");
    }

    /// The pitch follows the speed set while the sound plays: a 441 Hz tone at half speed is
    /// at 220.5 Hz.
    #[test]
    fn looped_sound_follows_its_pitch() {
        let tone = (0..44_100).map(|i| (i as f32 * std::f32::consts::TAU / 100.0).sin()).collect();
        let (mut f, pitch) = looped(tone, 1.0);
        let crossings = |f: &mut LowPass| {
            let s: Vec<f32> = f.by_ref().take(44_100).collect();
            s.windows(2).filter(|w| w[0] < 0.0 && w[1] >= 0.0).count()
        };
        assert!((crossings(&mut f) as i32 - 441).abs() <= 2);
        pitch.set(0.5);
        crossings(&mut f);
        assert!((crossings(&mut f) as i32 - 220).abs() <= 2);
    }
}
