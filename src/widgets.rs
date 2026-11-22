//! Custom-painted widgets: cards, gauges, attitude indicator, HSI and charts.

use crate::flight::TrackPoint;
use crate::theme::*;
use eframe::egui::{
    self, pos2, vec2, Align, Align2, Color32, CornerRadius, FontId, Layout, Rect, Sense, Stroke,
    StrokeKind, Ui,
};

pub fn card(ui: &mut Ui, title: &str, add: impl FnOnce(&mut Ui)) {
    egui::Frame::new()
        .fill(CARD)
        .stroke(Stroke::new(1.0_f32, BORDER))
        .corner_radius(CornerRadius::same(12))
        .inner_margin(egui::Margin::same(14))
        .show(ui, |ui| {
            ui.set_min_width(ui.available_width());
            if !title.is_empty() {
                ui.label(
                    egui::RichText::new(title.to_uppercase())
                        .size(10.5)
                        .color(MUTED)
                        .strong(),
                );
                ui.add_space(6.0);
            }
            add(ui);
        });
}

pub fn metric(ui: &mut Ui, label: &str, value: &str, unit: &str, accent: Color32) {
    egui::Frame::new()
        .fill(CARD)
        .stroke(Stroke::new(1.0_f32, BORDER))
        .corner_radius(CornerRadius::same(12))
        .inner_margin(egui::Margin::symmetric(13, 10))
        .show(ui, |ui| {
            ui.set_min_width(ui.available_width());
            ui.label(
                egui::RichText::new(label.to_uppercase())
                    .size(10.0)
                    .color(MUTED)
                    .strong(),
            );
            ui.add_space(1.0);
            ui.horizontal(|ui| {
                ui.label(
                    egui::RichText::new(value)
                        .size(27.0)
                        .family(egui::FontFamily::Monospace)
                        .color(accent),
                );
                if !unit.is_empty() {
                    ui.label(egui::RichText::new(unit).size(10.5).color(DIM));
                }
            });
        });
}

pub fn arrow_right(ui: &mut Ui, color: Color32) {
    let (rect, _) = ui.allocate_exact_size(vec2(24.0, 14.0), Sense::hover());
    let painter = ui.painter();
    let y = rect.center().y;
    painter.line_segment(
        [pos2(rect.left(), y), pos2(rect.right() - 6.0, y)],
        Stroke::new(1.6_f32, color),
    );
    painter.add(egui::Shape::convex_polygon(
        vec![
            pos2(rect.right(), y),
            pos2(rect.right() - 8.0, y - 4.5),
            pos2(rect.right() - 8.0, y + 4.5),
        ],
        color,
        Stroke::NONE,
    ));
}

pub fn pill(ui: &mut Ui, text: &str, color: Color32) {
    let fill = Color32::from_rgba_unmultiplied(color.r(), color.g(), color.b(), 28);
    let line = Color32::from_rgba_unmultiplied(color.r(), color.g(), color.b(), 120);
    egui::Frame::new()
        .fill(fill)
        .stroke(Stroke::new(1.0_f32, line))
        .corner_radius(CornerRadius::same(20))
        .inner_margin(egui::Margin::symmetric(11, 4))
        .show(ui, |ui| {
            ui.label(egui::RichText::new(text).size(10.5).color(color).strong());
        });
}

/// A filled call-to-action pill; returns true on click.
pub fn action_button(ui: &mut Ui, label: &str, color: Color32, enabled: bool) -> bool {
    let galley = ui.painter().layout_no_wrap(
        label.to_uppercase(),
        FontId::proportional(10.0),
        color,
    );
    let size = vec2(galley.size().x + 34.0, 26.0);
    let (rect, response) = ui.allocate_exact_size(size, Sense::click());
    let hot = enabled && (response.hovered() || response.is_pointer_button_down_on());
    let fill = Color32::from_rgba_unmultiplied(
        color.r(),
        color.g(),
        color.b(),
        if hot { 62 } else { 36 },
    );
    let edge = Color32::from_rgba_unmultiplied(color.r(), color.g(), color.b(), if enabled { 190 } else { 90 });
    ui.painter().rect_filled(rect, CornerRadius::same(13), fill);
    ui.painter()
        .rect_stroke(rect, CornerRadius::same(13), Stroke::new(1.0_f32, edge), StrokeKind::Inside);
    ui.painter().circle_filled(
        pos2(rect.left() + 13.0, rect.center().y),
        3.0,
        if enabled { color } else { MUTED },
    );
    ui.painter().text(
        pos2(rect.left() + 22.0, rect.center().y),
        Align2::LEFT_CENTER,
        label.to_uppercase(),
        FontId::proportional(10.0),
        if enabled { color } else { MUTED },
    );
    if !enabled {
        ui.ctx().set_cursor_icon(egui::CursorIcon::Default);
    }
    enabled && response.clicked()
}

