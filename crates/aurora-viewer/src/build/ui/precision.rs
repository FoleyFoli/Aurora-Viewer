//! Numeric terrain tools shared by the compact and legacy Build floaters.

use super::common;
use crate::build::precision::{PrecisionMode, PrecisionTerrain};
use crate::build::{BuildSettings, BuildTool};
use crate::theme::Palette;
use crate::ui::widgets;
use crate::world::World;
use aurora_i18n::{number, tr};
use egui::{Align2, Pos2, Sense, Stroke, pos2, vec2};

pub(super) fn panel(ui: &mut egui::Ui, p: &Palette, tool: &mut BuildTool, world: &World, settings: &BuildSettings) {
    let section = widgets::collapsing(ui, p, "precision_terrain", tr!("build-precision-title"), false, |ui| {
        let choices = [
            (tr!("build-precision-brushes"), PrecisionMode::Off),
            (tr!("build-precision-measure"), PrecisionMode::Measure),
            (tr!("build-precision-set"), PrecisionMode::SetHeight),
        ];
        let width = ui.available_width().min(180.0);
        if let Some(mode) = common::combo(ui, "precision_mode", Some(tool.land.precision.mode), &choices, width, true) {
            tool.land.set_precision_mode(mode);
        }
        let work = &mut tool.land.precision;
        if work.mode == PrecisionMode::Off {
            return;
        }
        let measure = work.mode == PrecisionMode::Measure;
        if work.b.is_none() {
            let hint = match (measure, work.a.is_some()) {
                (true, false) => tr!("build-precision-measure-hint"),
                (false, false) => tr!("build-precision-set-hint"),
                (true, true) => tr!("build-precision-end-hint"),
                (false, true) => tr!("build-precision-corner-hint"),
            };
            common::small(ui, p, hint);
        }
        if let Some(a) = work.a {
            show_point(ui, p, work, world, a, tr!("build-precision-point-a"));
        }
        if let Some(b) = work.b {
            show_point(ui, p, work, world, b, tr!("build-precision-point-b"));
        }
        if !measure && work.a.is_some() {
            height_controls(ui, p, work, world);
        }
        if let (Some(a), Some(b)) = (work.a, work.b) {
            let distance = a.distance(b);
            if !measure {
                common::small(
                    ui,
                    p,
                    &tr!(
                        "build-precision-extent",
                        width = number(f64::from((b.x - a.x).abs()), 0),
                        length = number(f64::from((b.y - a.y).abs()), 0)
                    ),
                );
            }
            if distance > 0.0
                && let Some(samples) = work.profile(world)
                && samples.len() >= 2
            {
                let rise = samples[samples.len() - 1] - samples[0];
                common::small(
                    ui,
                    p,
                    &tr!(
                        "build-precision-stats",
                        run = number(distance.into(), 2),
                        rise = number(rise.into(), 2),
                        grade = number(f64::from(100.0 * rise / distance), 1)
                    ),
                );
                elevation_profile(ui, p, &samples, (!measure).then_some(work.target_z), work.water(world));
            }
        }
        if let Some(message) = &work.message {
            common::small(ui, p, message);
        }
        if let Some((actual, submitted)) = work.observed_center(world) {
            common::small(
                ui,
                p,
                &tr!(
                    "build-precision-observed",
                    height = number(actual.into(), 2),
                    difference = number(f64::from(actual - submitted), 2)
                ),
            );
        }
        let ready = work.can_apply(world);
        if !measure
            && work.b.is_some()
            && let Err(reason) = ready
        {
            common::small(ui, p, reason);
        }
        let mut cmd = None;
        ui.horizontal_wrapped(|ui| {
            if !measure
                && ui
                    .add_enabled_ui(ready.is_ok(), |ui| {
                        widgets::primary_button_sized(ui, p, tr!("build-precision-apply"), vec2(138.0, 24.0))
                    })
                    .inner
                    .clicked()
            {
                cmd = work.apply(world, settings.land_brush_force);
            }
            if work.a.is_some()
                && widgets::flat_button(
                    ui,
                    p,
                    if measure || work.submitted() {
                        tr!("common-clear")
                    } else {
                        tr!("build-precision-discard")
                    },
                )
                .clicked()
            {
                work.clear();
            }
        });
        if let (Some(handle), Some(cmd)) = (work.region, cmd) {
            tool.land.last_regions.clear();
            tool.land.last_regions.push(handle);
            tool.send(cmd);
        }
    });
    // A closed inspector must never keep intercepting clicks in the world.
    if (section.header.changed() || section.body.is_none()) && tool.land.precision.mode != PrecisionMode::Off {
        tool.land.set_precision_mode(PrecisionMode::Off);
    }
}

