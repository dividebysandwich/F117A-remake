//! Engine sounds heard from inside the aircraft: a low-pass filter (the canopy and the helmet
//! take the highs off), its cutoff gliding when the view moves in or out of the cockpit (or the
//! SOUND options turn it off).
//!
//! The filter runs on the audio thread over the decoded samples, so it loops the sound itself:
//! Bevy's looping would replay the first pass as it was filtered then.

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
    made: HashMap<(AssetId<AudioSource>, bool), Handle<Muffled>>,
}

impl Default for MuffledSounds {
    fn default() -> Self {
        Self { cutoff: Arc::new(AtomicU32::new(INSIDE_HZ.to_bits())), made: HashMap::new() }
    }
}

/// An engine sound through the low-pass filter; `looped` plays it on without an end.
#[derive(Asset, TypePath)]
pub(super) struct Muffled {
    source: AudioSource,
    looped: bool,
    cutoff: Arc<AtomicU32>,
}

impl Decodable for Muffled {
    type Decoder = LowPass;

    fn decoder(&self) -> LowPass {
        let inner = self.source.decoder();
        let channels = inner.channels();
        LowPass {
            source: self.source.clone(),
            looped: self.looped,
            channels,
            rate: inner.sample_rate(),
            inner,
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
    /// The filtered sound of `handle` (None until it has loaded).
    pub(super) fn get(
        &mut self,
        handle: &Handle<AudioSource>,
        looped: bool,
    ) -> Option<Handle<Muffled>> {
        let key = (handle.id(), looped);
        if let Some(h) = self.sounds.made.get(&key) {
            return Some(h.clone());
        }
        let source = self.sources.get(handle)?.clone();
        let h = self.muffled.add(Muffled { source, looped, cutoff: self.sounds.cutoff.clone() });
        self.sounds.made.insert(key, h.clone());
        Some(h)
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
    source: AudioSource,
    looped: bool,
    inner: <AudioSource as Decodable>::Decoder,
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
        let x = match self.inner.next() {
            Some(x) => x,
            None if self.looped => {
                self.inner = self.source.decoder();
                self.inner.next()?
            }
            None => return None,
        };
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
        if self.looped { None } else { self.inner.total_duration() }
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
            source: AudioSource { bytes: Arc::from(&bytes[..]) },
            looped: false,
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

    /// A looped sound plays on past its end, through the filter.
    #[test]
    fn looped_sound_plays_on() {
        let bytes = include_bytes!("../../assets/sounds/engine_300.flac");
        let source = AudioSource { bytes: Arc::from(&bytes[..]) };
        let once = source.decoder().count();
        let sound =
            Muffled { source, looped: true, cutoff: Arc::new(AtomicU32::new(INSIDE_HZ.to_bits())) };
        let mut f = sound.decoder();
        assert_eq!(f.total_duration(), None);
        let played: Vec<f32> = f.by_ref().take(once * 2 + 1000).collect();
        assert_eq!(played.len(), once * 2 + 1000);
        assert!(played.iter().all(|x| x.is_finite() && x.abs() < 2.0));
        assert!(played[once + 500..].iter().any(|x| x.abs() > 0.01), "heard after the loop");
    }
}