pub fn kv(ui: &mut Ui, key: &str, value: &str) {
    ui.horizontal(|ui| {
        ui.label(egui::RichText::new(key).size(11.5).color(MUTED));
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            ui.label(
                egui::RichText::new(value)
                    .size(12.0)
                    .family(egui::FontFamily::Monospace)
                    .color(TEXT),
            );
        });
    });
}

pub fn progress(ui: &mut Ui, fraction: f64, color: Color32) {
    let width = ui.available_width();
    let (rect, _) = ui.allocate_exact_size(vec2(width, 9.0), Sense::hover());
    let painter = ui.painter();
    painter.rect_filled(rect, CornerRadius::same(5), Color32::from_rgb(34, 22, 26));
    let f = fraction.clamp(0.0, 1.0) as f32;
    if f > 0.001 {
        let filled = Rect::from_min_size(rect.min, vec2(rect.width() * f, rect.height()));
        painter.rect_filled(filled, CornerRadius::same(5), color);
    }
}

/// Artificial horizon with pitch ladder and bank index.
pub fn attitude(ui: &mut Ui, size: f32, pitch: f64, bank: f64, hdg: f64, on: bool) {
    let (rect, _) = ui.allocate_exact_size(vec2(size, size), Sense::hover());
    let painter = ui.painter_at(rect);
    let center = rect.center();
    let radius = size * 0.5 - 2.0;

    let pitch = if on && pitch.is_finite() { pitch } else { 0.0 };
    let bank = if on && bank.is_finite() { bank } else { 0.0 };

    if !on {
        painter.rect_filled(rect, CornerRadius::same(10), Color32::from_rgb(12, 9, 11));
        painter.text(
            center,
            Align2::CENTER_CENTER,
            "NO DATA",
            FontId::proportional(12.0),
            DIM,
        );
        return;
    }

    let bank_rad = (bank as f32).to_radians();
    let dir = vec2(bank_rad.cos(), bank_rad.sin());
    let nrm = vec2(-dir.y, dir.x);
    let px_per_deg = radius / 22.0;
    let offset = (pitch as f32) * px_per_deg;
    let hc = center + vec2(0.0, offset);
    let l = size * 3.0;

    let sky = vec![
        hc - dir * l,
        hc + dir * l,
        hc + dir * l - nrm * l,
        hc - dir * l - nrm * l,
    ];
    painter.add(egui::Shape::convex_polygon(
        sky,
        Color32::from_rgb(26, 74, 120),
        Stroke::NONE,
    ));
    let ground = vec![
        hc - dir * l,
        hc + dir * l,
        hc + dir * l + nrm * l,
        hc - dir * l + nrm * l,
    ];
    painter.add(egui::Shape::convex_polygon(
        ground,
        Color32::from_rgb(92, 62, 38),
        Stroke::NONE,
    ));
    painter.line_segment([hc - dir * l, hc + dir * l], Stroke::new(1.8_f32, Color32::WHITE));

    for deg in (-30..=30).step_by(5) {
        if deg == 0 {
            continue;
        }
        let major = deg % 10 == 0;
        let base = hc - nrm * ((deg as f32) * px_per_deg);
        let half = if major { 26.0 } else { 12.0 };
        painter.line_segment(
            [base - dir * half, base + dir * half],
            Stroke::new(1.0_f32, Color32::from_white_alpha(190)),
        );
        if major {
            painter.text(
                base + dir * (half + 12.0),
                Align2::LEFT_CENTER,
                format!("{deg}"),
                FontId::monospace(9.0),
                Color32::from_white_alpha(210),
            );
        }
    }

    // Roll scale and index.
    for angle in [-60.0f32, -45.0, -30.0, -20.0, -10.0, 0.0, 10.0, 20.0, 30.0, 45.0, 60.0] {
        let a = (-90.0 + angle).to_radians();
        let outer = center + vec2(a.cos(), a.sin()) * (radius - 6.0);
        let inner = center + vec2(a.cos(), a.sin()) * (radius - if angle == 0.0 { 17.0 } else { 12.0 });
        painter.line_segment(
            [outer, inner],
            Stroke::new(
                if angle == 0.0 { 2.0_f32 } else { 1.0_f32 },
                Color32::from_white_alpha(210),
            ),
        );
    }
    let index_a = (-90.0 - bank as f32).to_radians();
    let tip = center + vec2(index_a.cos(), index_a.sin()) * (radius - 18.0);
    let side = vec2(-index_a.sin(), index_a.cos()) * 7.0;
    let back = center + vec2(index_a.cos(), index_a.sin()) * (radius - 6.0);
    painter.add(egui::Shape::convex_polygon(
        vec![tip, back + side, back - side],
        ACCENT,
        Stroke::NONE,
    ));

    // Fixed aircraft symbol.
    let wing = radius * 0.34;
    let wing_y = center.y;
    painter.line_segment(
        [pos2(center.x - wing, wing_y), pos2(center.x - 12.0, wing_y)],
        Stroke::new(2.4_f32, ACCENT),
    );
    painter.line_segment(
        [pos2(center.x + 12.0, wing_y), pos2(center.x + wing, wing_y)],
        Stroke::new(2.4_f32, ACCENT),
    );
    painter.circle_filled(center, 3.0, ACCENT);

    painter.rect_stroke(
        rect,
        CornerRadius::same(10),
        Stroke::new(1.0_f32, BORDER),
        StrokeKind::Inside,
    );
    let chip = Rect::from_center_size(
        pos2(center.x, rect.bottom() - 15.0),
        vec2(58.0, 20.0),
    );
    painter.rect_filled(chip, CornerRadius::same(5), Color32::from_black_alpha(150));
    painter.text(
        chip.center(),
        Align2::CENTER_CENTER,
        format!("{:03.0}°", if hdg.is_finite() { hdg } else { 0.0 }),
        FontId::monospace(11.0),
        Color32::WHITE,
    );
}

