//! Theater map drawing shared by the mission planner and the debrief: the view's projection,
//! the map picture and the threat and airfield symbols.

use bevy_egui::egui;

use stealth_mission::Mission;
use stealth_mission::world::flags as F;

use crate::menu::DIM;

pub const MISSILE_RED: egui::Color32 = egui::Color32::from_rgb(255, 70, 50);
pub const UAV_PINK: egui::Color32 = egui::Color32::from_rgb(255, 120, 200);
pub const FRIENDLY_BLUE: egui::Color32 = egui::Color32::from_rgb(90, 160, 255);

/// A view of the map: map units (x east, y south) around `centre`, `scale` map units per
/// screen point, drawn into `rect`.
#[derive(Debug, Clone, Copy)]
pub struct MapView {
    pub rect: egui::Rect,
    pub centre: egui::Vec2,
    pub scale: f32,
}

impl MapView {
    pub fn to_screen(&self, x: f32, y: f32) -> egui::Pos2 {
        self.rect.center() + (egui::vec2(x, y) - self.centre) / self.scale
    }

    pub fn to_map(&self, p: egui::Pos2) -> egui::Vec2 {
        self.centre + (p - self.rect.center()) * self.scale
    }

    /// Centre and scale that show the box `lo..hi` (map units) with a margin.
    pub fn fit(rect: egui::Rect, lo: egui::Vec2, hi: egui::Vec2) -> (egui::Vec2, f32) {
        let scale =
            ((hi.x - lo.x) / rect.width()).max((hi.y - lo.y) / rect.height()).max(4.0) * 1.4;
        ((lo + hi) / 2.0, scale)
    }

    /// Wheel zoom around the pointer and drag to pan (map units are written back).
    pub fn pan_zoom(
        &self,
        ui: &egui::Ui,
        resp: &egui::Response,
        centre: &mut egui::Vec2,
        scale: &mut f32,
    ) {
        if let Some(pos) = resp.hover_pos() {
            let scroll = ui.input(|i| i.smooth_scroll_delta.y);
            if scroll != 0.0 {
                let before = self.to_map(pos);
                *scale = (*scale * (-scroll * 0.002).exp()).clamp(1.0, 64.0);
                *centre = before - (pos - self.rect.center()) * *scale;
            }
        }
        if resp.dragged() {
            *centre -= resp.drag_delta() * *scale;
        }
    }
}

/// The map picture (the whole theater), or a note while it is being prepared.
pub fn picture(painter: &egui::Painter, view: &MapView, tex: Option<&egui::TextureHandle>) {
    painter.rect_filled(view.rect, 0.0, egui::Color32::BLACK);
    match tex {
        Some(tex) => {
            let r = egui::Rect::from_min_max(
                view.to_screen(0.0, 0.0),
                view.to_screen(0x8000 as f32, 0x8000 as f32),
            );
            painter.image(
                tex.id(),
                r,
                egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)),
                egui::Color32::WHITE,
            );
        }
        None => {
            painter.text(
                view.rect.center(),
                egui::Align2::CENTER_CENTER,
                "LOADING MAP...",
                egui::FontId::monospace(18.0),
                DIM,
            );
        }
    }
}

/// A SAM site's missile range: a dotted red circle.
pub fn missile_circle(c: egui::Pos2, r: f32, alpha: f32) -> Vec<egui::Shape> {
    let n = ((r * std::f32::consts::TAU / 2.0) as usize).clamp(24, 720);
    let pts: Vec<egui::Pos2> = (0..=n)
        .map(|k| c + egui::Vec2::angled(k as f32 / n as f32 * std::f32::consts::TAU) * r)
        .collect();
    egui::Shape::dashed_line(
        &pts,
        egui::Stroke::new(1.4, MISSILE_RED.gamma_multiply(alpha * 0.8)),
        3.0,
        4.0,
    )
}

/// A site: a red dot with its name once zoomed in far enough.
pub fn site(painter: &egui::Painter, view: &MapView, c: egui::Pos2, name: &str, alpha: f32) {
    painter.circle_filled(c, 3.0, MISSILE_RED.gamma_multiply(alpha));
    if 1.0 / view.scale > 0.02 {
        painter.text(
            c + egui::vec2(5.0, -5.0),
            egui::Align2::LEFT_BOTTOM,
            name,
            egui::FontId::monospace(11.0),
            MISSILE_RED.gamma_multiply(alpha),
        );
    }
}

/// The mission's airfields: blue friendly, dim neutral, red hostile squares.
pub fn airfields(painter: &egui::Painter, view: &MapView, m: &Mission) {
    for o in m.world.objects.iter().filter(|o| o.flags & F::AIRFIELD != 0) {
        let p = view.to_screen(o.x as u16 as f32, o.y as u16 as f32);
        let colour = if o.flags & F::FRIENDLY != 0 {
            FRIENDLY_BLUE
        } else if o.flags & F::NEUTRAL != 0 {
            DIM
        } else {
            MISSILE_RED
        };
        painter.rect_stroke(
            egui::Rect::from_center_size(p, egui::vec2(8.0, 8.0)),
            0.0,
            egui::Stroke::new(1.5, colour),
            egui::StrokeKind::Middle,
        );
    }
}

/// A drone on the map: a pink cross.
pub fn drone_cross(painter: &egui::Painter, p: egui::Pos2) {
    let stroke = egui::Stroke::new(1.5, UAV_PINK);
    painter.line_segment([p + egui::vec2(-4.0, -4.0), p + egui::vec2(4.0, 4.0)], stroke);
    painter.line_segment([p + egui::vec2(-4.0, 4.0), p + egui::vec2(4.0, -4.0)], stroke);
}

/// A legend line: a symbol drawn by `draw` into a small box, then its meaning.
pub fn legend(ui: &mut egui::Ui, text: &str, draw: impl FnOnce(&egui::Painter, egui::Pos2)) {
    ui.horizontal(|ui| {
        let (rect, _) = ui.allocate_exact_size(egui::vec2(18.0, 14.0), egui::Sense::hover());
        draw(&ui.painter_at(rect.expand(1.0)), rect.center());
        ui.label(egui::RichText::new(text).small().color(crate::menu::BRIGHT));
    });
}