fn show_point(ui: &mut egui::Ui, p: &Palette, work: &PrecisionTerrain, world: &World, point: glam::Vec2, name: &str) {
    let z = work.sample(world, point);
    let fmt = |v: Option<f32>| v.map(|n| number(n.into(), 2)).unwrap_or_else(|| "—".into());
    common::small(
        ui,
        p,
        &tr!(
            "build-precision-coordinates",
            label = name,
            x = number(point.x.into(), 0),
            y = number(point.y.into(), 0),
            z = fmt(z),
            offset = fmt(z.zip(work.water(world)).map(|(z, water)| z - water))
        ),
    );
}

fn height_controls(ui: &mut egui::Ui, p: &Palette, work: &mut PrecisionTerrain, world: &World) {
    let water = work.water(world);
    let references = [(tr!("build-precision-region-z"), false), (tr!("build-precision-water"), true)];
    ui.horizontal_wrapped(|ui| {
        common::label(ui, p, tr!("build-precision-reference"));
        if let Some(relative) = common::combo(
            ui,
            "precision_reference",
            Some(work.relative_to_water),
            &references,
            108.0,
            water.is_some(),
        ) {
            work.relative_to_water = relative;
        }
    });
    let datum = if work.relative_to_water { water } else { Some(0.0) };
    ui.horizontal_wrapped(|ui| {
        common::label(ui, p, tr!("build-precision-target"));
        if let Some(datum) = datum {
            let mut height = work.target_z - datum;
            if common::spin(ui, &mut height, 0.1, f32::MIN..=f32::MAX, 2, 78.0, false).changed {
                work.target_z = height + datum;
                work.message = None;
            }
        } else {
            common::empty_spin(ui, 78.0);
        }
        if let Some(z) = work.a.and_then(|pt| work.sample(world, pt))
            && widgets::flat_button(ui, p, tr!("build-precision-use-a")).clicked()
        {
            work.target_z = z;
            work.message = None;
        }
    });
}

