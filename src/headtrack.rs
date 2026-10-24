//! Head tracking on the desktop (`--head-tracking`, the menu's "Head tracking"): TrackIR-style
//! trackers driven by OpenTrack (or any program speaking its protocols) turn and move the
//! pilot's head in the cockpit views, on top of the mouse look. Two inputs, read every frame:
//!
//! - Windows: the FreeTrack shared memory (`FT_SharedMem`), written by OpenTrack's "freetrack
//!   2.0 Enhanced" output.
//! - OpenTrack's "UDP over network" output (Linux, or by choice): datagrams of six little-endian
//!   f64 (x, y, z in cm, yaw, pitch, roll in degrees; yaw right, pitch up, roll left, x left,
//!   y up, z back: the signs its FreeTrack, SimConnect and FlightGear outputs assume) sent to
//!   127.0.0.1 port 4242, OpenTrack's default. A loopback socket needs no firewall rule.
//!
//! Home puts the current pose at the centre (OpenTrack's own centre key does too). Not in VR,
//! where the headset tracks the head.
// The browser build has neither (the plugin only adds the resource there).
#![cfg_attr(target_arch = "wasm32", allow(dead_code, unused_imports))]

use std::net::UdpSocket;

use bevy::prelude::*;

use crate::settings::Settings;

pub struct HeadTrackPlugin;

impl Plugin for HeadTrackPlugin {
    fn build(&self, app: &mut App) {
        // Every state, so the menu shows whether a tracker is heard before the flight.
        app.init_resource::<HeadTrack>();
        // The browser has neither OpenTrack's UDP nor FreeTrack's shared memory.
        #[cfg(not(target_arch = "wasm32"))]
        app.add_systems(PreUpdate, receive);
    }
}

/// OpenTrack's "UDP over network" output port.
const UDP_PORT: u16 = 4242;
/// Seconds without new data before the tracker counts as stopped (the view returns to the
/// mouse look).
const STALE_SECONDS: f64 = 1.0;
/// Seconds between attempts to open the UDP port or the shared memory.
const RETRY_SECONDS: f64 = 2.0;
/// How far the head moves from the eye point (m, cockpit frame: right, up, back): keeps it
/// inside the canopy whatever the tracker's curves.
const HEAD_BOX: (Vec3, Vec3) = (Vec3::new(-0.3, -0.2, -0.35), Vec3::new(0.3, 0.2, 0.3));

/// A head pose in the cockpit frame: angles in radians (yaw left, pitch up, roll left: the
/// rotations about Bevy's +Y, +X and +Z), position in metres (right, up, back).
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct HeadPose {
    pub yaw: f32,
    pub pitch: f32,
    pub roll: f32,
    pub pos: Vec3,
}

/// Where the pose comes from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Source {
    Udp,
    FreeTrack,
}

impl Source {
    pub fn name(self) -> &'static str {
        match self {
            Source::Udp => "OpenTrack UDP",
            Source::FreeTrack => "FreeTrack",
        }
    }
}

#[derive(Resource, Default)]
pub struct HeadTrack {
    /// The input heard lately (None: off, or the tracker stopped sending).
    pub source: Option<Source>,
    /// The tracker's last pose, when it came (seconds since startup) and the centre (Home).
    raw: HeadPose,
    heard: f64,
    centre: HeadPose,
    udp: Retried<UdpSocket>,
    #[cfg(windows)]
    ft: Retried<freetrack::Mapping>,
}

impl HeadTrack {
    /// The recentred head pose while a tracker sends.
    pub fn pose(&self) -> Option<HeadPose> {
        self.source.map(|_| {
            let (r, c) = (self.raw, self.centre);
            HeadPose {
                yaw: r.yaw - c.yaw,
                pitch: r.pitch - c.pitch,
                roll: r.roll - c.roll,
                pos: (r.pos - c.pos).clamp(HEAD_BOX.0, HEAD_BOX.1),
            }
        })
    }
}

/// An input opened on first use and every `RETRY_SECONDS` while that fails.
struct Retried<T> {
    value: Option<T>,
    tried: Option<f64>,
}

impl<T> Default for Retried<T> {
    fn default() -> Self {
        Self { value: None, tried: None }
    }
}

impl<T> Retried<T> {
    /// The input, opened by `open` (told whether this is the first attempt) when due.
    fn get(&mut self, now: f64, open: impl FnOnce(bool) -> Option<T>) -> Option<&mut T> {
        if self.value.is_none() && self.tried.is_none_or(|t| now - t >= RETRY_SECONDS) {
            self.value = open(self.tried.is_none());
            self.tried = Some(now);
        }
        self.value.as_mut()
    }
}

