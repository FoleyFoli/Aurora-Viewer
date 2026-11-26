//! The measured ground and proposed leveling plane are temporary build guides.

use super::draw::{Painter3d, fade};
use super::precision::{PrecisionMode, PrecisionTerrain};
use crate::world::World;
use glam::{Vec2, Vec3};

pub(super) fn draw(tool: &PrecisionTerrain, hover: Option<Vec3>, painter: &mut Painter3d, world: &World) {
    let ground_color = painter.pal.teal;
    let target_color = painter.pal.violet_light;
    if let Some(point) = hover {
        painter.square(point, 7.0, fade(ground_color, 0.35), egui::Stroke::new(1.0, ground_color));
    }
    let (Some(handle), Some(a)) = (tool.region, tool.a) else { return };
    if Some(handle) != world.main_region {
        return;
    }
    let Some(origin) = world.region_offset(handle) else { return };
    let ground = |xy: Vec2| tool.sample(world, xy).map(|z| origin + xy.extend(z));
    let b = tool.b.or_else(|| hover.map(|p| (p - origin).truncate()));
    if let Some(b) = b {
        match tool.mode {
            PrecisionMode::Measure => draw_ground_trace(painter, a, b, ground),
            PrecisionMode::SetHeight if tool.target_z.is_finite() => {
                let (lo, hi) = (a.min(b), a.max(b));
                let corners = [lo, Vec2::new(hi.x, lo.y), hi, Vec2::new(lo.x, hi.y)];
                let plane = corners.map(|xy| origin + xy.extend(tool.target_z));
                if lo.x < hi.x && lo.y < hi.y {
                    painter.quad(plane, fade(target_color, 0.12));
                    painter.flush();
                }
                for i in 0..4 {
                    let next = (i + 1) % 4;
                    draw_ground_trace(painter, corners[i], corners[next], ground);
                    painter.line_cased(plane[i], plane[next], target_color, 2.0);
                    if let Some(foot) = ground(corners[i]) {
                        painter.guide_line(foot, plane[i], target_color, 0.7);
                    }
                }
            }
            _ => {}
        }
    }

    for (xy, label) in [
        (Some(a), aurora_i18n::tr!("build-precision-point-a")),
        (tool.b, aurora_i18n::tr!("build-precision-point-b")),
    ] {
        if let Some(foot) = xy.and_then(ground) {
            let top = foot + Vec3::Z * 0.7;
            painter.line_cased(foot, top, ground_color, 1.25);
            painter.square(foot, 5.0, ground_color, egui::Stroke::NONE);
            painter.text(top, label, 13.0, ground_color);
        }
    }
}

/// Follow the received surface and leave gaps wherever terrain hasn't arrived.
fn draw_ground_trace(painter: &Painter3d, a: Vec2, b: Vec2, ground: impl Fn(Vec2) -> Option<Vec3>) {
    let steps = (a.distance(b).ceil() as usize).clamp(1, 256);
    let mut prev = ground(a);
    for i in 1..=steps {
        let next = ground(a.lerp(b, i as f32 / steps as f32));
        if let (Some(start), Some(end)) = (prev, next) {
            painter.line_cased(start, end, painter.pal.teal, 1.5);
        }
        prev = next;
    }
}
