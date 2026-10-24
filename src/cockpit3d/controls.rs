//! Pushable cockpit controls.
//!
//! Panel buttons are caps carrying a face of the instrument atlas (a lamp or label, so lit/unlit
//! comes from the panel drawing code); the layout says which faces are caps. The modern
//! panel's MFD bezel buttons are caps too, with page-dependent labels (`mfd::osb`). Extra
//! buttons on two side panels carry text labels. Every control has a local frame with +Z pointing out of
//! its face. A VR fingertip touching a control highlights it (with a light haptic tick) and
//! pulling the trigger then fires it; a mouse click or a script `press=` fires it directly.

use bevy::prelude::*;

use super::Hands;
use super::layout::Effect;
use super::pick::{Picker, Pointed};
use crate::camera::{View, ViewState};
use crate::flight::{Aircraft, Player};
use crate::hud::{HudMode, HudState};
use crate::input::{Action, ActionEvent};
use crate::mfd::{CamDir, MfdCommand, MfdState, OsbFn, cmd_action, osb};

/// Presses the 3D-cockpit control bound to an action (scripts).
#[derive(Message, Debug, Clone, Copy)]
pub struct PressControl(pub Action);

impl Effect {
    /// The key action a script `press=` names for this control (None: blank bezel button).
    pub(super) fn action(self, mfd: &MfdState) -> Option<Action> {
        Some(match self {
            Effect::Action(a) => a,
            Effect::HudMode(_) => Action::HudMode,
            Effect::LeftPage(_) => Action::Maps,
            Effect::CamCycle => Action::CamAhead,
            Effect::TimeToggle => Action::TimeAccel,
            Effect::Osb(side, slot) => match osb(side, slot, mfd)?.1 {
                OsbFn::Cmd(c) => cmd_action(c),
                OsbFn::Action(a) => a,
                OsbFn::Readout => return None,
            },
        })
    }
}

#[derive(Clone, Copy, PartialEq)]
enum Kind {
    /// Cap on the panel carrying an atlas face (uv min, max), size in panel pixels.
    Cap,
    /// MFD bezel button.
    Osb,
    /// Labelled button on a side panel.
    Side,
    /// Landing gear lever (left of the panel).
    GearLever,
    /// Ejection handle (front of the seat pan).
    EjectHandle,
}

/// Fingertip radius.
const FINGER: f32 = 0.008;
/// A fingertip touches a control within this distance of its face (in front or pushed in), and
/// this far beyond its footprint.
const TOUCH_GAP: f32 = 0.012;
const TOUCH_MARGIN: f32 = 0.006;

#[derive(Component)]
pub(super) struct Control {
    kind: Kind,
    pub(super) effect: Effect,
    half: Vec2,
    height: f32,
    travel: f32,
    /// Current push depth (metres), from the fingers or a click animation.
    depth: f32,
    /// A VR fingertip touches it.
    touched: bool,
    /// Seconds left of a click/script press animation.
    click: f32,
}

/// The moving part of a control (cap, lever arm).
#[derive(Component)]
pub(super) struct Moving;

/// Frame around a control shown while a fingertip touches it.
#[derive(Component)]
pub(super) struct Highlight;

impl Control {
    /// A control from the cockpit model's object properties: `kind` (cap, osb, side, gear,
    /// eject), `control` (the effect, e.g. `Action(Gear)`, `HudMode(1)`, `Osb(Left, 3)`),
    /// `half` (half width and height of the face, metres), `height` (face above the origin) and
    /// `travel` (press depth).
    pub(super) fn from_model(extras: &serde_json::Value) -> Option<Self> {
        let kind = match extras["kind"].as_str()? {
            "cap" => Kind::Cap,
            "osb" => Kind::Osb,
            "side" => Kind::Side,
            "gear" => Kind::GearLever,
            "eject" => Kind::EjectHandle,
            _ => return None,
        };
        let effect = parse_effect(extras["control"].as_str()?)?;
        let f = |v: &serde_json::Value| v.as_f64().map(|x| x as f32);
        let half = Vec2::new(f(&extras["half"][0])?, f(&extras["half"][1])?);
        Some(Self {
            kind,
            effect,
            half,
            height: f(&extras["height"])?,
            travel: f(&extras["travel"])?,
            depth: 0.0,
            touched: false,
            click: 0.0,
        })
    }