/// The loopback UDP socket OpenTrack sends to (a failure is logged on the first attempt only).
fn open_udp(first: bool) -> Option<UdpSocket> {
    UdpSocket::bind(("127.0.0.1", UDP_PORT))
        .and_then(|s| s.set_nonblocking(true).map(|_| s))
        .inspect(|_| info!("head tracking: listening on UDP port {UDP_PORT}"))
        .inspect_err(|e| {
            if first {
                warn!("head tracking: UDP port {UDP_PORT}: {e}");
            }
        })
        .ok()
}

/// The last OpenTrack datagram waiting on the socket, if any.
fn read_udp(socket: &mut UdpSocket) -> Option<HeadPose> {
    let mut buf = [0u8; 64];
    let mut last = None;
    while let Ok((n, _)) = socket.recv_from(&mut buf) {
        last = parse_opentrack(&buf[..n]).or(last);
    }
    last
}

/// An OpenTrack "UDP over network" datagram: x, y, z (cm), yaw, pitch, roll (degrees) as
/// little-endian f64, sometimes followed by a frame number.
fn parse_opentrack(data: &[u8]) -> Option<HeadPose> {
    let words: &[[u8; 8]; 6] = data.as_chunks::<8>().0.get(..6)?.try_into().ok()?;
    let [x, y, z, yaw, pitch, roll] = words.map(f64::from_le_bytes);
    if ![x, y, z, yaw, pitch, roll].iter().all(|v| v.is_finite()) {
        return None;
    }
    Some(HeadPose {
        yaw: -yaw.to_radians() as f32,
        pitch: pitch.to_radians() as f32,
        roll: roll.to_radians() as f32,
        pos: Vec3::new(-x as f32, y as f32, z as f32) / 100.0,
    })
}

fn receive(
    time: Res<Time>,
    settings: Res<Settings>,
    keys: Res<ButtonInput<KeyCode>>,
    mut track: ResMut<HeadTrack>,
) {
    if !settings.head_tracking || settings.vr {
        // Closes the socket and the mapping, forgets the centre.
        *track = HeadTrack::default();
        return;
    }
    let track = &mut *track;
    let now = time.elapsed_secs_f64();
    let udp = track.udp.get(now, open_udp).and_then(read_udp);
    #[cfg(windows)]
    let ft = track.ft.get(now, |_| freetrack::Mapping::open()).and_then(|m| m.next());
    #[cfg(not(windows))]
    let ft = None;
    // Both at once: the UDP output wins.
    if let Some((pose, source)) =
        udp.map(|p| (p, Source::Udp)).or(ft.map(|p| (p, Source::FreeTrack)))
    {
        track.raw = pose;
        track.heard = now;
        track.source = Some(source);
    } else if now - track.heard > STALE_SECONDS {
        track.source = None;
    }
    if track.source.is_some() && keys.just_pressed(KeyCode::Home) {
        track.centre = track.raw;
        info!("head tracking: recentred");
    }
}

#[cfg(windows)]
mod freetrack {
    use bevy::prelude::*;
    use windows_sys::Win32::Foundation::{CloseHandle, HANDLE};
    use windows_sys::Win32::System::Memory::{
        FILE_MAP_READ, MEMORY_MAPPED_VIEW_ADDRESS, MapViewOfFile, OpenFileMappingW, UnmapViewOfFile,
    };

    use super::HeadPose;

    /// `FTData` (opentrack's freetrackclient/fttypes.h): DataID (u32), CamWidth, CamHeight
    /// (i32), then Yaw, Pitch, Roll, X, Y, Z (f32) and more.
    const POSE_OFFSET: usize = 12;
    const MAPPED: usize = 36;

    /// A read-only view of `FT_SharedMem`.
    pub struct Mapping {
        handle: HANDLE,
        view: MEMORY_MAPPED_VIEW_ADDRESS,
        /// The tracker's frame counter at the last read.
        frame: Option<u32>,
    }

    // SAFETY: the handle and the view are owned by the mapping and only read through it.
    unsafe impl Send for Mapping {}
    unsafe impl Sync for Mapping {}

    impl Mapping {
        /// The mapping, if a tracker program created it.
        pub fn open() -> Option<Self> {
            let name: Vec<u16> = "FT_SharedMem".encode_utf16().chain([0]).collect();
            // SAFETY: `name` is a NUL-terminated UTF-16 string.
            let handle = unsafe { OpenFileMappingW(FILE_MAP_READ, 0, name.as_ptr()) };
            if handle.is_null() {
                return None;
            }
            // SAFETY: `handle` is an open file mapping; the view covers the pose fields.
            let view = unsafe { MapViewOfFile(handle, FILE_MAP_READ, 0, 0, MAPPED) };
            if view.Value.is_null() {
                // SAFETY: the handle was opened above and is not used again.
                unsafe { CloseHandle(handle) };
                return None;
            }
            info!("head tracking: FreeTrack shared memory found");
            Some(Self { handle, view, frame: None })
        }