/// Horizontal situation indicator: compass rose, heading bug, waypoint bearing.
pub fn hsi(ui: &mut Ui, size: f32, hdg: f64, brg: Option<f64>) {
    let (rect, _) = ui.allocate_exact_size(vec2(size, size), Sense::hover());
    let painter = ui.painter_at(rect);
    let center = rect.center();
    let radius = size * 0.5 - 10.0;

    painter.circle_filled(center, radius, Color32::from_rgb(12, 9, 11));
    painter.circle_stroke(center, radius, Stroke::new(1.0_f32, BORDER));

    let heading = if hdg.is_finite() { hdg } else { 0.0 } as f32;

    for i in 0..36 {
        let angle = (i as f32 * 10.0 - 90.0).to_radians();
        let major = i % 3 == 0;
        let outer = center + vec2(angle.cos(), angle.sin()) * radius;
        let inner = center + vec2(angle.cos(), angle.sin()) * (radius - if major { 9.0 } else { 5.0 });
        painter.line_segment(
            [outer, inner],
            Stroke::new(1.0_f32, if major { MUTED } else { DIM }),
        );
    }
    for (label, deg) in [("N", 0.0f32), ("E", 90.0), ("S", 180.0), ("W", 270.0)] {
        let angle = (deg - 90.0).to_radians();
        let pos = center + vec2(angle.cos(), angle.sin()) * (radius - 20.0);
        painter.text(
            pos,
            Align2::CENTER_CENTER,
            label,
            FontId::proportional(10.0),
            if label == "N" { ACCENT } else { MUTED },
        );
    }

    // Waypoint bearing arrow.
    if let Some(bearing) = brg {
        if bearing.is_finite() {
            let angle = (bearing as f32 - 90.0).to_radians();
            let tip = center + vec2(angle.cos(), angle.sin()) * (radius - 6.0);
            let side = vec2(-angle.sin(), angle.cos()) * 6.0;
            let base = center + vec2(angle.cos(), angle.sin()) * (radius - 26.0);
            painter.add(egui::Shape::convex_polygon(
                vec![tip, base + side, base - side],
                CYAN,
                Stroke::NONE,
            ));
        }
    }

    // Heading line.
    let angle = (heading - 90.0).to_radians();
    let tip = center + vec2(angle.cos(), angle.sin()) * (radius - 12.0);
    painter.line_segment([center, tip], Stroke::new(2.2_f32, ACCENT));
    painter.circle_filled(center, 3.5, Color32::from_rgb(12, 9, 11));
    painter.circle_stroke(center, 3.5, Stroke::new(1.5_f32, TEXT));

    painter.text(
        pos2(center.x, rect.bottom() - 6.0),
        Align2::CENTER_BOTTOM,
        format!("{:03.0}°", heading),
        FontId::monospace(11.0),
        TEXT,
    );
}