    /// A lamp face that is pushed as a whole (the master caution): `half` its half size
    /// (metres), flat on its object's origin, not moving.
    pub(super) fn push_light(effect: Effect, half: Vec2) -> Self {
        Self {
            kind: Kind::Cap,
            effect,
            half,
            height: 0.0,
            travel: 0.0,
            depth: 0.0,
            touched: false,
            click: 0.0,
        }
    }

    pub(super) fn is_eject(&self) -> bool {
        self.kind == Kind::EjectHandle
    }

    /// A VR fingertip touches it.
    pub(super) fn touched(&self) -> bool {
        self.touched
    }

    /// Where the ray o + t d (cockpit frame, `tf` the control's) goes through the face
    /// rectangle: t.
    pub(super) fn face_hit(&self, tf: &Transform, o: Vec3, d: Vec3) -> Option<f32> {
        let n = tf.rotation * Vec3::Z;
        let plane = tf.translation + n * self.height;
        let denom = d.dot(n);
        if denom.abs() < 1e-9 {
            return None;
        }
        let t = (plane - o).dot(n) / denom;
        let local = tf.rotation.inverse() * (o + d * t - tf.translation);
        (t > 0.0 && local.x.abs() <= self.half.x && local.y.abs() <= self.half.y).then_some(t)
    }
}

/// Parses an effect as written in the cockpit model (`Effect`'s `Debug` text).
pub(super) fn parse_effect(s: &str) -> Option<Effect> {
    let s = s.trim();
    let (name, args) = match s.split_once('(') {
        Some((n, rest)) => {
            (n.trim(), rest.strip_suffix(')')?.split(',').map(str::trim).collect::<Vec<_>>())
        }
        None => (s, Vec::new()),
    };
    Some(match (name, args.as_slice()) {
        ("Action", [a]) => Effect::Action(crate::input::action_by_name(a)?),
        ("HudMode", [n]) => Effect::HudMode(n.parse().ok()?),
        ("LeftPage", ["Map"]) => Effect::LeftPage(crate::mfd::LeftMfd::Map),
        ("LeftPage", ["Tac"]) => Effect::LeftPage(crate::mfd::LeftMfd::Tac),
        ("CamCycle", []) => Effect::CamCycle,
        ("TimeToggle", []) => Effect::TimeToggle,
        ("Osb", [side, n]) => Effect::Osb(
            match *side {
                "Left" => crate::mfd::MfdSide::Left,
                "Right" => crate::mfd::MfdSide::Right,
                "Third" => crate::mfd::MfdSide::Third,
                _ => return None,
            },
            n.parse().ok()?,
        ),
        _ => return None,
    })
}

#[derive(bevy::ecs::system::SystemParam)]
pub(super) struct Fire<'w, 's> {
    actions: MessageWriter<'w, ActionEvent>,
    mfd_commands: MessageWriter<'w, MfdCommand>,
    hud: Res<'w, HudState>,
    mfd: Res<'w, MfdState>,
    accel: Res<'w, crate::combat::TimeAccel>,
    _p: std::marker::PhantomData<&'s ()>,
}