fn elevation_profile(ui: &mut egui::Ui, p: &Palette, heights: &[f32], target: Option<f32>, water: Option<f32>) {
    let target = target.filter(|z| z.is_finite());
    common::label(ui, p, tr!("build-precision-profile"));
    let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), 64.0), Sense::hover());
    if !ui.is_rect_visible(rect) || rect.width() < 75.0 {
        return;
    }
    let chart = egui::Rect::from_min_max(rect.min + vec2(47.0, 7.0), rect.max - vec2(2.0, 7.0));
    let min = heights.iter().copied().chain(target).fold(f32::INFINITY, f32::min);
    let max = heights.iter().copied().chain(target).fold(f32::NEG_INFINITY, f32::max);
    let spread = (max - min).max(0.20);
    let lo = min - (spread - (max - min)) * 0.5;
    let hi = lo + spread;
    let y = |z: f32| chart.bottom() - (z - lo) / spread * chart.height();
    let painter = ui.painter();
    for (z, edge) in [(hi, chart.top()), (lo, chart.bottom())] {
        painter.text(
            pos2(chart.left() - 4.0, edge),
            Align2::RIGHT_CENTER,
            number(z.into(), 2),
            widgets::control_font(ui, 11.0),
            p.muted,
        );
        painter.line_segment([pos2(chart.left(), edge), pos2(chart.right(), edge)], Stroke::new(0.5, p.muted_dim));
    }
    // Keep the ground readable on high land. The water line appears when it
    // falls within the profile's elevation range; it never expands the scale.
    let visible_water = water.filter(|z| (lo..=hi).contains(z));
    if let Some(z) = visible_water {
        let mut x = chart.left();
        while x < chart.right() {
            painter.line_segment(
                [pos2(x, y(z)), pos2((x + 4.0).min(chart.right()), y(z))],
                Stroke::new(1.25, p.axis_z),
            );
            x += 7.0;
        }
    }
    if let Some(z) = target {
        let y = y(z);
        painter.line_segment([pos2(chart.left(), y), pos2(chart.right(), y)], Stroke::new(1.5, p.violet_light));
    }
    let vertices: Vec<Pos2> = heights
        .iter()
        .enumerate()
        .map(|(i, &z)| {
            let x = chart.left() + chart.width() * i as f32 / (heights.len() - 1) as f32;
            pos2(x, y(z))
        })
        .collect();
    painter.add(egui::Shape::line(vertices, Stroke::new(1.5, p.teal)));
    ui.horizontal_wrapped(|ui| {
        ui.colored_label(p.teal, tr!("build-precision-received"));
        if let Some(z) = visible_water {
            ui.colored_label(p.axis_z, tr!("build-precision-profile-water", height = number(z.into(), 2)));
        }
        if target.is_some() {
            ui.colored_label(p.violet_light, tr!("build-precision-requested"));
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use aurora_net::{NetCommand, ParcelInfo, build::BuildCmd};
    use std::sync::Arc;

    fn text_rect(shape: &egui::Shape, text: &str) -> Option<egui::Rect> {
        match shape {
            egui::Shape::Text(t) if t.galley.text() == text => Some(egui::Rect::from_min_size(t.pos, t.galley.size())),
            egui::Shape::Vec(children) => children.iter().find_map(|s| text_rect(s, text)),
            _ => None,
        }
    }

    /// Real pointer presses in a legacy-width panel: opening and editing a
    /// preview send nothing; only the explicit primary button sends one edit.
    #[test]
    fn narrow_precision_panel_applies_only_on_request() {
        let ctx = egui::Context::default();
        ctx.all_styles_mut(|s| s.animation_time = 0.0);
        let mut world = World::new(Arc::new(crate::scene::avatar::AvatarLibrary::load()));
        for event in crate::demo::events() {
            world.apply(event);
        }
        let mut tool = BuildTool::default();
        let settings = BuildSettings::default();
        let palette = crate::theme::Theme::default().palette();

        let frame = |tool: &mut BuildTool, events: Vec<egui::Event>, find: &str| {
            let output = crate::ui::login::test_output(ctx.run_ui(
                egui::RawInput {
                    screen_rect: Some(egui::Rect::from_min_size(Pos2::ZERO, vec2(275.0, 800.0))),
                    focused: true,
                    events,
                    ..Default::default()
                },
                |ui| panel(ui, &palette, tool, &world, &settings),
            ));
            output.shapes.iter().find_map(|s| text_rect(&s.shape, find))
        };
        let click = |p: Pos2, pressed| {
            vec![
                egui::Event::PointerMoved(p),
                egui::Event::PointerButton {
                    pos: p,
                    button: egui::PointerButton::Primary,
                    pressed,
                    modifiers: egui::Modifiers::NONE,
                },
            ]
        };
        let heading = tr!("build-precision-title");
        let pos = frame(&mut tool, vec![], heading).expect("disclosure").center();
        frame(&mut tool, click(pos, true), heading);
        frame(&mut tool, click(pos, false), heading);
        assert_eq!(tool.land.precision.mode, PrecisionMode::Off);

        tool.land.set_precision_mode(PrecisionMode::SetHeight);
        let work = &mut tool.land.precision;
        work.pick(&world, glam::vec3(4.0, 4.0, 0.0));
        work.pick(&world, glam::vec3(12.0, 8.0, 0.0));
        let Some(BuildCmd::ParcelRequest { handle, sequence, .. }) = work.take_parcel_request() else {
            panic!("expected parcel lookup")
        };
        work.on_parcel(
            handle,
            sequence,
            &Arc::new(ParcelInfo {
                request_result: aurora_net::land::PARCEL_RESULT_SUCCESS,
                flags: aurora_net::parcel_flags::ALLOW_TERRAFORM,
                bitmap: vec![255; 512],
                ..Default::default()
            }),
        );
        work.target_z = 23.5;
        let apply = tr!("build-precision-apply");
        frame(&mut tool, vec![], apply);
        let button = frame(&mut tool, vec![], apply).expect("full apply label fits");
        assert!(button.left() >= 0.0 && button.right() <= 275.0);
        assert!(tool.out.is_empty());
        let center = button.center();
        frame(&mut tool, click(center, true), apply);
        frame(&mut tool, click(center, false), apply);
        assert!(matches!(tool.out.as_slice(), [NetCommand::Build(BuildCmd::ModifyLand { height, .. })] if *height == 23.5));
        assert_eq!(tool.land.last_regions, [handle]);

        frame(&mut tool, click(pos, true), heading);
        frame(&mut tool, click(pos, false), heading);
        assert_eq!(tool.land.precision.mode, PrecisionMode::Off);
        assert_eq!(tool.out.len(), 1);
    }
}