/// Altitude-over-time profile of a recorded track.
pub fn profile(ui: &mut Ui, height: f32, track: &[TrackPoint], marker_t: Option<f64>, duration: f64) {
    let width = ui.available_width();
    let (rect, _) = ui.allocate_exact_size(vec2(width, height), Sense::hover());
    let painter = ui.painter_at(rect);
    painter.rect_filled(rect, CornerRadius::same(10), Color32::from_rgb(12, 9, 11));

    if track.len() < 2 {
        painter.text(
            rect.center(),
            Align2::CENTER_CENTER,
            "No track recorded",
            FontId::proportional(12.0),
            DIM,
        );
        return;
    }

    let max_alt = track
        .iter()
        .map(|p| p.alt)
        .fold(1000.0f64, f64::max)
        .max(1000.0);
    let t_max = duration.max(track.last().map(|p| p.t).unwrap_or(1.0)).max(1.0);

    let plot = Rect::from_min_max(
        pos2(rect.left() + 46.0, rect.top() + 10.0),
        pos2(rect.right() - 10.0, rect.bottom() - 22.0),
    );

    for i in 0..=4 {
        let frac = i as f32 / 4.0;
        let y = plot.bottom() - plot.height() * frac;
        painter.line_segment(
            [pos2(plot.left(), y), pos2(plot.right(), y)],
            Stroke::new(1.0_f32, Color32::from_rgb(36, 24, 28)),
        );
        painter.text(
            pos2(plot.left() - 6.0, y),
            Align2::RIGHT_CENTER,
            format!("{:.0}", max_alt * frac as f64),
            FontId::monospace(9.0),
            DIM,
        );
    }

    let stride = (track.len() / 400).max(1);
    let samples: Vec<&TrackPoint> = track.iter().step_by(stride).collect();

    let point_at = |p: &TrackPoint| {
        pos2(
            plot.left() + (p.t / t_max).clamp(0.0, 1.0) as f32 * plot.width(),
            plot.bottom() - (p.alt / max_alt).clamp(0.0, 1.0) as f32 * plot.height(),
        )
    };

    for pair in samples.windows(2) {
        let (a, b) = (point_at(pair[0]), point_at(pair[1]));
        painter.add(egui::Shape::convex_polygon(
            vec![
                a,
                b,
                pos2(b.x, plot.bottom()),
                pos2(a.x, plot.bottom()),
            ],
            Color32::from_rgba_unmultiplied(238, 66, 78, 46),
            Stroke::NONE,
        ));
    }
    let line: Vec<_> = samples.iter().map(|p| point_at(p)).collect();
    painter.add(egui::Shape::line(line, Stroke::new(1.6_f32, CYAN)));

    painter.text(
        pos2(plot.left(), rect.bottom() - 5.0),
        Align2::LEFT_BOTTOM,
        "00:00",
        FontId::monospace(9.0),
        DIM,
    );
    painter.text(
        pos2(plot.right(), rect.bottom() - 5.0),
        Align2::RIGHT_BOTTOM,
        crate::util::fmt_clock(duration),
        FontId::monospace(9.0),
        DIM,
    );

    if let Some(t) = marker_t {
        let frac = (t / t_max).clamp(0.0, 1.0);
        let x = plot.left() + frac as f32 * plot.width();
        painter.line_segment(
            [pos2(x, plot.top()), pos2(x, plot.bottom())],
            Stroke::new(1.0_f32, ACCENT),
        );
        let sample = track
            .iter()
            .min_by(|a, b| (a.t - t).abs().partial_cmp(&(b.t - t).abs()).unwrap_or(std::cmp::Ordering::Equal));
        if let Some(s) = sample {
            painter.circle_filled(point_at(s), 3.5, ACCENT);
            painter.text(
                pos2(x + 6.0, plot.top() + 4.0),
                Align2::LEFT_TOP,
                format!("{:.0} ft", s.alt),
                FontId::monospace(9.5),
                ACCENT,
            );
        }
    }
}

/// Vertical bars of landing rates, best on the left.
pub fn landing_bars(ui: &mut Ui, height: f32, values: &[f64]) {
    let width = ui.available_width();
    let (rect, _) = ui.allocate_exact_size(vec2(width, height), Sense::hover());
    let painter = ui.painter_at(rect);
    painter.rect_filled(rect, CornerRadius::same(10), Color32::from_rgb(12, 9, 11));

    if values.is_empty() {
        painter.text(
            rect.center(),
            Align2::CENTER_CENTER,
            "No landings recorded yet",
            FontId::proportional(12.0),
            DIM,
        );
        return;
    }

    let max = values.iter().cloned().fold(300.0f64, f64::max);
    let inner = rect.shrink2(vec2(14.0, 14.0));
    let slot = inner.width() / values.len() as f32;
    let bar_w = (slot * 0.6).clamp(2.0, 26.0);

    for (i, fpm) in values.iter().enumerate() {
        let frac = (fpm.abs() / max).clamp(0.02, 1.0) as f32;
        let x = inner.left() + slot * i as f32 + (slot - bar_w) * 0.5;
        let bar = Rect::from_min_max(
            pos2(x, inner.bottom() - inner.height() * frac),
            pos2(x + bar_w, inner.bottom()),
        );
        let color = if fpm.abs() <= 100.0 {
            GREEN
        } else if fpm.abs() <= 250.0 {
            ACCENT
        } else if fpm.abs() <= 450.0 {
            ORANGE
        } else {
            RED
        };
        painter.rect_filled(bar, CornerRadius::same(3), color);
    }
}