impl Fire<'_, '_> {
    fn fire(&mut self, effect: Effect) {
        match effect {
            Effect::Action(a) => {
                self.actions.write(ActionEvent(a));
            }
            Effect::HudMode(target) => {
                let cur = match self.hud.mode {
                    HudMode::Nav => 0,
                    HudMode::Air => 1,
                    HudMode::Gnd => 2,
                };
                // A-G again switches between CCIP and CCRP; A-A again goes back to NAV (the
                // F-16's A-A / A-G).
                if target == 2 && cur == 2 {
                    self.actions.write(ActionEvent(Action::BombMode));
                    return;
                }
                let target = if target == cur { 0 } else { target };
                for _ in 0..(target + 3 - cur) % 3 {
                    self.actions.write(ActionEvent(Action::HudMode));
                }
            }
            Effect::LeftPage(p) => {
                if self.mfd.left != p {
                    self.actions.write(ActionEvent(Action::Maps));
                }
            }
            Effect::Osb(side, slot) => match osb(side, slot, &self.mfd).map(|o| o.1) {
                Some(OsbFn::Cmd(c)) => {
                    self.mfd_commands.write(MfdCommand(c));
                }
                Some(OsbFn::Action(a)) => {
                    self.actions.write(ActionEvent(a));
                }
                Some(OsbFn::Readout) | None => {}
            },
            Effect::CamCycle => {
                let next = match self.mfd.cam_dir {
                    CamDir::Ahead => Action::CamRight,
                    CamDir::Right => Action::CamRear,
                    CamDir::Rear => Action::CamLeft,
                    CamDir::Left => Action::CamAhead,
                };
                self.actions.write(ActionEvent(next));
            }
            Effect::TimeToggle => {
                self.actions.write(ActionEvent(if self.accel.on {
                    Action::TimeNormal
                } else {
                    Action::TimeAccel
                }));
            }
        }
    }
}

/// Seconds a clicked control stays pushed in.
const CLICK_SECONDS: f32 = 0.18;