        /// The pose of a frame the tracker wrote since the last call. The memory outlives a
        /// stopped tracker, so the frame found on opening is not one.
        pub fn next(&mut self) -> Option<HeadPose> {
            let base = self.view.Value as *const u8;
            // SAFETY: the view is MAPPED bytes long and stays mapped while `self` lives; the
            // other process writes it, hence the volatile reads.
            let (frame, v, torn) = unsafe {
                let frame = std::ptr::read_volatile(base as *const u32);
                let v: [f32; 6] = std::array::from_fn(|i| {
                    std::ptr::read_volatile(base.add(POSE_OFFSET + i * 4) as *const f32)
                });
                (frame, v, std::ptr::read_volatile(base as *const u32) != frame)
            };
            if torn {
                return None;
            }
            let new = self.frame.replace(frame).is_some_and(|f| f != frame);
            new.then_some(v).and_then(pose)
        }
    }

    /// A FreeTrack pose: yaw, pitch, roll (radians; OpenTrack writes yaw and pitch negated,
    /// roll as in its UDP output), x, y, z (mm).
    pub(super) fn pose([yaw, pitch, roll, x, y, z]: [f32; 6]) -> Option<HeadPose> {
        [yaw, pitch, roll, x, y, z].iter().all(|v| v.is_finite()).then(|| HeadPose {
            yaw,
            pitch: -pitch,
            roll,
            pos: Vec3::new(-x, y, z) / 1000.0,
        })
    }

    impl Drop for Mapping {
        fn drop(&mut self) {
            // SAFETY: the view and the handle were opened by `open` and are released once.
            unsafe {
                UnmapViewOfFile(self.view);
                CloseHandle(self.handle);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn datagram(v: [f64; 6]) -> Vec<u8> {
        v.iter().flat_map(|x| x.to_le_bytes()).collect()
    }

    #[test]
    fn opentrack_axes() {
        // Looking right and up, head tilted right, leaning right, up and forward (OpenTrack:
        // roll and x positive to the left).
        let p = parse_opentrack(&datagram([-2.0, 3.0, -10.0, 30.0, 20.0, -10.0])).unwrap();
        let fwd = Quat::from_rotation_y(p.yaw) * Quat::from_rotation_x(p.pitch) * Vec3::NEG_Z;
        assert!(fwd.x > 0.0 && fwd.y > 0.0, "looks right and up: {fwd}");
        let up = Quat::from_rotation_z(p.roll) * Vec3::Y;
        assert!(up.x > 0.0, "tilted right: {up}");
        assert!((p.pos - Vec3::new(0.02, 0.03, -0.1)).length() < 1e-6);
        // A frame number after the pose is ignored; short or broken datagrams are not poses.
        let mut d = datagram([0.0; 6]);
        d.extend(7u32.to_le_bytes());
        assert_eq!(parse_opentrack(&d), Some(HeadPose::default()));
        assert_eq!(parse_opentrack(&d[..40]), None);
        assert_eq!(parse_opentrack(&datagram([f64::NAN, 0.0, 0.0, 0.0, 0.0, 0.0])), None);
    }

    #[cfg(windows)]
    #[test]
    fn freetrack_matches_udp() {
        // OpenTrack's freetrack output: yaw and pitch negated, radians, mm.
        let (x, y, z, yaw, pitch, roll) = (2.0f64, 3.0, -10.0, 30.0f64, 20.0f64, 10.0f64);
        let udp = parse_opentrack(&datagram([x, y, z, yaw, pitch, roll])).unwrap();
        let ft = freetrack::pose([
            -yaw.to_radians() as f32,
            -pitch.to_radians() as f32,
            roll.to_radians() as f32,
            (x * 10.0) as f32,
            (y * 10.0) as f32,
            (z * 10.0) as f32,
        ])
        .unwrap();
        assert!((udp.yaw - ft.yaw).abs() < 1e-6);
        assert!((udp.pitch - ft.pitch).abs() < 1e-6);
        assert!((udp.roll - ft.roll).abs() < 1e-6);
        assert!((udp.pos - ft.pos).length() < 1e-6);
    }

    #[test]
    fn recentred_pose_stays_in_the_cockpit() {
        let mut t = HeadTrack { source: Some(Source::Udp), ..default() };
        t.raw = HeadPose { yaw: 0.5, pitch: 0.2, roll: 0.0, pos: Vec3::new(0.0, 0.0, -2.0) };
        t.centre = HeadPose { yaw: 0.1, ..default() };
        let p = t.pose().unwrap();
        assert!((p.yaw - 0.4).abs() < 1e-6 && (p.pitch - 0.2).abs() < 1e-6);
        assert_eq!(p.pos.z, HEAD_BOX.0.z);
        assert_eq!(HeadTrack::default().pose(), None);
    }
}