/// Fingertip touches with the trigger, mouse clicks and script presses.
#[allow(clippy::too_many_arguments)]
pub(super) fn interact(
    time: Res<Time<Real>>,
    settings: Res<crate::settings::Settings>,
    view: Res<ViewState>,
    mut hands: ResMut<Hands>,
    mut controls: Query<(Entity, &mut Control, &Transform)>,
    mut was_touching: Local<[Option<Entity>; 2]>,
    mut fire: Fire,
    mut presses: MessageReader<PressControl>,
    buttons: Res<ButtonInput<MouseButton>>,
    mut picker: Picker,
    menu: Res<crate::flight_menu::FlightMenu>,
    hide: Option<Res<crate::camera::HideOwnAircraft>>,
) {
    let dt = time.delta_secs();
    // The cockpit is not there (VR, the campaign's Ops screen): nothing to touch.
    if hide.is_some_and(|h| h.0) {
        presses.clear();
        for hand in hands.hands.iter_mut() {
            hand.touching = false;
        }
        *was_touching = [None; 2];
        for (_, mut c, _) in &mut controls {
            c.click = 0.0;
            c.depth = 0.0;
            if c.touched {
                c.touched = false;
            }
        }
        return;
    }
    // With the menu open only the MENU button works (it closes the menu again).
    let usable = |c: &Control| !menu.open || c.effect == Effect::Action(Action::Menu);
    for PressControl(a) in presses.read() {
        // Panel caps and bezel buttons before the side panel duplicates; of two bezel buttons
        // with the same key action (MAP / TAC), the one not selected.
        let mfd = fire.mfd.clone();
        let rank = |c: &Control| {
            let lit = matches!(c.effect, Effect::Osb(side, slot) if osb(side, slot, &mfd).is_some_and(|o| o.2));
            (!matches!(c.kind, Kind::Cap | Kind::Osb), lit)
        };
        if let Some((_, mut c, _)) = controls
            .iter_mut()
            .filter(|(_, c, _)| c.effect.action(&mfd) == Some(*a))
            .min_by_key(|(_, c, _)| rank(c))
        {
            c.click = CLICK_SECONDS;
            fire.fire(c.effect);
        } else {
            warn!("no 3D cockpit control for {a:?}");
        }
    }
    let cockpit_view =
        matches!(view.view, View::Cockpit | View::LookLeft | View::LookRight | View::LookRear);

    // Mouse click: the control under the cursor, unless something in front hides it.
    if !settings.vr
        && cockpit_view
        && buttons.just_pressed(MouseButton::Left)
        && let Some(ray) = picker.cursor_ray()
    {
        let picked = {
            let all: Vec<_> = controls.iter().collect();
            picker.pick(ray, &all, usable)
        };
        if let Some(Pointed::Control(e)) = picked
            && let Ok((_, mut c, _)) = controls.get_mut(e)
            && usable(&c)
        {
            c.click = CLICK_SECONDS;
            fire.fire(c.effect);
        }
    }

    // VR fingertips: each hand touches the control whose face is nearest its fingertip (not
    // while it holds the stick or throttle); the trigger fires it and holds it pushed in.
    let mut touching: [Option<(f32, Entity)>; 2] = [None; 2];
    for (e, mut c, tf) in &mut controls {
        c.click = (c.click - dt).max(0.0);
        for (h, hand) in hands.hands.iter().enumerate() {
            // The ejection handle has to be grabbed and pulled (no accidental pokes).
            if !hand.active || !cockpit_view || c.kind == Kind::EjectHandle || !usable(&c) {
                continue;
            }
            let local = tf.rotation.inverse() * (hand.finger - tf.translation);
            let inside = local.x.abs() <= c.half.x + TOUCH_MARGIN
                && local.y.abs() <= c.half.y + TOUCH_MARGIN
                && local.z > -0.04;
            if !inside {
                continue;
            }
            let face = c.height + FINGER;
            let gap = local.z - face;
            if hand.squeeze < 0.4
                && gap < TOUCH_GAP
                && gap > -c.travel - TOUCH_GAP
                && touching[h].is_none_or(|(g, _)| gap.abs() < g)
            {
                touching[h] = Some((gap.abs(), e));
            }
        }
        c.depth = if c.click > 0.0 { c.travel } else { 0.0 };
    }
    for (h, hand) in hands.hands.iter_mut().enumerate() {
        let touched = touching[h].map(|(_, e)| e);
        hand.touching = touched.is_some();
        if touched.is_some() && touched != was_touching[h] {
            hand.haptic = hand.haptic.max(0.1);
        }
        was_touching[h] = touched;
        if let Some(e) = touched
            && let Ok((_, mut c, _)) = controls.get_mut(e)
        {
            if hand.trigger_pulled() {
                hand.haptic = hand.haptic.max(0.6);
                c.click = CLICK_SECONDS;
                fire.fire(c.effect);
            }
            if hand.trigger > 0.7 {
                c.depth = c.travel;
            }
        }
    }
    for (e, mut c, _) in &mut controls {
        let touched = touching.iter().flatten().any(|&(_, t)| t == e);
        if c.touched != touched {
            c.touched = touched;
        }
    }
}

/// Moves the caps in, the gear lever to the gear position, and shows the touched controls'
/// highlights.
pub(super) fn animate(
    controls: Query<(&Control, &Children)>,
    mut moving: Query<(&mut Transform, &Children), With<Moving>>,
    mut highlights: Query<&mut Visibility, With<Highlight>>,
    aircraft: Query<&Aircraft, With<Player>>,
) {
    let gear = aircraft.single().is_ok_and(|a| a.gear_down);
    for (c, children) in &controls {
        for child in children.iter() {
            if let Ok((mut tf, parts)) = moving.get_mut(child) {
                for part in parts.iter() {
                    if let Ok(mut v) = highlights.get_mut(part) {
                        v.set_if_neq(if c.touched {
                            Visibility::Inherited
                        } else {
                            Visibility::Hidden
                        });
                    }
                }
                let mut t = Transform::from_xyz(0.0, 0.0, -c.depth);
                if c.kind == Kind::GearLever {
                    t.rotation = Quat::from_rotation_x(if gear { 0.5 } else { -0.5 });
                }
                if *tf != t {
                    *tf = t;
                }
            }
        }
    }
}
