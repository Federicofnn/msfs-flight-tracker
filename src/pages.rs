//! The five screens of the tracker.

use crate::flight::{Landing, Phase};
use crate::plan::FlightPlan;
use crate::sim::{LinkState, Telemetry};
use crate::theme::*;
use crate::util::{
    fmt_clock, fmt_duration, fmt_local_hhmm, fmt_utc, fmt_utc_full, haversine_nm, num, now_epoch,
};
use crate::widgets::{self, card, kv, landing_bars, metric, pill, profile, progress};
use crate::traffic;
use crate::update;
use crate::worldmap;
use crate::{App, Page};
use eframe::egui::{
    self, Align, Color32, CornerRadius, Layout, Pos2, RichText, Sense, Stroke, StrokeKind, Ui,
};

/// A floating panel drawn on top of the map, anchored by its top-left corner.
fn overlay_m(ui: &mut Ui, max: egui::Rect, margin: i8, add: impl FnOnce(&mut Ui)) {
    let width = max.width();
    let mut child = ui.new_child(egui::UiBuilder::new().max_rect(max));
    child.set_width(width);
    egui::Frame::new()
        .fill(Color32::from_rgba_unmultiplied(20, 15, 17, 243))
        .stroke(Stroke::new(1.0_f32, BORDER))
        .corner_radius(CornerRadius::same(16))
        .inner_margin(egui::Margin::same(margin))
        .show(&mut child, |ui| {
            ui.set_width(width - margin as f32 * 2.0);
            add(ui);
        });
}

fn overlay(ui: &mut Ui, max: egui::Rect, add: impl FnOnce(&mut Ui)) {
    overlay_m(ui, max, 14, add);
}

fn caps(ui: &mut Ui, text: &str, color: Color32, size: f32) {
    ui.label(
        RichText::new(text.to_uppercase())
            .size(size)
            .color(color)
            .strong(),
    );
}

/// A compact readout inside the floating flight card.
fn chip_stat(ui: &mut Ui, label: &str, value: &str, unit: &str, color: Color32) {
    egui::Frame::new()
        .fill(Color32::from_rgb(28, 22, 25))
        .corner_radius(CornerRadius::same(8))
        .inner_margin(egui::Margin::symmetric(8, 5))
        .show(ui, |ui| {
            ui.set_min_width(ui.available_width());
            ui.label(RichText::new(label).size(7.5).color(DIM).strong());
            ui.horizontal(|ui| {
                ui.label(
                    RichText::new(value)
                        .size(12.0)
                        .family(egui::FontFamily::Monospace)
                        .color(color),
                );
                ui.label(RichText::new(unit).size(7.0).color(DIM));
            });
        });
}

fn map_chip(ui: &mut Ui, value: &str, label: &str) {
    egui::Frame::new()
        .fill(Color32::from_rgb(30, 23, 26))
        .corner_radius(CornerRadius::same(14))
        .inner_margin(egui::Margin::symmetric(10, 5))
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.label(RichText::new(value).size(10.0).color(TEXT).strong());
                ui.label(RichText::new(label).size(8.0).color(MUTED));
            });
        });
}

/// A pill segmented control, drawn by hand to keep the dark reference look.
fn segmented(ui: &mut Ui, index: &mut usize, labels: &[&str]) -> bool {
    let width = ui.available_width();
    let height = 30.0;
    let (rect, _) = ui.allocate_exact_size(egui::vec2(width, height), Sense::hover());
    ui.painter()
        .rect_filled(rect, CornerRadius::same(15), Color32::from_rgb(13, 10, 12));
    let seg_w = width / labels.len().max(1) as f32;
    let mut changed = false;
    for (i, label) in labels.iter().enumerate() {
        let seg = egui::Rect::from_min_size(
            egui::pos2(rect.left() + seg_w * i as f32 + 2.0, rect.top() + 2.0),
            egui::vec2(seg_w - 4.0, height - 4.0),
        );
        let response = ui.interact(seg, ui.id().with(("seg", i, label)), Sense::click());
        if *index == i {
            ui.painter()
                .rect_filled(seg, CornerRadius::same(13), Color32::from_rgb(46, 32, 36));
        }
        ui.painter().text(
            seg.center(),
            egui::Align2::CENTER_CENTER,
            label.to_uppercase(),
            egui::FontId::proportional(8.5),
            if *index == i { TEXT } else { MUTED },
        );
        if response.clicked() {
            *index = i;
            changed = true;
        }
    }
    changed
}

fn pill_button(ui: &mut Ui, active: bool, label: &str, width: f32) -> bool {
    let (rect, response) = ui.allocate_exact_size(egui::vec2(width, 32.0), Sense::click());
    let fill = if active {
        Color32::from_rgb(46, 32, 36)
    } else {
        Color32::from_rgb(24, 18, 21)
    };
    ui.painter().rect_filled(rect, CornerRadius::same(16), fill);
    ui.painter().rect_stroke(
        rect,
        CornerRadius::same(16),
        Stroke::new(
            1.0_f32,
            if active {
                Color32::from_rgb(96, 60, 66)
            } else {
                BORDER
            },
        ),
        StrokeKind::Inside,
    );
    if active {
        let dot = egui::pos2(rect.left() + 14.0, rect.center().y);
        ui.painter().circle_filled(dot, 2.4, ACCENT);
    }
    ui.painter().text(
        rect.center(),
        egui::Align2::CENTER_CENTER,
        label.to_uppercase(),
        egui::FontId::proportional(8.5),
        if active { TEXT } else { MUTED },
    );
    response.clicked()
}

fn toggle_row(ui: &mut Ui, value: &mut bool, label: &str) {
    let width = ui.available_width();
    let (rect, response) = ui.allocate_exact_size(egui::vec2(width, 27.0), Sense::click());
    ui.painter().text(
        rect.left_center() + egui::vec2(2.0, 0.0),
        egui::Align2::LEFT_CENTER,
        label,
        egui::FontId::proportional(10.5),
        MUTED,
    );
    let track = egui::Rect::from_center_size(
        egui::pos2(rect.right() - 18.0, rect.center().y),
        egui::vec2(30.0, 15.0),
    );
    ui.painter().rect_filled(
        track,
        CornerRadius::same(8),
        if *value {
            Color32::from_rgb(86, 32, 38)
        } else {
            Color32::from_rgb(35, 28, 31)
        },
    );
    ui.painter().circle_filled(
        egui::pos2(
            if *value {
                track.right() - 8.0
            } else {
                track.left() + 8.0
            },
            track.center().y,
        ),
        5.5,
        if *value { ACCENT } else { MUTED },
    );
    if response.clicked() {
        *value = !*value;
    }
}

fn rail_icon(painter: &egui::Painter, center: Pos2, kind: u8, color: Color32) {
    let stroke = Stroke::new(1.3_f32, color);
    match kind {
        0 => {
            painter.circle_stroke(center, 7.5, stroke);
            painter.line_segment(
                [center + egui::vec2(-7.5, 0.0), center + egui::vec2(7.5, 0.0)],
                Stroke::new(1.0_f32, color),
            );
            let mut meridian = Vec::new();
            for i in 0..=16 {
                let angle = i as f32 / 16.0 * std::f32::consts::TAU;
                meridian.push(center + egui::vec2(3.6 * angle.sin(), 7.5 * angle.cos()));
            }
            painter.add(egui::Shape::line(
                meridian,
                Stroke::new(1.0_f32, color),
            ));
        }
        1 => {
            painter.circle_stroke(center + egui::vec2(-6.0, 5.5), 2.4, stroke);
            painter.add(egui::Shape::convex_polygon(
                vec![
                    center + egui::vec2(7.0, -7.0),
                    center + egui::vec2(1.4, -5.0),
                    center + egui::vec2(5.0, -1.4),
                ],
                color,
                Stroke::NONE,
            ));
            painter.line_segment(
                [center + egui::vec2(-4.2, 4.0), center + egui::vec2(2.4, -1.8)],
                Stroke::new(1.0_f32, color),
            );
        }
        2 => {
            painter.circle_stroke(center, 7.5, stroke);
            painter.line_segment(
                [center, center + egui::vec2(4.4, -4.4)],
                Stroke::new(1.6_f32, color),
            );
            painter.circle_filled(center, 1.6, color);
        }
        3 => {
            for i in -1..=1 {
                let y = center.y + i as f32 * 4.5;
                painter.circle_filled(egui::pos2(center.x - 6.0, y), 1.2, color);
                painter.line_segment(
                    [egui::pos2(center.x - 3.0, y), egui::pos2(center.x + 7.0, y)],
                    Stroke::new(1.1_f32, color),
                );
            }
        }
        4 => {
            for (i, height) in [5.0_f32, 10.0, 14.0].iter().enumerate() {
                let x = center.x - 6.0 + i as f32 * 5.5;
                painter.line_segment(
                    [
                        egui::pos2(x, center.y + 7.0),
                        egui::pos2(x, center.y + 7.0 - height),
                    ],
                    Stroke::new(2.2_f32, color),
                );
            }
        }
        _ => {
            painter.circle_stroke(center, 4.2, stroke);
            for i in 0..6 {
                let angle = i as f32 / 6.0 * std::f32::consts::TAU;
                let dir = egui::vec2(angle.cos(), angle.sin());
                painter.line_segment(
                    [center + dir * 4.2, center + dir * 7.2],
                    Stroke::new(1.1_f32, color),
                );
            }
        }
    }
}

fn icon_search(painter: &egui::Painter, center: Pos2, color: Color32) {
    painter.circle_stroke(center + egui::vec2(-1.0, -1.0), 5.0, Stroke::new(1.4_f32, color));
    painter.line_segment(
        [center + egui::vec2(2.6, 2.6), center + egui::vec2(6.5, 6.5)],
        Stroke::new(1.6_f32, color),
    );
}

fn icon_flame(painter: &egui::Painter, center: Pos2, color: Color32) {
    painter.add(egui::Shape::convex_polygon(
        vec![
            center + egui::vec2(0.0, -6.0),
            center + egui::vec2(4.6, 1.0),
            center + egui::vec2(2.6, 6.0),
            center + egui::vec2(-2.6, 6.0),
            center + egui::vec2(-4.6, 1.0),
        ],
        color,
        Stroke::NONE,
    ));
}

fn truncate(text: &str, max: usize) -> String {
    if text.chars().count() <= max {
        return text.to_string();
    }
    let mut out: String = text.chars().take(max.saturating_sub(1)).collect();
    out.push('…');
    out
}

enum Action {
    Gpx(String),
    Kml(String),
    Delete(String),
}

fn sys(ui: &mut Ui, key: &str, value: &str, color: Color32) {
    ui.label(RichText::new(key).size(10.5).color(MUTED));
    ui.label(
        RichText::new(value)
            .size(12.5)
            .family(egui::FontFamily::Monospace)
            .color(color),
    );
}

fn stat_tile(ui: &mut Ui, label: &str, value: &str, unit: &str, color: Color32) {
    egui::Frame::new()
        .fill(CARD)
        .stroke(Stroke::new(1.0_f32, BORDER))
        .corner_radius(CornerRadius::same(10))
        .inner_margin(egui::Margin::symmetric(12, 9))
        .show(ui, |ui| {
            ui.set_min_width(ui.available_width());
            ui.label(RichText::new(label.to_uppercase()).size(9.5).color(MUTED).strong());
            ui.horizontal(|ui| {
                let size = if value.chars().count() > 11 { 13.0 } else { 20.0 };
                ui.label(
                    RichText::new(value)
                        .size(size)
                        .family(egui::FontFamily::Monospace)
                        .color(color),
                );
                if !unit.is_empty() {
                    ui.label(RichText::new(unit).size(10.0).color(DIM));
                }
            });
        });
}

fn landing_banner(ui: &mut Ui, landing: &Landing) -> bool {
    let color = rating_color(&landing.rating);
    let mut dismiss = false;
    egui::Frame::new()
        .fill(Color32::from_rgb(32, 20, 23))
        .stroke(Stroke::new(1.2_f32, color))
        .corner_radius(CornerRadius::same(12))
        .inner_margin(egui::Margin::symmetric(16, 12))
        .show(ui, |ui| {
            ui.set_min_width(ui.available_width());
            ui.horizontal(|ui| {
                ui.vertical(|ui| {
                    ui.label(
                        RichText::new("TOUCHDOWN")
                            .size(10.0)
                            .color(MUTED)
                            .strong(),
                    );
                    ui.label(RichText::new(&landing.rating).size(25.0).strong().color(color));
                });
                ui.add_space(18.0);
                ui.vertical(|ui| {
                    ui.label(
                        RichText::new(format!("{:.0} fpm", landing.fpm.abs()))
                            .size(22.0)
                            .family(egui::FontFamily::Monospace)
                            .color(TEXT),
                    );
                    ui.label(
                        RichText::new(format!(
                            "{:.2} G · {:.0} kt · {:+.1}° pitch · {:.1}° bank",
                            landing.g, landing.ias, landing.pitch, landing.bank
                        ))
                        .size(11.5)
                        .color(MUTED),
                    );
                });
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    if ui.button("Dismiss").clicked() {
                        dismiss = true;
                    }
                    ui.label(
                        RichText::new(format!(
                            "wind {:03.0}° / {:.0} kt",
                            landing.wind_dir, landing.wind_kt
                        ))
                        .size(11.0)
                        .color(MUTED),
                    );
                });
            });
        });
    dismiss
}

/// Where the flight is along its route, and when it gets there: every screen
/// reads its times from here so they can never disagree.
#[derive(Clone, Copy)]
pub struct Trip {
    pub total_nm: f64,
    pub remaining_nm: f64,
    pub fraction: f64,
    pub speed_kt: f64,
    pub ete_s: f64,
    pub eta_epoch: u64,
    pub known: bool,
}

impl Trip {
    /// Arrival time on the pilot's own clock.
    pub fn arrival(&self) -> String {
        if self.known {
            fmt_local_hhmm(self.eta_epoch)
        } else {
            "--:--".to_string()
        }
    }

    /// "2h 57m" left, or a dash while there is no route.
    pub fn time_to_go(&self) -> String {
        if self.known {
            fmt_duration(self.ete_s)
        } else {
            "—".to_string()
        }
    }
}

impl App {
    /// Route progress and arrival, from live speed when the aircraft is moving
    /// and from the plan average otherwise, so the ETA never collapses to "—".
    pub fn trip(&self, t: &Telemetry, plan: &FlightPlan, fresh: bool) -> Trip {
        let total = plan.total_nm();
        let flying = fresh && t.position_valid();
        let remaining = if flying {
            plan.remaining_nm(t.lat, t.lon).unwrap_or(total)
        } else {
            total
        };
        let remaining = if remaining.is_finite() {
            remaining.clamp(0.0, total.max(0.0))
        } else {
            total
        };
        let planned = self
            .planned_ete()
            .filter(|seconds| seconds.is_finite() && *seconds > 0.0);
        let speed = if flying && self.gs_smooth > 60.0 {
            self.gs_smooth
        } else if fresh && t.tas_kt.is_finite() && t.tas_kt > 80.0 {
            t.tas_kt
        } else if let Some(seconds) = planned.filter(|_| total > 1.0) {
            (total / seconds * 3600.0).clamp(120.0, 620.0)
        } else {
            430.0
        };
        let ete_s = if total > 1.0 && speed > 40.0 {
            remaining / speed * 3600.0
        } else {
            f64::NAN
        };
        let known = ete_s.is_finite() && ete_s >= 0.0 && total > 1.0;
        Trip {
            total_nm: total,
            remaining_nm: remaining,
            fraction: if total > 0.5 {
                (1.0 - remaining / total).clamp(0.0, 1.0)
            } else {
                0.0
            },
            speed_kt: speed,
            ete_s,
            eta_epoch: if known {
                now_epoch() + ete_s.max(0.0).round() as u64
            } else {
                0
            },
            known,
        }
    }

    pub fn dashboard(&mut self, ui: &mut Ui) {
        let t = self.tele.clone();
        let fresh = self.fresh();

        if let Some((landing, at)) = self.landing_popup.clone() {
            if !self.popup_dismissed && at.elapsed().as_secs() < 150 {
                if landing_banner(ui, &landing) {
                    self.popup_dismissed = true;
                }
                ui.add_space(12.0);
            }
        }

        ui.horizontal(|ui| {
            ui.label(RichText::new("Flight deck").size(21.0).strong().color(TEXT));
            ui.add_space(10.0);
            let name = if t.title.is_empty() {
                "No aircraft".to_string()
            } else {
                t.title.clone()
            };
            ui.label(RichText::new(name).size(12.5).color(MUTED));
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                let color = match self.recorder.phase {
                    Phase::Airborne => CYAN,
                    Phase::Rollout => GREEN,
                    Phase::Ground => MUTED,
                    Phase::Idle => DIM,
                };
                pill(ui, self.recorder.phase.label(), color);
            });
        });

        ui.add_space(12.0);

        // The route card is the hero of the screen, exactly as on the map.
        if let Some(plan) = self.plan.clone() {
            self.route_card(ui, &plan, &t, fresh);
            ui.add_space(12.0);
        }

        let dash = "—".to_string();
        let ias = if fresh { num(t.ias_kt, 0) } else { dash.clone() };
        let gs = if fresh { num(t.gs_kt, 0) } else { dash.clone() };
        let alt = if fresh {
            num(self.alt_val(t.alt_ft), 0)
        } else {
            dash.clone()
        };
        let vs = if fresh {
            num(self.vs_val(t.vs_fpm), 0)
        } else {
            dash.clone()
        };
        let hdg = if fresh { format!("{:03.0}", t.hdg_mag) } else { "---".into() };
        let agl = if fresh {
            num(self.alt_val(t.agl_ft), 0)
        } else {
            dash.clone()
        };
        let vs_color = if !fresh {
            MUTED
        } else if t.vs_fpm > 100.0 {
            GREEN
        } else if t.vs_fpm < -100.0 {
            ORANGE
        } else {
            TEXT
        };
        let alt_unit = self.alt_unit();
        let vs_unit = self.vs_unit();

        ui.columns(6, |cols| {
            metric(&mut cols[0], "IAS", &ias, "kt", CYAN);
            metric(&mut cols[1], "GS", &gs, "kt", TEXT);
            metric(&mut cols[2], "ALT", &alt, alt_unit, ACCENT);
            metric(&mut cols[3], "V/S", &vs, vs_unit, vs_color);
            metric(&mut cols[4], "HDG", &hdg, "°", BLUE);
            metric(&mut cols[5], "AGL", &agl, alt_unit, MUTED);
        });

        ui.add_space(12.0);

        let bearing = self
            .plan
            .as_ref()
            .and_then(|plan| plan.bearing_to_dest(t.lat, t.lon));

        ui.horizontal_top(|ui| {
            ui.allocate_ui_with_layout(
                egui::vec2(320.0, 306.0),
                Layout::top_down(Align::Min),
                |ui| {
                    card(ui, "Attitude", |ui| {
                        let avail = ui.available_width();
                        let size = 244.0_f32.min(avail);
                        ui.horizontal(|ui| {
                            ui.add_space(((avail - size) * 0.5).max(0.0));
                            widgets::attitude(ui, size, t.pitch_deg, t.bank_deg, t.hdg_mag, fresh);
                        });
                    });
                },
            );
            ui.allocate_ui_with_layout(
                egui::vec2(246.0, 306.0),
                Layout::top_down(Align::Min),
                |ui| {
                    card(ui, "Heading / waypoint", |ui| {
                        let avail = ui.available_width();
                        let size = 196.0_f32.min(avail);
                        ui.horizontal(|ui| {
                            ui.add_space(((avail - size) * 0.5).max(0.0));
                            widgets::hsi(ui, size, t.hdg_mag, bearing);
                        });
                    });
                },
            );
            ui.vertical(|ui| {
                card(ui, "Systems", |ui| {
                    let gear = if !fresh {
                        "—"
                    } else if t.gear_pct >= 99.0 {
                        "DOWN"
                    } else if t.gear_pct <= 1.0 {
                        "UP"
                    } else {
                        "TRANSIT"
                    };
                    let fuel = format!(
                        "{} {}",
                        num(self.fuel_val(t.fuel_kg), 0),
                        self.fuel_unit()
                    );
                    let weight = format!(
                        "{} {}",
                        num(self.fuel_val(t.weight_kg), 0),
                        self.fuel_unit()
                    );
                    let wind = if fresh && t.wind_kt.is_finite() {
                        format!("{:03.0}° / {:.0} kt", t.wind_dir, t.wind_kt)
                    } else {
                        "—".to_string()
                    };
                    let oat = if fresh {
                        format!("{:.0} °C", t.oat_c)
                    } else {
                        "—".to_string()
                    };
                    egui::Grid::new("systems")
                        .num_columns(4)
                        .spacing([14.0, 8.0])
                        .show(ui, |ui| {
                            sys(ui, "GEAR", gear, if gear == "DOWN" { GREEN } else { TEXT });
                            sys(
                                ui,
                                "FLAPS",
                                &if fresh { format!("{:.0}%", t.flaps_pct) } else { "—".into() },
                                TEXT,
                            );
                            ui.end_row();
                            sys(
                                ui,
                                "PARK BRAKE",
                                if t.park_brake { "SET" } else { "OFF" },
                                if t.park_brake { ACCENT } else { TEXT },
                            );
                            sys(
                                ui,
                                "AUTOPILOT",
                                if t.autopilot { "ON" } else { "OFF" },
                                if t.autopilot { CYAN } else { TEXT },
                            );
                            ui.end_row();
                            sys(
                                ui,
                                "ENGINE",
                                if t.engine_on { "RUNNING" } else { "OFF" },
                                if t.engine_on { GREEN } else { MUTED },
                            );
                            sys(
                                ui,
                                "STALL",
                                if t.stall_warning { "WARNING" } else { "CLEAR" },
                                if t.stall_warning { RED } else { TEXT },
                            );
                            ui.end_row();
                        });
                    ui.add_space(8.0);
                    ui.separator();
                    ui.add_space(6.0);
                    kv(ui, "Fuel on board", &fuel);
                    kv(ui, "Gross weight", &weight);
                    kv(ui, "Wind", &wind);
                    kv(ui, "Outside air", &oat);
                    if self.burn_kg_h > 1.0 {
                        kv(
                            ui,
                            "Fuel flow",
                            &format!("{} {}/h", num(self.fuel_val(self.burn_kg_h), 0), self.fuel_unit()),
                        );
                    }
                });
            });
        });

    }

    fn route_card(&mut self, ui: &mut Ui, plan: &FlightPlan, t: &Telemetry, fresh: bool) {
        let trip = self.trip(t, plan, fresh);
        let remaining = trip.remaining_nm;
        let fraction = trip.fraction;
        let gs = trip.speed_kt;
        let ete = trip.ete_s;
        let eta = trip.arrival();
        let tod = plan.tod_nm(t.alt_ft);
        let fuel_at_dest = t.fuel_kg - self.burn_kg_h * (ete / 3600.0).max(0.0);

        card(ui, "Route progress", |ui| {
            ui.horizontal(|ui| {
                ui.label(
                    RichText::new(&plan.origin)
                        .size(20.0)
                        .strong()
                        .family(egui::FontFamily::Monospace)
                        .color(TEXT),
                );
                widgets::arrow_right(ui, DIM);
                ui.label(
                    RichText::new(&plan.dest)
                        .size(20.0)
                        .strong()
                        .family(egui::FontFamily::Monospace)
                        .color(TEXT),
                );
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    ui.label(
                        RichText::new(format!("{:.0}% complete", fraction * 100.0))
                            .size(11.0)
                            .color(ACCENT),
                    );
                });
            });
            ui.add_space(8.0);
            progress(ui, fraction, CYAN);
            ui.add_space(10.0);
            ui.columns(4, |cols| {
                stat_tile(&mut cols[0], "Arrivo (ora locale)", &eta, "", ACCENT);
                stat_tile(
                    &mut cols[1],
                    "Tra quanto arrivo",
                    &trip.time_to_go(),
                    "",
                    CYAN,
                );
                stat_tile(
                    &mut cols[2],
                    "A destinazione",
                    &num(remaining, 0),
                    "NM",
                    TEXT,
                );
                stat_tile(
                    &mut cols[3],
                    "Top of descent",
                    &format!("{:.0} NM", tod),
                    "",
                    MUTED,
                );
            });
            ui.add_space(4.0);
            ui.horizontal(|ui| {
                ui.label(
                    RichText::new(if gs > 40.0 {
                        format!(
                            "3° profile: {:+.0} {}",
                            self.vs_val(plan.path_vs_fpm(gs)),
                            self.vs_unit()
                        )
                    } else {
                        "3° profile: waiting for ground speed".to_string()
                    })
                    .size(11.0)
                    .color(MUTED),
                );
                ui.add_space(4.0);
                ui.label(RichText::new("METAR").size(8.5).color(DIM).strong());
                if ui
                    .small_button(format!("{} partenza ↗", plan.origin))
                    .on_hover_text("METAR, TAF e meteo su metar-taf.com")
                    .clicked()
                {
                    App::open_metar(&plan.origin);
                }
                if ui
                    .small_button(format!("{} arrivo ↗", plan.dest))
                    .on_hover_text("METAR, TAF e meteo su metar-taf.com")
                    .clicked()
                {
                    App::open_metar(&plan.dest);
                }
                if t.fuel_kg.is_finite() && fuel_at_dest.is_finite() {
                    ui.label(
                        RichText::new(format!(
                            "· fuel at destination: {} {}",
                            num(self.fuel_val(fuel_at_dest.max(0.0)), 0),
                            self.fuel_unit()
                        ))
                        .size(11.0)
                        .color(if fuel_at_dest < 0.0 { RED } else { MUTED }),
                    );
                }
            });
        });
    }

    pub fn plan_page(&mut self, ui: &mut Ui) {
        let mut import = false;
        let mut unload = false;
        let mut simbrief = false;
        ui.horizontal(|ui| {
            ui.label(RichText::new("Flight plan").size(21.0).strong().color(TEXT));
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                if ui.button("Import .pln").clicked() {
                    import = true;
                }
                if ui
                    .add_enabled(
                        !self.simbrief_busy,
                        egui::Button::new(if self.simbrief_busy {
                            "SimBrief…"
                        } else {
                            "Load SimBrief"
                        }),
                    )
                    .clicked()
                {
                    simbrief = true;
                }
                if self.plan.is_some() && ui.button("Unload").clicked() {
                    unload = true;
                }
            });
        });
        if import {
            self.pick_plan();
        }
        if simbrief {
            self.load_simbrief();
        }
        if unload {
            self.plan = None;
            self.store.settings.last_plan = None;
            self.store.save_settings();
            self.toast_msg("Flight plan unloaded — free flight");
        }

        ui.add_space(12.0);

        let Some(plan) = self.plan.clone() else {
            card(ui, "No active route", |ui| {
                ui.label(
                    RichText::new(
                        "Load the .pln you fly in Microsoft Flight Simulator: destination distance, \
                         time enroute, ETA, top of descent and fuel at destination all come alive.",
                    )
                    .size(12.5)
                    .color(MUTED),
                );
                ui.add_space(10.0);
                ui.horizontal(|ui| {
                    if ui.button("Import .pln").clicked() {
                        self.pick_plan();
                    }
                    if ui
                        .add_enabled(
                            !self.simbrief_busy,
                            egui::Button::new(if self.simbrief_busy {
                                "SimBrief…"
                            } else {
                                "Load SimBrief"
                            }),
                        )
                        .clicked()
                    {
                        self.load_simbrief();
                    }
                    ui.label(
                        RichText::new("…or drop the .pln anywhere on this window.")
                            .size(11.5)
                            .color(DIM),
                    );
                });
                ui.add_space(8.0);
                ui.horizontal(|ui| {
                    ui.label(
                        RichText::new("SimBrief pilot ID or username")
                            .size(11.5)
                            .color(MUTED),
                    );
                    if ui
                        .add(
                            egui::TextEdit::singleline(&mut self.store.settings.simbrief_id)
                                .desired_width(140.0)
                                .hint_text("e.g. 123456"),
                        )
                        .changed()
                    {
                        self.store.save_settings();
                    }
                });
                if let Some(err) = &self.plan_error {
                    ui.add_space(8.0);
                    ui.label(RichText::new(err).size(11.5).color(RED));
                }
            });
            return;
        };

        let t = self.tele.clone();
        let fresh = self.fresh();

        ui.horizontal(|ui| {
            ui.vertical(|ui| {
                ui.label(RichText::new("FROM").size(9.5).color(MUTED).strong());
                ui.label(
                    RichText::new(&plan.origin)
                        .size(26.0)
                        .strong()
                        .family(egui::FontFamily::Monospace)
                        .color(TEXT),
                );
            });
            ui.add_space(18.0);
            widgets::arrow_right(ui, DIM);
            ui.add_space(18.0);
            ui.vertical(|ui| {
                ui.label(RichText::new("TO").size(9.5).color(MUTED).strong());
                ui.label(
                    RichText::new(&plan.dest)
                        .size(26.0)
                        .strong()
                        .family(egui::FontFamily::Monospace)
                        .color(TEXT),
                );
            });
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                if plan.cruise_ft.is_finite() {
                    ui.label(
                        RichText::new(format!(
                            "cruise {} {}",
                            num(self.alt_val(plan.cruise_ft), 0),
                            self.alt_unit()
                        ))
                        .size(11.5)
                        .color(MUTED),
                    );
                }
                ui.label(
                    RichText::new(format!("{} NM planned", num(plan.total_nm(), 0)))
                        .size(11.5)
                        .color(MUTED),
                );
            });
        });

        ui.add_space(12.0);
        self.route_card(ui, &plan, &t, fresh);

        ui.add_space(12.0);
        ui.horizontal_top(|ui| {
            ui.allocate_ui_with_layout(
                egui::vec2((ui.available_width() * 0.56).max(380.0), 320.0),
                Layout::top_down(Align::Min),
                |ui| {
                    card(ui, "Route", |ui| {
                        egui::ScrollArea::vertical()
                            .id_salt("route_list")
                            .max_height(230.0)
                            .auto_shrink([false, false])
                            .show(ui, |ui| {
                                egui::Grid::new("route_grid")
                                    .num_columns(4)
                                    .spacing([14.0, 6.0])
                                    .striped(true)
                                    .show(ui, |ui| {
                                        ui.label(RichText::new("WP").size(10.0).color(MUTED).strong());
                                        ui.label(RichText::new("TYPE").size(10.0).color(MUTED).strong());
                                        ui.label(RichText::new("LEG").size(10.0).color(MUTED).strong());
                                        ui.label(RichText::new("CUM").size(10.0).color(MUTED).strong());
                                        ui.end_row();
                                        let mut cum = 0.0;
                                        let mut prev = plan.origin_pos;
                                        for wp in &plan.route {
                                            if let Some(prev_pos) = prev {
                                                cum +=
                                                    haversine_nm(prev_pos.0, prev_pos.1, wp.lat, wp.lon);
                                            }
                                            let leg = prev
                                                .map(|p| haversine_nm(p.0, p.1, wp.lat, wp.lon))
                                                .unwrap_or(0.0);
                                            prev = Some((wp.lat, wp.lon));
                                            ui.label(
                                                RichText::new(&wp.id)
                                                    .size(12.0)
                                                    .family(egui::FontFamily::Monospace)
                                                    .color(TEXT),
                                            );
                                            ui.label(RichText::new(&wp.kind).size(10.5).color(MUTED));
                                            ui.label(
                                                RichText::new(format!("{leg:.0}"))
                                                    .size(11.5)
                                                    .family(egui::FontFamily::Monospace)
                                                    .color(MUTED),
                                            );
                                            ui.label(
                                                RichText::new(format!("{cum:.0}"))
                                                    .size(11.5)
                                                    .family(egui::FontFamily::Monospace)
                                                    .color(CYAN),
                                            );
                                            ui.end_row();
                                        }
                                    });
                            });
                    });
                },
            );
            ui.vertical(|ui| {
                card(ui, "Live navigation", |ui| {
                    let trip = self.trip(&t, &plan, fresh);
                    let remaining = trip.remaining_nm;
                    let gs = trip.speed_kt;
                    let ete = trip.ete_s;
                    let next = if fresh && t.position_valid() {
                        plan.next_waypoint(t.lat, t.lon).map(|w| w.id.clone())
                    } else {
                        None
                    };
                    kv(ui, "Aircraft position", &if fresh && t.position_valid() {
                        format!("{:.3}, {:.3}", t.lat, t.lon)
                    } else {
                        "—".to_string()
                    });
                    kv(ui, "Next waypoint", next.as_deref().unwrap_or("—"));
                    kv(
                        ui,
                        "Leg distance (GPS)",
                        &format!("{} NM", num(t.wp_dist_nm, 1)),
                    );
                    kv(
                        ui,
                        "Leg ETE (GPS)",
                        &if t.wp_ete_s.is_finite() && t.wp_ete_s > 0.0 {
                            fmt_clock(t.wp_ete_s)
                        } else {
                            "—".to_string()
                        },
                    );
                    kv(ui, "Remaining", &format!("{} NM", num(remaining, 0)));
                    kv(
                        ui,
                        "Ground speed",
                        &format!("{} kt", num(gs, 0)),
                    );
                    kv(ui, "Tra quanto arrivo", &trip.time_to_go());
                    kv(ui, "Arrivo (locale)", &trip.arrival());
                    kv(
                        ui,
                        "Fuel at destination",
                        &if t.fuel_kg.is_finite() && ete.is_finite() {
                            format!(
                                "{} {}",
                                num(self.fuel_val((t.fuel_kg - self.burn_kg_h * ete / 3600.0).max(0.0)), 0),
                                self.fuel_unit()
                            )
                        } else {
                            "—".to_string()
                        },
                    );
                    ui.add_space(6.0);
                    ui.label(
                        RichText::new(format!(
                            "Plan loaded from {}",
                            if plan.source.is_empty() {
                                "manual entry".to_string()
                            } else {
                                plan.source.clone()
                            }
                        ))
                        .size(10.0)
                        .color(DIM),
                    );
                });
            });
        });

        ui.add_space(12.0);
        card(ui, "METAR & TAF", |ui| {
            ui.label(
                RichText::new(
                    "Meteo reale degli aeroporti della rotta, su metar-taf.com (edizione italiana).",
                )
                .size(11.5)
                .color(MUTED),
            );
            ui.add_space(8.0);
            ui.horizontal(|ui| {
                if ui
                    .button(format!("{} partenza ↗", plan.origin))
                    .on_hover_text("METAR, TAF e meteo di partenza")
                    .clicked()
                {
                    App::open_metar(&plan.origin);
                }
                if ui
                    .button(format!("{} arrivo ↗", plan.dest))
                    .on_hover_text("METAR, TAF e meteo di arrivo")
                    .clicked()
                {
                    App::open_metar(&plan.dest);
                }
            });
            if let Some(briefing) = &self.briefing {
                for (icao, text) in [
                    (plan.origin.clone(), briefing.origin_metar.clone()),
                    (plan.dest.clone(), briefing.dest_metar.clone()),
                ] {
                    if text.trim().is_empty() {
                        continue;
                    }
                    ui.add_space(8.0);
                    ui.label(
                        RichText::new(format!("{icao} · METAR"))
                            .size(9.0)
                            .color(DIM)
                            .strong(),
                    );
                    ui.label(
                        RichText::new(text.trim())
                            .size(11.0)
                            .family(egui::FontFamily::Monospace)
                            .color(MUTED),
                    );
                }
            }
        });
    }

    /// Full-bleed map with the floating cards of the reference layout.
    pub fn map_page(&mut self, ui: &mut Ui) {
        // The map is the whole window: screen_rect is stable across egui's
        // layout passes, so the floating cards never get drawn twice.
        let rect = ui.ctx().screen_rect();
        if rect.width() < 360.0 || rect.height() < 320.0 {
            return;
        }

        let t = self.tele.clone();
        let fresh = self.fresh();
        let aircraft = if fresh && t.position_valid() {
            Some((
                t.lat,
                t.lon,
                if t.hdg_true.is_finite() { t.hdg_true } else { 0.0 },
            ))
        } else {
            None
        };
        let trail: Vec<(f64, f64)> = self
            .recorder
            .track
            .iter()
            .map(|point| (point.lat, point.lon))
            .collect();
        let plan = self.plan.clone();

        let query = self.map_search.trim().to_ascii_uppercase();
        let found: Vec<(String, String, f64, f64)> = self
            .world
            .airports
            .search(&query, 7)
            .iter()
            .map(|a| (a.ident.clone(), a.name.clone(), a.lat, a.lon))
            .collect();
        let matches: Vec<String> = found.iter().map(|a| a.0.clone()).collect();

        let blips = self.traffic.visible(&self.traffic_filter);
        let piloted = blips.len();
        let input = worldmap::MapInput {
            aircraft,
            plan: plan.as_ref(),
            trail: &trail,
            matches: &matches,
            query: &query,
            blips: &blips,
        };
        let map_response = self.map.show(ui, rect, &self.world, &input);

        if aircraft.is_none() && plan.is_none() {
            ui.painter().text(
                rect.center(),
                egui::Align2::CENTER_CENTER,
                "Start Microsoft Flight Simulator or load a SimBrief plan to see the route",
                egui::FontId::proportional(13.0),
                DIM,
            );
        }

        // ---------------------------------------------------------------- rail
        let mut goto: Option<Page> = None;
        let rail_rect = egui::Rect::from_min_size(
            egui::pos2(rect.left() + 14.0, rect.top() + 72.0),
            egui::vec2(62.0, 316.0),
        );
        let page_now = self.page;
        overlay(ui, rail_rect, |ui| {
            ui.spacing_mut().item_spacing.y = 5.0;
            for (index, page) in [
                Page::Map,
                Page::Dashboard,
                Page::Plan,
                Page::Logbook,
                Page::Stats,
                Page::Settings,
            ]
            .into_iter()
            .enumerate()
            {
                let (cell, response) =
                    ui.allocate_exact_size(egui::vec2(34.0, 42.0), Sense::click());
                let active = page_now == page;
                if active || response.hovered() {
                    ui.painter().rect_filled(
                        cell,
                        CornerRadius::same(12),
                        if active {
                            Color32::from_rgb(40, 28, 32)
                        } else {
                            Color32::from_rgb(28, 21, 24)
                        },
                    );
                }
                rail_icon(
                    ui.painter(),
                    cell.center(),
                    index as u8,
                    if active { ACCENT } else { MUTED },
                );
                if response.clicked() {
                    goto = Some(page);
                }
                response.on_hover_text(page.title());
            }
        });

        // ------------------------------------------------------------- top bar
        let bar_left = rect.left() + 90.0;
        let bar_right = rect.right() - 96.0;
        let bar_w = (bar_right - bar_left).clamp(300.0, 1040.0);
        let bar_rect = egui::Rect::from_center_size(
            egui::pos2((bar_left + bar_right) * 0.5, rect.top() + 38.0),
            egui::vec2(bar_w, 44.0),
        );

        let mut search = std::mem::take(&mut self.map_search);
        let mut picked: Option<(f64, f64)> = None;
        let mut toggle_flight = false;
        let recording = self.recorder.active;
        let can_fly = recording || fresh || self.briefing.is_some();
        let airport_count = self.world.airports.list.len();
        let waypoint_count = plan.as_ref().map(|p| p.route.len()).unwrap_or(0);
        let (link_text, link_color) = match self.link {
            LinkState::Connected if fresh => ("SIM LIVE", GREEN),
            LinkState::Connected => ("WAITING FOR DATA", ACCENT),
            LinkState::WaitingSim => ("WAITING FOR SIM", ACCENT),
            LinkState::NoDll => ("SIMCONNECT.DLL MISSING", RED),
            LinkState::Starting => ("STARTING", MUTED),
        };
        overlay_m(ui, bar_rect, 6, |ui| {
            ui.horizontal_centered(|ui| {
                let (icon_rect, _) =
                    ui.allocate_exact_size(egui::vec2(16.0, 16.0), Sense::hover());
                icon_search(ui.painter(), icon_rect.center(), MUTED);
                let field_w = (bar_w * 0.34).max(150.0);
                ui.add(
                    egui::TextEdit::singleline(&mut search)
                        .frame(false)
                        .hint_text("Search airports or waypoints")
                        .desired_width(field_w)
                        .font(egui::FontId::proportional(12.5)),
                );
                ui.add_space(6.0);
                map_chip(ui, &format!("{airport_count}"), "AIRPORTS");
                map_chip(ui, &format!("{waypoint_count}"), "WAYPOINTS");
                if piloted > 0 {
                    map_chip(ui, &format!("{piloted}"), "PILOTS");
                }
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    widgets::pill(ui, link_text, link_color);
                    ui.add_space(6.0);
                    ui.label(
                        RichText::new(fmt_local_hhmm(now_epoch()))
                            .size(11.5)
                            .family(egui::FontFamily::Monospace)
                            .color(MUTED),
                    );
                    ui.add_space(8.0);
                    let (label, color) = if recording {
                        ("STOP & SAVE", RED)
                    } else {
                        ("START FLIGHT", ACCENT)
                    };
                    if let Some(release) = self.update_state.available.clone() {
                        if widgets::action_button(
                            ui,
                            &format!("UPDATE {}", release.version),
                            ACCENT,
                            !self.update_state.downloading,
                        ) {
                            goto = Some(Page::Settings);
                        }
                        ui.add_space(6.0);
                    }
                    if widgets::action_button(ui, label, color, can_fly) {
                        toggle_flight = true;
                    }
                });
            });
        });

        // ------------------------------------------------------- callsign card
        let briefing = self.briefing.clone();
        let callsign = briefing
            .as_ref()
            .map(|b| b.callsign.clone())
            .filter(|value| !value.trim().is_empty())
            .or_else(|| (!t.atc_id.trim().is_empty()).then(|| t.atc_id.trim().to_string()))
            .unwrap_or_else(|| "----".to_string());
        let airline = briefing
            .as_ref()
            .map(|b| b.airline.clone())
            .unwrap_or_default();
        let model = briefing
            .as_ref()
            .map(|b| {
                if b.aircraft_name.is_empty() {
                    b.aircraft_type.clone()
                } else {
                    b.aircraft_name.clone()
                }
            })
            .unwrap_or_else(|| {
                if t.atc_model.trim().is_empty() {
                    t.title.trim().to_string()
                } else {
                    t.atc_model.trim().to_string()
                }
            });
        let registration = briefing
            .as_ref()
            .map(|b| b.registration.clone())
            .filter(|r| !r.trim().is_empty())
            .unwrap_or_else(|| t.atc_id.trim().to_string());
        let subtitle = if airline.trim().is_empty() {
            if model.trim().is_empty() {
                "NO AIRCRAFT DATA".to_string()
            } else {
                model.clone()
            }
        } else {
            format!("{airline} · {model}")
        };
        let subtitle = if registration.trim().is_empty() {
            subtitle
        } else {
            format!("{subtitle} · {registration}")
        };

        let (status_text, status_color) = if !fresh {
            ("WAITING FOR SIM", ACCENT)
        } else if t.on_ground {
            ("ON GROUND", MUTED)
        } else {
            ("EN ROUTE", GREEN)
        };
        let squawk = if fresh && !t.squawk.is_empty() {
            t.squawk.clone()
        } else {
            "----".to_string()
        };
        let trip = plan.as_ref().map(|plan| self.trip(&t, plan, fresh));
        let card_w = 378.0;
        let card_rect = egui::Rect::from_min_size(
            egui::pos2(rect.left() + 90.0, rect.top() + 72.0),
            egui::vec2                (card_w,
                (rect.bottom() - rect.top() - 132.0).clamp(400.0, 552.0),
            ),
        );
        let mut metar: Option<String> = None;
        let photo = self.photo.clone();
        let photo_status = self.photo_status.clone();
        let photo_hint = self.photo_lookup();
        let mut retry = false;
        overlay(ui, card_rect, |ui| {
            ui.spacing_mut().item_spacing.y = 3.0;
            ui.horizontal(|ui| {
                ui.label(
                    RichText::new(status_text)
                        .size(8.5)
                        .color(status_color)
                        .strong(),
                );
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    ui.label(
                        RichText::new(&squawk)
                            .size(12.0)
                            .family(egui::FontFamily::Monospace)
                            .color(if squawk == "----" { MUTED } else { GREEN }),
                    );
                    ui.label(RichText::new("XPDR").size(8.5).color(DIM).strong());
                });
            });

            ui.add_space(12.0);
            ui.label(
                RichText::new(&callsign)
                    .size(31.0)
                    .strong()
                    .family(egui::FontFamily::Proportional)
                    .color(TEXT),
            );
            ui.label(
                RichText::new(subtitle.to_uppercase())
                    .size(8.5)
                    .color(MUTED)
                    .strong(),
            );

            if let Some(texture) = &photo {
                ui.add_space(10.0);
                let size = texture.size_vec2();
                let width = card_w - 30.0;
                let scale = (width / size.x.max(1.0)).min(104.0 / size.y.max(1.0));
                ui.add(
                    egui::Image::from_texture(texture)
                        .fit_to_exact_size(size * scale)
                        .corner_radius(CornerRadius::same(10)),
                );
                ui.add_space(2.0);
                ui.label(
                    RichText::new(format!(
                        "PHOTO · {} · {}",
                        self.photo_source, self.photo_label
                    ))
                    .size(8.0)
                    .color(DIM),
                );
                if !self.photo_credit.is_empty() {
                    ui.label(RichText::new(&self.photo_credit).size(7.5).color(DIM));
                }
            } else if photo_status == "loading" {
                ui.add_space(10.0);
                ui.label(RichText::new("Fetching the aircraft photo…").size(10.0).color(DIM));
            } else {
                ui.add_space(8.0);
                ui.horizontal(|ui| {
                    ui.label(
                        RichText::new(if photo_status.is_empty() {
                            "No aircraft photo"
                        } else {
                            photo_status.as_str()
                        })
                        .size(9.5)
                        .color(DIM),
                    );
                    if !photo_hint.is_empty() && ui.small_button("Fetch").clicked() {
                        retry = true;
                    }
                });
            }

            ui.add_space(6.0);
            ui.horizontal(|ui| {
                ui.label(RichText::new("✈").size(10.0).color(ACCENT));
                ui.label(RichText::new("FLIGHT PLAN").size(8.5).color(TEXT).strong());
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    if let Some(plan) = &plan {
                        ui.label(
                            RichText::new(format!("{} NM", num(plan.total_nm(), 0)))
                                .size(8.5)
                                .color(DIM),
                        );
                    }
                });
            });
            let origin = plan
                .as_ref()
                .map(|p| p.origin.clone())
                .unwrap_or_else(|| "----".to_string());
            let dest = plan
                .as_ref()
                .map(|p| p.dest.clone())
                .unwrap_or_else(|| "----".to_string());
            let (dep, arr, to_go, fraction) = match trip {
                Some(trip) => (
                    if self.recorder.active {
                        fmt_local_hhmm(self.recorder.started_epoch())
                    } else {
                        "--:--".to_string()
                    },
                    trip.arrival(),
                    trip.time_to_go(),
                    trip.fraction,
                ),
                None => (
                    "--:--".to_string(),
                    "--:--".to_string(),
                    "—".to_string(),
                    0.0,
                ),
            };
            ui.add_space(6.0);
            ui.horizontal(|ui| {
                ui.vertical(|ui| {
                    ui.label(
                        RichText::new(&origin)
                            .size(20.0)
                            .strong()
                            .family(egui::FontFamily::Monospace)
                            .color(TEXT),
                    );
                    ui.label(
                        RichText::new(format!("PARTENZA {dep}"))
                            .size(8.5)
                            .color(MUTED),
                    );
                });
                ui.add_space(6.0);
                widgets::arrow_right(ui, DIM);
                ui.add_space(6.0);
                ui.vertical(|ui| {
                    ui.label(
                        RichText::new(&dest)
                            .size(20.0)
                            .strong()
                            .family(egui::FontFamily::Monospace)
                            .color(TEXT),
                    );
                    ui.label(
                        RichText::new(format!("ARRIVO {arr}"))
                            .size(8.5)
                            .color(ACCENT),
                    );
                });
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    ui.vertical(|ui| {
                        ui.label(RichText::new("TRA").size(8.0).color(DIM).strong());
                        ui.label(
                            RichText::new(&to_go)
                                .size(12.5)
                                .family(egui::FontFamily::Monospace)
                                .color(TEXT),
                        );
                    });
                });
            });
            ui.add_space(6.0);
            progress(ui, fraction, CYAN);
            ui.add_space(6.0);
            ui.horizontal(|ui| {
                ui.label(
                    RichText::new(format!("{:.0}%", fraction * 100.0))
                        .size(9.0)
                        .color(ACCENT),
                );
                ui.add_space(4.0);
                if ui
                    .small_button(format!("METAR {origin}"))
                    .on_hover_text("METAR e TAF su metar-taf.com")
                    .clicked()
                {
                    metar = Some(origin.clone());
                }
                if ui
                    .small_button(format!("METAR {dest}"))
                    .on_hover_text("METAR e TAF su metar-taf.com")
                    .clicked()
                {
                    metar = Some(dest.clone());
                }
            });

            ui.add_space(10.0);
            let dash = || "----".to_string();
            let vs_color = if !fresh {
                MUTED
            } else if t.vs_fpm > 100.0 {
                GREEN
            } else if t.vs_fpm < -100.0 {
                ORANGE
            } else {
                TEXT
            };
            let gs_text = if fresh { num(t.gs_kt, 0) } else { dash() };
            let alt_text = if fresh {
                num(self.alt_val(t.alt_ft), 0)
            } else {
                dash()
            };
            let vs_text = if fresh {
                num(self.vs_val(t.vs_fpm), 0)
            } else {
                dash()
            };
            let hdg_text = if fresh {
                format!("{:03.0}", t.hdg_mag)
            } else {
                dash()
            };
            ui.columns(4, |cols| {
                chip_stat(&mut cols[0], "GS", &gs_text, "kt", TEXT);
                chip_stat(&mut cols[1], "ALT", &alt_text, self.alt_unit(), ACCENT);
                chip_stat(&mut cols[2], "V/S", &vs_text, self.vs_unit(), vs_color);
                chip_stat(&mut cols[3], "HDG", &hdg_text, "°", BLUE);
            });

            ui.add_space(8.0);
            ui.horizontal(|ui| {
                let (dot, _) =
                    ui.allocate_exact_size(egui::vec2(9.0, 9.0), Sense::hover());
                ui.painter().circle_filled(
                    dot.center(),
                    3.0,
                    if fresh {
                        GREEN
                    } else {
                        Color32::from_rgb(78, 62, 65)
                    },
                );
                ui.label(RichText::new(status_text).size(8.5).color(if fresh { TEXT } else { MUTED }));
                let weather = if fresh {
                    format!(
                        "OAT {:.0}°C · WIND {:03.0}/{:.0}",
                        t.oat_c, t.wind_dir, t.wind_kt
                    )
                } else {
                    "OAT — · WIND —".to_string()
                };
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    ui.label(RichText::new(weather).size(8.5).color(MUTED));
                });
            });
        });
        if retry {
            let ctx = ui.ctx().clone();
            self.retry_photo(&photo_hint, &ctx);
        }

        // -------------------------------------------------------- map filters
        let panel_w = 336.0;
        let mut filters_open = self.map_filters_open;
        let panel_rect = egui::Rect::from_min_size(
            egui::pos2(rect.right() - panel_w - 92.0, rect.top() + 72.0),
            egui::vec2(
                panel_w,
                if filters_open {
                    (rect.bottom() - rect.top() - 132.0).clamp(390.0, 552.0)
                } else {
                    50.0
                },
            ),
        );
        let mut tab = self.map_filter_tab.min(2) as usize;
        let mut show_route = self.map.show_route;
        let mut show_trail = self.map.show_trail;
        let mut show_airports = self.map.show_airports;
        let mut show_places = self.map.show_places;
        let mut show_grid = self.map.show_grid;
        let mut show_labels = self.map.show_labels;
        let mut load_brief = false;
        let mut recenter = false;
        let mut refresh_traffic = false;
        let mut filter = self.traffic_filter.clone();
        let filter_was_active = filter.active();
        overlay(ui, panel_rect, |ui| {
            ui.horizontal(|ui| {
                let arrow = if filters_open { "-" } else { "+" };
                if ui
                    .small_button(arrow)
                    .on_hover_text(if filters_open {
                        "Nascondi il pannello dei filtri"
                    } else {
                        "Mostra il pannello dei filtri"
                    })
                    .clicked()
                {
                    filters_open = !filters_open;
                }
                ui.label(RichText::new("Map Filters").size(15.0).strong().color(TEXT));
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    if ui.small_button("FIT").on_hover_text("Recentre the map").clicked() {
                        recenter = true;
                    }
                    if !filters_open {
                        ui.label(RichText::new("nascosto").size(9.0).color(DIM));
                    }
                });
            });
            if !filters_open {
                return;
            }
            ui.add_space(10.0);
            segmented(ui, &mut tab, &["ALL", "TRAFFIC", "LAYERS"]);
            ui.add_space(12.0);

            if tab == 0 {
                caps(ui, "ACTIVE FLIGHT", DIM, 8.0);
                ui.add_space(6.0);
                ui.horizontal(|ui| {
                    let width = (ui.available_width() - 8.0) * 0.5;
                    if pill_button(ui, show_route, "PLANNED ROUTE", width) {
                        show_route = !show_route;
                    }
                    if pill_button(ui, show_trail, "FLOWN TRAIL", width) {
                        show_trail = !show_trail;
                    }
                });
                ui.add_space(14.0);
                ui.horizontal(|ui| {
                    caps(ui, "MAP DATA", DIM, 8.0);
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        ui.label(
                            RichText::new(format!("{airport_count} APT"))
                                .size(9.5)
                                .color(MUTED),
                        );
                    });
                });
                ui.add_space(6.0);
                toggle_row(ui, &mut show_airports, "Airport labels");
                toggle_row(ui, &mut show_places, "Country labels");
                toggle_row(ui, &mut show_labels, "Waypoint labels");
                ui.add_space(12.0);
                caps(ui, "AIRPORT SEARCH", DIM, 8.0);
                ui.add_space(4.0);
                ui.add(
                    egui::TextEdit::singleline(&mut search)
                        .hint_text("e.g. LIRF, EDDF, Nice…")
                        .desired_width(f32::INFINITY)
                        .font(egui::FontId::proportional(11.5)),
                );
                ui.add_space(10.0);
                if ui
                    .add_sized(
                        [ui.available_width(), 30.0],
                        egui::Button::new(if self.simbrief_busy {
                            "Loading SimBrief…"
                        } else {
                            "Load SimBrief plan"
                        }),
                    )
                    .clicked()
                {
                    load_brief = true;
                }
            } else if tab == 1 {
                let shown = self.traffic.count(&filter);
                ui.horizontal(|ui| {
                    caps(ui, "FLIGHT TRAFFIC", DIM, 8.0);
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        ui.label(
                            RichText::new(format!("{shown} / {}", self.traffic.pilots.len()))
                                .size(9.5)
                                .color(MUTED),
                        );
                    });
                });
                ui.add_space(6.0);
                toggle_row(
                    ui,
                    &mut filter.vatsim,
                    &format!("VATSIM · {} pilots", self.traffic.vatsim_total),
                );
                toggle_row(
                    ui,
                    &mut filter.ivao,
                    &format!("IVAO · {} pilots", self.traffic.ivao_total),
                );
                ui.add_space(4.0);
                let status = if self.traffic.loading {
                    "Downloading the feeds…".to_string()
                } else {
                    self.traffic.status_line()
                };
                ui.label(RichText::new(status).size(9.0).color(DIM));
                if filter.active() {
                    for error in [&self.traffic.vatsim_error, &self.traffic.ivao_error] {
                        if !error.is_empty() {
                            ui.label(RichText::new(error).size(9.0).color(RED));
                        }
                    }
                }
                ui.add_space(10.0);
                caps(ui, "AIRCRAFT TYPE", DIM, 8.0);
                ui.add_space(4.0);
                ui.add_enabled(
                    filter.active(),
                    egui::TextEdit::singleline(&mut filter.aircraft)
                        .hint_text("e.g. A320, B738…")
                        .desired_width(f32::INFINITY),
                );
                ui.add_space(8.0);
                caps(ui, "CALLSIGN", DIM, 8.0);
                ui.add_space(4.0);
                ui.add_enabled(
                    filter.active(),
                    egui::TextEdit::singleline(&mut filter.callsign)
                        .hint_text("e.g. ITY, DLH, RYR…")
                        .desired_width(f32::INFINITY),
                );
                ui.add_space(12.0);
                caps(ui, "STATUS", DIM, 8.0);
                ui.add_space(4.0);
                let mut status_index = filter.status as usize;
                if segmented(ui, &mut status_index, &["ALL", "IN FLIGHT", "ON GROUND"]) {
                    filter.status = status_index as u8;
                }
                ui.add_space(10.0);
                caps(ui, "ALTITUDE", DIM, 8.0);
                ui.add_space(4.0);
                let mut altitude_index = filter.altitude as usize;
                if segmented(
                    ui,
                    &mut altitude_index,
                    &["ALL", "< 10K", "10–30K", "> 30K"],
                ) {
                    filter.altitude = altitude_index as u8;
                }
                ui.add_space(12.0);
                ui.horizontal(|ui| {
                    let label = if self.traffic.loading {
                        "Refresh".to_string()
                    } else {
                        "Refresh now".to_string()
                    };
                    if ui
                        .add_enabled(
                            filter.active() && !self.traffic.loading,
                            egui::Button::new(label),
                        )
                        .clicked()
                    {
                        refresh_traffic = true;
                    }
                    if ui.button("Clear filters").clicked() {
                        filter.callsign.clear();
                        filter.aircraft.clear();
                        filter.status = 0;
                        filter.altitude = 0;
                    }
                });
            } else {
                caps(ui, "MAP LAYERS", DIM, 8.0);
                ui.add_space(6.0);
                toggle_row(ui, &mut show_route, "Planned route");
                toggle_row(ui, &mut show_trail, "Flown trail");
                toggle_row(ui, &mut show_airports, "Airports");
                toggle_row(ui, &mut show_places, "Country names");
                toggle_row(ui, &mut show_labels, "Labels");
                toggle_row(ui, &mut show_grid, "Coordinate grid");
                ui.add_space(14.0);
                ui.label(
                    RichText::new("Drag to pan · scroll to zoom · double-click to follow the aircraft.")
                        .size(9.5)
                        .color(DIM),
                );
            }
        });
        self.map_filter_tab = tab as u8;
        self.map_filters_open = filters_open;
        self.traffic_filter = filter;
        if self.traffic_filter.active() && !filter_was_active {
            self.traffic_changed();
        }
        if refresh_traffic {
            self.traffic_changed();
        }
        self.map.show_route = show_route;
        self.map.show_trail = show_trail;
        self.map.show_airports = show_airports;
        self.map.show_places = show_places;
        self.map.show_grid = show_grid;
        self.map.show_labels = show_labels;
        // Search results hang from the search field.
        if !found.is_empty() {
            let list_rect = egui::Rect::from_min_size(
                egui::pos2(bar_rect.left() + 24.0, bar_rect.bottom() + 6.0),
                egui::vec2(
                    (bar_w * 0.52).max(260.0),
                    34.0 + found.len() as f32 * 28.0,
                ),
            );
            overlay(ui, list_rect, |ui| {
                ui.spacing_mut().item_spacing.y = 2.0;
                for (ident, name, lat, lon) in &found {
                    let (row, response) =
                        ui.allocate_exact_size(egui::vec2(ui.available_width(), 28.0), Sense::click());
                    if response.hovered() {
                        ui.painter()
                            .rect_filled(row, CornerRadius::same(8), Color32::from_rgb(40, 29, 32));
                    }
                    ui.painter().text(
                        row.left_center() + egui::vec2(8.0, 0.0),
                        egui::Align2::LEFT_CENTER,
                        ident,
                        egui::FontId::monospace(11.5),
                        TEXT,
                    );
                    ui.painter().text(
                        row.left_center() + egui::vec2(62.0, 0.0),
                        egui::Align2::LEFT_CENTER,
                        truncate(name, 38),
                        egui::FontId::proportional(10.5),
                        MUTED,
                    );
                    if response.clicked() {
                        picked = Some((*lat, *lon));
                    }
                }
            });
        }

        self.map_search = search;
        if recenter {
            self.focus_plan();
        }
        if load_brief {
            self.load_simbrief();
        }

        // --------------------------------------------------------------- zoom
        let zoom_rect = egui::Rect::from_min_size(
            egui::pos2(rect.right() - 66.0, rect.center().y - 76.0),
            egui::vec2(52.0, 168.0),
        );
        overlay(ui, zoom_rect, |ui| {
            ui.spacing_mut().item_spacing.y = 4.0;
            for (sign, label) in [(1.0, "+"), (-1.0, "−"), (0.0, "◎")] {
                let (cell, response) =
                    ui.allocate_exact_size(egui::vec2(24.0, 36.0), Sense::click());
                if response.hovered() {
                    ui.painter()
                        .rect_filled(cell, CornerRadius::same(8), Color32::from_rgb(40, 29, 32));
                }
                ui.painter().text(
                    cell.center(),
                    egui::Align2::CENTER_CENTER,
                    label,
                    egui::FontId::proportional(15.0),
                    MUTED,
                );
                if response.clicked() {
                    if sign != 0.0 {
                        self.map.zoom_by(sign * 0.7);
                    } else {
                        self.focus_plan();
                    }
                }
            }
        });

        // ----------------------------------------------------------- bottom bar
        let streak = self.flight_streak();
        let bottom_rect = egui::Rect::from_min_size(
            egui::pos2(rect.left() + 90.0, rect.bottom() - 48.0),
            egui::vec2(264.0, 34.0),
        );
        overlay_m(ui, bottom_rect, 6, |ui| {
            ui.horizontal_centered(|ui| {
                let (flame, _) = ui.allocate_exact_size(egui::vec2(12.0, 12.0), Sense::hover());
                icon_flame(ui.painter(), flame.center(), if streak > 0 { ORANGE } else { DIM });
                ui.label(
                    RichText::new(if streak > 0 {
                        format!("{streak}-day streak")
                    } else {
                        "No streak yet".to_string()
                    })
                    .size(10.5)
                    .color(if streak > 0 { TEXT } else { MUTED }),
                );
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    for day in 0..5 {
                        let (dot, _) = ui.allocate_exact_size(egui::vec2(9.0, 9.0), Sense::hover());
                        ui.painter().circle_filled(
                            dot.center(),
                            3.0,
                            if day < streak.min(5) as i32 {
                                ORANGE
                            } else {
                                Color32::from_rgb(66, 52, 55)
                            },
                        );
                    }
                });
            });
        });

        ui.painter().text(
            rect.right_bottom() + egui::vec2(-24.0, -16.0),
            egui::Align2::RIGHT_BOTTOM,
            "Natural Earth · OurAirports · offline map",
            egui::FontId::proportional(9.0),
            DIM,
        );

        // Hovering the aircraft gives its numbers; a click locks the map on it.
        if let Some((lat, lon, _)) = aircraft {
            let at = self.map.screen_pos(rect, lat, lon);
            if let Some(pointer) = ui.ctx().pointer_hover_pos() {
                if at.distance(pointer) <= 18.0 {
                    let tip_rect = egui::Rect::from_min_size(
                        pointer + egui::vec2(18.0, 14.0),
                        egui::vec2(190.0, 74.0),
                    )
                    .intersect(rect.shrink(6.0));
                    let title = if callsign.trim().is_empty() || callsign == "----" {
                        "YOUR AIRCRAFT".to_string()
                    } else {
                        callsign.clone()
                    };
                    let lines = [
                        format!(
                            "{} {} · {} kt",
                            num(self.alt_val(t.alt_ft), 0),
                            self.alt_unit(),
                            num(t.gs_kt, 0)
                        ),
                        format!("Squawk {}", if squawk == "----" { "—" } else { squawk.as_str() }),
                        "Click to follow".to_string(),
                    ];
                    overlay_m(ui, tip_rect, 9, |ui| {
                        ui.spacing_mut().item_spacing.y = 1.0;
                        ui.label(RichText::new(title).size(11.0).strong().color(TEXT));
                        for line in lines {
                            ui.label(RichText::new(line).size(9.5).color(MUTED));
                        }
                    });
                    if map_response.clicked() {
                        self.map.center_on(lat, lon);
                        self.map.follow = true;
                        self.toast_msg("Following the aircraft");
                    }
                }
            }
        }

        if let Some(icao) = metar {
            App::open_metar(&icao);
        }
        if toggle_flight {
            self.toggle_flight();
        }

        // ------------------------------------------------------ traffic hover
        if let Some(pointer) = ui.ctx().pointer_hover_pos() {
            if rect.contains(pointer) && self.map.show_traffic {
                let mut best: Option<(&traffic::Pilot, f32)> = None;
                for pilot in self
                    .traffic
                    .pilots
                    .iter()
                    .filter(|pilot| self.traffic_filter.matches(pilot))
                {
                    let at = self.map.screen_pos(rect, pilot.lat, pilot.lon);
                    let distance = at.distance(pointer);
                    if distance < 11.0 && best.map(|(_, d)| distance < d).unwrap_or(true) {
                        best = Some((pilot, distance));
                    }
                }
                if let Some((pilot, _)) = best {
                    let mut info = vec![
                        format!("{} · {}", pilot.callsign, pilot.network_name()),
                        format!(
                            "{} · {} {:.0} kt",
                            pilot.aircraft.to_uppercase(),
                            if pilot.on_ground {
                                "ON GROUND".to_string()
                            } else {
                                format!("FL {:.0}", pilot.alt_ft)
                            },
                            pilot.gs_kt
                        ),
                        format!("HDG {:03.0}°", pilot.hdg),
                    ];
                    if pilot.aircraft.trim().is_empty() {
                        info.remove(1);
                        info.push(format!("{:.0} ft · {:.0} kt", pilot.alt_ft, pilot.gs_kt));
                    }
                    let tip_rect = egui::Rect::from_min_size(
                        pointer + egui::vec2(16.0, 16.0),
                        egui::vec2(186.0, 20.0 + info.len() as f32 * 15.0),
                    );
                    let tip_rect = tip_rect.intersect(rect.shrink(6.0));
                    overlay_m(ui, tip_rect, 9, |ui| {
                        ui.spacing_mut().item_spacing.y = 1.0;
                        for (index, line) in info.iter().enumerate() {
                            let mut text = RichText::new(line)
                                .size(if index == 0 { 11.0 } else { 9.5 })
                                .color(if index == 0 { TEXT } else { MUTED });
                            if index == 0 {
                                text = text.strong();
                            }
                            ui.label(text);
                        }
                    });
                    if map_response.clicked() {
                        self.map.center_on(pilot.lat, pilot.lon);
                        self.map.follow = false;
                    }
                }
            }
        }

        if let Some(page) = goto {
            self.page = page;
            if page == Page::Map {
                self.focus_plan();
            }
        }
        if let Some((lat, lon)) = picked {
            self.map.center_on(lat, lon);
            self.map.zoom = self.map.zoom.max(7.0);
            self.map.follow = false;
            self.map_search.clear();
        }

    }

    /// Consecutive days (ending today or yesterday) with a recorded flight.
    fn flight_streak(&self) -> i64 {
        if self.store.flights.is_empty() {
            return 0;
        }
        let mut days: Vec<i64> = self
            .store
            .flights
            .iter()
            .map(|f| (f.started / 86_400) as i64)
            .collect();
        days.sort_unstable();
        days.dedup();
        let today = (now_epoch() / 86_400) as i64;
        let Some(last) = days.last().copied() else {
            return 0;
        };
        let mut day = if last == today {
            today
        } else if last == today - 1 {
            today - 1
        } else {
            return 0;
        };
        let mut streak = 0;
        while days.binary_search(&day).is_ok() {
            streak += 1;
            day -= 1;
        }
        streak
    }

    /// Enroute time from the SimBrief OFP, used before the aircraft moves.
    pub fn planned_ete(&self) -> Option<f64> {
        self.briefing
            .as_ref()
            .map(|briefing| briefing.planned_ete_s)
            .filter(|seconds| seconds.is_finite() && *seconds > 0.0)
    }

    pub fn photo_query(&self) -> String {
        if let Some(briefing) = &self.briefing {
            if !briefing.photo_query.is_empty() {
                return briefing.photo_query.clone();
            }
        }
        if !self.tele.atc_model.trim().is_empty() {
            return self.tele.atc_model.trim().to_string();
        }
        self.tele.title.trim().to_string()
    }

    pub fn logbook(&mut self, ui: &mut Ui) {
        let mut export = false;
        ui.horizontal(|ui| {
            ui.label(RichText::new("Logbook").size(21.0).strong().color(TEXT));
            ui.add_space(8.0);
            ui.label(
                RichText::new(format!("{} flights recorded", self.store.flights.len()))
                    .size(12.0)
                    .color(MUTED),
            );
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                if ui.button("Export CSV").clicked() {
                    export = true;
                }
            });
        });
        if export {
            match self.store.export_csv() {
                Ok(path) => self.toast_msg(format!("CSV exported to {}", path.display())),
                Err(err) => self.toast_err(format!("Export failed: {err}")),
            }
        }

        ui.add_space(12.0);

        if self.store.flights.is_empty() {
            card(ui, "Empty logbook", |ui| {
                ui.label(
                    RichText::new(
                        "Flights are recorded automatically: take off in Microsoft Flight Simulator \
                         and the tracker starts sampling; touchdown, and the landing rate is saved here.",
                    )
                    .size(12.5)
                    .color(MUTED),
                );
            });
            return;
        }

        let mut action: Option<Action> = None;
        let list_height = 560.0;

        ui.horizontal_top(|ui| {
            ui.allocate_ui_with_layout(
                egui::vec2(342.0, list_height),
                Layout::top_down(Align::Min),
                |ui| {
                    egui::ScrollArea::vertical()
                        .id_salt("logbook_list")
                        .auto_shrink([false, false])
                        .show(ui, |ui| {
                            let count = self.store.flights.len();
                            for index in 0..count {
                                let (id, route, when, duration, aircraft, fpm, rating) = {
                                    let flight = &self.store.flights[index];
                                    (
                                        flight.id.clone(),
                                        flight.route_label(),
                                        fmt_utc(flight.started),
                                        fmt_duration(flight.duration_s),
                                        flight.aircraft.clone(),
                                        flight.landing.as_ref().map(|l| l.fpm.abs()),
                                        flight.landing.as_ref().map(|l| l.rating.clone()),
                                    )
                                };
                                let selected = self.selected.as_deref() == Some(id.as_str());
                                let (rect, response) = ui.allocate_exact_size(
                                    egui::vec2(ui.available_width(), 64.0),
                                    Sense::click(),
                                );
                                if selected || response.hovered() {
                                    ui.painter().rect_filled(
                                        rect,
                                        CornerRadius::same(8),
                                        if selected {
                                            Color32::from_rgb(40, 24, 28)
                                        } else {
                                            Color32::from_rgb(30, 20, 23)
                                        },
                                    );
                                }
                                let painter = ui.painter();
                                painter.text(
                                    rect.left_top() + egui::vec2(12.0, 9.0),
                                    egui::Align2::LEFT_TOP,
                                    &route,
                                    egui::FontId::proportional(13.5),
                                    TEXT,
                                );
                                painter.text(
                                    rect.left_top() + egui::vec2(12.0, 29.0),
                                    egui::Align2::LEFT_TOP,
                                    &aircraft,
                                    egui::FontId::proportional(10.0),
                                    DIM,
                                );
                                painter.text(
                                    rect.left_top() + egui::vec2(12.0, 47.0),
                                    egui::Align2::LEFT_TOP,
                                    format!("{when} · {duration}"),
                                    egui::FontId::monospace(9.5),
                                    MUTED,
                                );
                                if let Some(rate) = fpm {
                                    let color = rating
                                        .as_deref()
                                        .map(rating_color)
                                        .unwrap_or(MUTED);
                                    painter.text(
                                        rect.right_center() + egui::vec2(-12.0, -7.0),
                                        egui::Align2::RIGHT_CENTER,
                                        format!("{rate:.0}"),
                                        egui::FontId::monospace(15.0),
                                        color,
                                    );
                                    painter.text(
                                        rect.right_center() + egui::vec2(-12.0, 11.0),
                                        egui::Align2::RIGHT_CENTER,
                                        "fpm",
                                        egui::FontId::monospace(9.0),
                                        DIM,
                                    );
                                }
                                if response.clicked() {
                                    self.selected = Some(id.clone());
                                    self.scrub = 1.0;
                                }
                            }
                        });
                },
            );

            ui.add_space(4.0);

            ui.vertical(|ui| {
                let Some(id) = self.selected.clone() else {
                    card(ui, "Select a flight", |ui| {
                        ui.label(
                            RichText::new("Pick a flight on the left to see the track profile, the touchdown analysis and the exports.")
                                .size(12.5)
                                .color(MUTED),
                        );
                    });
                    return;
                };
                let Some(flight) = self.store.find(&id).cloned() else {
                    return;
                };

                ui.horizontal(|ui| {
                    ui.label(
                        RichText::new(flight.route_label())
                            .size(19.0)
                            .strong()
                            .color(TEXT),
                    );
                    ui.add_space(8.0);
                    ui.label(
                        RichText::new(fmt_utc_full(flight.started))
                            .size(11.5)
                            .family(egui::FontFamily::Monospace)
                            .color(MUTED),
                    );
                });
                ui.add_space(10.0);

                ui.columns(4, |cols| {
                    stat_tile(
                        &mut cols[0],
                        "Duration",
                        &fmt_clock(flight.duration_s),
                        "",
                        CYAN,
                    );
                    stat_tile(
                        &mut cols[1],
                        "Distance",
                        &num(flight.distance_nm, 0),
                        "NM",
                        TEXT,
                    );
                    stat_tile(
                        &mut cols[2],
                        "Max altitude",
                        &num(self.alt_val(flight.max_alt_ft), 0),
                        self.alt_unit(),
                        TEXT,
                    );
                    stat_tile(
                        &mut cols[3],
                        "Landing",
                        &flight
                            .landing
                            .as_ref()
                            .map(|l| format!("{:.0}", l.fpm.abs()))
                            .unwrap_or_else(|| "—".to_string()),
                        "fpm",
                        flight
                            .landing
                            .as_ref()
                            .map(|l| rating_color(&l.rating))
                            .unwrap_or(MUTED),
                    );
                });

                ui.add_space(12.0);
                card(ui, "Vertical profile", |ui| {
                    profile(
                        ui,
                        180.0,
                        &flight.track,
                        Some(self.scrub * flight.duration_s),
                        flight.duration_s,
                    );
                    ui.add_space(6.0);
                    ui.horizontal(|ui| {
                        ui.add(
                            egui::Slider::new(&mut self.scrub, 0.0..=1.0)
                                .show_value(false)
                                .text("Track"),
                        );
                        let sample = flight
                            .track
                            .iter()
                            .min_by(|a, b| {
                                (a.t - self.scrub * flight.duration_s)
                                    .abs()
                                    .partial_cmp(&(b.t - self.scrub * flight.duration_s).abs())
                                    .unwrap_or(std::cmp::Ordering::Equal)
                            })
                            .cloned();
                        if let Some(point) = sample {
                            ui.label(
                                RichText::new(format!(
                                    "{} · {} {} · {} kt · {:.3}, {:.3}",
                                    fmt_clock(point.t),
                                    num(self.alt_val(point.alt), 0),
                                    self.alt_unit(),
                                    num(point.gs, 0),
                                    point.lat,
                                    point.lon
                                ))
                                .size(10.5)
                                .family(egui::FontFamily::Monospace)
                                .color(MUTED),
                            );
                        }
                    });
                });

                ui.add_space(12.0);
                ui.horizontal_top(|ui| {
                    ui.vertical(|ui| {
                        card(ui, "Flight data", |ui| {
                            kv(ui, "Aircraft", if flight.aircraft.is_empty() { "—" } else { &flight.aircraft });
                            kv(ui, "Departure", if flight.origin.is_empty() { "—" } else { &flight.origin });
                            kv(ui, "Arrival", if flight.dest.is_empty() { "—" } else { &flight.dest });
                            kv(ui, "Started", &fmt_utc_full(flight.started));
                            kv(ui, "Ended", &fmt_utc_full(flight.ended));
                            kv(
                                ui,
                                "Fuel used",
                                &format!(
                                    "{} {}",
                                    num(self.fuel_val((flight.fuel_start_kg - flight.fuel_end_kg).max(0.0)), 0),
                                    self.fuel_unit()
                                ),
                            );
                            kv(ui, "Max ground speed", &format!("{} kt", num(flight.max_gs_kt, 0)));
                            if !flight.plan_title.is_empty() {
                                kv(ui, "Plan", &flight.plan_title);
                            }
                        });
                    });
                    ui.vertical(|ui| {
                        card(ui, "Touchdown", |ui| {
                            if let Some(landing) = &flight.landing {
                                let color = rating_color(&landing.rating);
                                ui.horizontal(|ui| {
                                    ui.label(
                                        RichText::new(&landing.rating).size(20.0).strong().color(color),
                                    );
                                    ui.label(
                                        RichText::new(format!("{:.0} fpm", landing.fpm.abs()))
                                            .size(16.0)
                                            .family(egui::FontFamily::Monospace)
                                            .color(TEXT),
                                    );
                                });
                                ui.add_space(6.0);
                                kv(ui, "Vertical speed", &format!("{:.0} fpm", landing.fpm.abs()));
                                kv(ui, "Load factor", &format!("{:.2} G", landing.g));
                                kv(ui, "Airspeed", &format!("{:.0} kt", landing.ias));
                                kv(ui, "Pitch", &format!("{:+.1}°", landing.pitch));
                                kv(ui, "Bank", &format!("{:+.1}°", landing.bank));
                                kv(ui, "Wind", &format!("{:03.0}° / {:.0} kt", landing.wind_dir, landing.wind_kt));
                                kv(ui, "Touchdown point", &format!("{:.4}, {:.4}", landing.lat, landing.lon));
                            } else {
                                ui.label(
                                    RichText::new("No landing detected for this flight.")
                                        .size(12.0)
                                        .color(MUTED),
                                );
                            }
                            ui.add_space(10.0);
                            ui.horizontal(|ui| {
                                if ui.button("Export GPX").clicked() {
                                    action = Some(Action::Gpx(flight.id.clone()));
                                }
                                if ui.button("Export KML").clicked() {
                                    action = Some(Action::Kml(flight.id.clone()));
                                }
                                let confirm = self.confirm_delete.as_deref() == Some(flight.id.as_str());
                                if ui
                                    .button(if confirm { "Confirm delete" } else { "Delete" })
                                    .clicked()
                                {
                                    if confirm {
                                        action = Some(Action::Delete(flight.id.clone()));
                                    } else {
                                        self.confirm_delete = Some(flight.id.clone());
                                    }
                                }
                            });
                        });
                    });
                });
            });
        });

        if let Some(action) = action {
            match action {
                Action::Gpx(id) => {
                    if let Some(flight) = self.store.find(&id).cloned() {
                        match self.store.export_gpx(&flight) {
                            Ok(path) => self.toast_msg(format!("GPX written to {}", path.display())),
                            Err(err) => self.toast_err(format!("GPX export failed: {err}")),
                        }
                    }
                }
                Action::Kml(id) => {
                    if let Some(flight) = self.store.find(&id).cloned() {
                        match self.store.export_kml(&flight) {
                            Ok(path) => self.toast_msg(format!("KML written to {}", path.display())),
                            Err(err) => self.toast_err(format!("KML export failed: {err}")),
                        }
                    }
                }
                Action::Delete(id) => match self.store.delete(&id) {
                    Ok(()) => {
                        self.selected = None;
                        self.confirm_delete = None;
                        self.toast_msg("Flight deleted");
                    }
                    Err(err) => self.toast_err(format!("Delete failed: {err}")),
                },
            }
        }
    }

    pub fn stats_page(&mut self, ui: &mut Ui) {
        let stats = self.store.stats();
        ui.label(RichText::new("Statistics").size(21.0).strong().color(TEXT));
        ui.add_space(12.0);

        ui.columns(4, |cols| {
            stat_tile(
                &mut cols[0],
                "Flights",
                &format!("{}", stats.flights),
                "",
                TEXT,
            );
            stat_tile(
                &mut cols[1],
                "Hours flown",
                &format!("{:.1}", stats.total_hours),
                "h",
                CYAN,
            );
            stat_tile(
                &mut cols[2],
                "Distance",
                &format!("{:.0}", stats.total_nm),
                "NM",
                TEXT,
            );
            stat_tile(
                &mut cols[3],
                "Average landing",
                &if stats.landings > 0 {
                    format!("{:.0}", stats.avg_landing_fpm)
                } else {
                    "—".into()
                },
                "fpm",
                ACCENT,
            );
        });

        ui.add_space(10.0);
        ui.columns(4, |cols| {
            stat_tile(
                &mut cols[0],
                "Best landing",
                &if stats.landings > 0 {
                    format!("{:.0}", stats.best_landing_fpm)
                } else {
                    "—".into()
                },
                "fpm",
                GREEN,
            );
            stat_tile(
                &mut cols[1],
                "Hardest landing",
                &if stats.landings > 0 {
                    format!("{:.0}", stats.worst_landing_fpm)
                } else {
                    "—".into()
                },
                "fpm",
                ORANGE,
            );
            stat_tile(
                &mut cols[2],
                "Average flight",
                &fmt_clock(stats.avg_duration_s),
                "",
                TEXT,
            );
            stat_tile(
                &mut cols[3],
                "Most flown",
                if stats.top_aircraft.is_empty() {
                    "—"
                } else {
                    &stats.top_aircraft
                },
                "",
                TEXT,
            );
        });

        ui.add_space(12.0);
        card(ui, "Landing rate history", |ui| {
            let values: Vec<f64> = self
                .store
                .flights
                .iter()
                .rev()
                .filter_map(|f| f.landing.as_ref().map(|l| l.fpm))
                .take(40)
                .collect();
            landing_bars(ui, 190.0, &values);
            ui.add_space(6.0);
            ui.label(
                RichText::new("Most recent 40 landings · lower bars are smoother touchdowns")
                    .size(10.5)
                    .color(DIM),
            );
        });

        if stats.landings > 0 {
            ui.add_space(12.0);
            card(ui, "Fleet", |ui| {
                let mut counts: std::collections::BTreeMap<String, usize> = Default::default();
                for flight in &self.store.flights {
                    let name = if flight.aircraft.is_empty() {
                        "Unknown".to_string()
                    } else {
                        flight.aircraft.clone()
                    };
                    *counts.entry(name).or_insert(0) += 1;
                }
                let mut rows: Vec<(String, usize)> = counts.into_iter().collect();
                rows.sort_by(|a, b| b.1.cmp(&a.1));
                egui::Grid::new("fleet")
                    .num_columns(2)
                    .spacing([24.0, 6.0])
                    .show(ui, |ui| {
                        for (name, count) in rows {
                            ui.label(RichText::new(name).size(12.0).color(TEXT));
                            ui.label(
                                RichText::new(format!("{count} flights"))
                                    .size(11.0)
                                    .family(egui::FontFamily::Monospace)
                                    .color(MUTED),
                            );
                            ui.end_row();
                        }
                    });
            });
        }
    }

    /// STOP asks first: ending a flight cannot be undone.
    pub fn stop_confirm(&mut self, ctx: &egui::Context) {
        if !self.confirm_stop {
            return;
        }
        let mut choice: Option<bool> = None;
        egui::Window::new("stop-flight-confirm")
            .id(egui::Id::new("stop-flight-confirm"))
            .anchor(egui::Align2::CENTER_CENTER, egui::Vec2::ZERO)
            .title_bar(false)
            .resizable(false)
            .collapsible(false)
            .frame(
                egui::Frame::new()
                    .fill(CARD)
                    .stroke(egui::Stroke::new(1.0_f32, BORDER))
                    .corner_radius(CornerRadius::same(18))
                    .inner_margin(egui::Margin::same(22)),
            )
            .show(ctx, |ui| {
                ui.set_width(360.0);
                ui.label(RichText::new("END THE FLIGHT?").size(8.5).color(ACCENT).strong());
                ui.add_space(5.0);
                ui.label(
                    RichText::new("The flight will be saved to the logbook")
                        .size(16.0)
                        .color(TEXT)
                        .strong(),
                );
                ui.add_space(6.0);
                ui.label(
                    RichText::new(format!(
                        "{} of flight time will be written to the logbook, with the route and the landing. This cannot be undone.",
                        fmt_clock(self.recorder.elapsed())
                    ))
                    .size(11.0)
                    .color(MUTED),
                );
                ui.add_space(16.0);
                ui.horizontal(|ui| {
                    if widgets::action_button(ui, "END & SAVE", RED, true) {
                        choice = Some(true);
                    }
                    ui.add_space(8.0);
                    if widgets::action_button(ui, "KEEP FLYING", MUTED, true) {
                        choice = Some(false);
                    }
                });
            });
        match choice {
            Some(true) => {
                self.confirm_stop = false;
                self.stop_flight();
            }
            Some(false) => self.confirm_stop = false,
            None => {}
        }
    }

    pub fn settings_page(&mut self, ui: &mut Ui) {
        ui.label(RichText::new("Settings").size(21.0).strong().color(TEXT));
        ui.add_space(12.0);

        card(ui, "Units", |ui| {
            let mut changed = false;
            if ui
                .checkbox(
                    &mut self.store.settings.imperial,
                    "Imperial units (feet, feet per minute, pounds)",
                )
                .changed()
            {
                changed = true;
            }
            ui.label(
                RichText::new(if self.store.settings.imperial {
                    "Altitudes in feet, vertical speeds in fpm, weights in pounds."
                } else {
                    "Altitudes in metres, vertical speeds in m/s, weights in kilograms."
                })
                .size(11.0)
                .color(DIM),
            );
            if changed {
                self.store.save_settings();
            }
        });

        ui.add_space(12.0);
        card(ui, "Recording", |ui| {
            let mut changed = false;
            if ui
                .checkbox(
                    &mut self.store.settings.auto_record,
                    "Start recording automatically when the aircraft gets airborne",
                )
                .changed()
            {
                changed = true;
            }
            ui.add_space(4.0);
            ui.horizontal(|ui| {
                ui.label(RichText::new("Track sample interval").size(12.0).color(MUTED));
                if ui
                    .add(
                        egui::Slider::new(&mut self.store.settings.sample_secs, 0.5..=10.0)
                            .suffix(" s")
                            .fixed_decimals(1),
                    )
                    .changed()
                {
                    changed = true;
                }
            });
            ui.label(
                RichText::new(
                    "A flight is closed and saved when the aircraft comes to a stop after touchdown. \
                     You can also stop it by hand below.",
                )
                .size(11.0)
                .color(DIM),
            );
            ui.add_space(8.0);
            ui.horizontal(|ui| {
                if self.recorder.active {
                    if ui.button("Stop and save flight").clicked() {
                        let record = self.recorder.stop();
                        if let Some(record) = record {
                            self.persist(record);
                        } else {
                            self.toast_msg("Flight too short to save");
                        }
                    }
                } else if ui.button("Start recording now").clicked() {
                    let telemetry = self.tele.clone();
                    self.recorder.start(&telemetry, self.plan.as_ref());
                    self.toast_msg("Recording started");
                }
                ui.label(
                    RichText::new(if self.recorder.active {
                        format!("Recording · {}", fmt_clock(self.recorder.elapsed()))
                    } else {
                        "Idle".to_string()
                    })
                    .size(11.5)
                    .color(if self.recorder.active { RED } else { MUTED }),
                );
            });
            if changed {
                self.store.save_settings();
            }
        });

        ui.add_space(12.0);
        card(ui, "Flight plan", |ui| {
            ui.horizontal(|ui| {
                if ui.button("Import .pln").clicked() {
                    self.pick_plan();
                }
                if self.plan.is_some() && ui.button("Unload plan").clicked() {
                    self.plan = None;
                    self.store.settings.last_plan = None;
                    self.store.save_settings();
                }
            });
            ui.label(
                RichText::new(
                    "Use the same .pln you load in the simulator, or drag it onto the window.",
                )
                .size(11.0)
                .color(DIM),
            );
        });

        ui.add_space(12.0);
        card(ui, "SimBrief", |ui| {
            ui.horizontal(|ui| {
                ui.label(RichText::new("Pilot ID or username").size(12.0).color(MUTED));
                if ui
                    .add(
                        egui::TextEdit::singleline(&mut self.store.settings.simbrief_id)
                            .desired_width(150.0)
                            .hint_text("e.g. 123456"),
                    )
                    .changed()
                {
                    self.store.save_settings();
                }
                if ui
                    .add_enabled(
                        !self.simbrief_busy,
                        egui::Button::new(if self.simbrief_busy {
                            "Loading…"
                        } else {
                            "Load latest plan"
                        }),
                    )
                    .clicked()
                {
                    self.load_simbrief();
                }
            });
            ui.label(
                RichText::new(
                    "Downloads the newest OFP from your SimBrief account: route and waypoints, \
                     cruise level, block fuel, planned enroute time, callsign and the aircraft photo.",
                )
                .size(11.0)
                .color(DIM),
            );
        });

        ui.add_space(12.0);
        card(ui, "Aircraft photo", |ui| {
            ui.label(
                RichText::new(
                    "A tail number gives the exact airframe. If the photo is still not the \
                     right one, type a search here and the card uses it instead.",
                )
                .size(11.0)
                .color(DIM),
            );
            ui.add_space(4.0);
            ui.horizontal(|ui| {
                if ui
                    .add(
                        egui::TextEdit::singleline(&mut self.store.settings.photo_query)
                            .desired_width(320.0)
                            .hint_text("e.g. Lufthansa A380"),
                    )
                    .changed()
                {
                    self.store.save_settings();
                    self.photo = None;
                    self.photo_for.clear();
                }
                if ui.button("Fetch photo").clicked() {
                    let lookup = self.photo_lookup();
                    let ctx = ui.ctx().clone();
                    self.retry_photo(&lookup, &ctx);
                }
                if ui.button("Automatic").clicked() {
                    self.store.settings.photo_query.clear();
                    self.store.save_settings();
                    self.photo = None;
                    self.photo_for.clear();
                    let lookup = self.photo_lookup();
                    let ctx = ui.ctx().clone();
                    self.retry_photo(&lookup, &ctx);
                }
            });
            ui.label(
                RichText::new(if self.photo_source.is_empty() {
                    format!("Photo for: {}", self.photo_lookup().key())
                } else {
                    format!("Current photo source: {}", self.photo_source)
                })
                .size(10.5)
                .color(DIM),
            );
        });

        ui.add_space(12.0);
        card(ui, "Cloud logbook (GitHub)", |ui| {
            let mut changed = false;
            ui.label(
                RichText::new(
                    "The same logbook here and on the website. Flights are stored in a public \
                     gist of your GitHub account: whoever has the logbook code can read it, and \
                     nothing passes through a server of ours.",
                )
                .size(10.5)
                .color(DIM),
            );
            ui.add_space(6.0);
            ui.horizontal(|ui| {
                ui.label(RichText::new("GitHub token").size(11.5).color(MUTED));
                if ui
                    .add(
                        egui::TextEdit::singleline(&mut self.store.settings.github_token)
                            .password(true)
                            .desired_width(300.0)
                            .hint_text("classic token with the gist scope"),
                    )
                    .changed()
                {
                    changed = true;
                }
            });
            ui.horizontal(|ui| {
                ui.label(RichText::new("Logbook code").size(11.5).color(MUTED));
                if ui
                    .add(
                        egui::TextEdit::singleline(&mut self.store.settings.github_gist)
                            .desired_width(300.0)
                            .hint_text("created by the first upload"),
                    )
                    .changed()
                {
                    changed = true;
                }
                if ui.button("Copy code").clicked() {
                    let code = self.store.settings.github_gist.trim().to_string();
                    if code.is_empty() {
                        self.cloud_error =
                            "Upload the logbook first: the code appears here".to_string();
                    } else {
                        ui.ctx().copy_text(code.clone());
                        self.cloud_status =
                            format!("Code {code} copied: paste it once on the website");
                    }
                }
            });
            if ui
                .checkbox(
                    &mut self.store.settings.sync_auto,
                    "Upload the logbook after every saved flight",
                )
                .changed()
            {
                changed = true;
            }
            if changed {
                self.store.save_settings();
            }
            ui.add_space(6.0);
            ui.horizontal(|ui| {
                if ui
                    .add_enabled(
                        !self.cloud_busy,
                        egui::Button::new(if self.cloud_busy {
                            "Working…"
                        } else {
                            "Upload logbook"
                        }),
                    )
                    .clicked()
                {
                    self.push_cloud(false);
                }
                if ui
                    .add_enabled(!self.cloud_busy, egui::Button::new("Download logbook"))
                    .clicked()
                {
                    self.pull_cloud();
                }
                if ui.button("Open the website").clicked() {
                    self.open_site();
                }
            });
            if !self.cloud_status.is_empty() {
                ui.label(RichText::new(&self.cloud_status).size(10.5).color(MUTED));
            }
            if !self.cloud_error.is_empty() {
                ui.label(RichText::new(&self.cloud_error).size(10.5).color(RED));
            }
            ui.label(
                RichText::new(
                    "The token is only for this app (github.com/settings/tokens → classic token \
                     with the gist scope) and stays in settings.json. The website needs no login: \
                     it opens the logbook with the code, asked once and remembered.",
                )
                .size(9.5)
                .color(DIM),
            );
        });

        card(ui, "Updates", |ui| {
            let mut changed = false;
            if ui
                .checkbox(
                    &mut self.store.settings.check_updates,
                    "Check for updates when the app starts",
                )
                .changed()
            {
                changed = true;
            }
            ui.horizontal(|ui| {
                ui.label(RichText::new("Manifest URL").size(11.5).color(MUTED));
                if ui
                    .add(
                        egui::TextEdit::singleline(&mut self.store.settings.update_url)
                            .desired_width(360.0)
                            .hint_text("https://…/latest.json"),
                    )
                    .changed()
                {
                    changed = true;
                }
            });
            if changed {
                self.store.save_settings();
            }
            ui.add_space(6.0);
            ui.horizontal(|ui| {
                let label = if self.update_state.checking {
                    "Checking…"
                } else {
                    "Verify updates"
                };
                if ui
                    .add_enabled(
                        !self.update_state.checking && !self.update_state.downloading,
                        egui::Button::new(label),
                    )
                    .clicked()
                {
                    self.check_updates(true);
                }
                if let Some(release) = self.update_state.available.clone() {
                    let label = if self.update_state.downloading {
                        format!("Downloading {}…", release.version)
                    } else {
                        format!("Download and install {}", release.version)
                    };
                    if ui
                        .add_enabled(!self.update_state.downloading, egui::Button::new(label))
                        .clicked()
                    {
                        self.install_update();
                    }
                }
                ui.label(
                    RichText::new(format!("Installed {}", update::VERSION))
                        .size(10.5)
                        .color(DIM),
                );
            });
            if !self.update_state.status.is_empty() {
                ui.label(
                    RichText::new(&self.update_state.status)
                        .size(10.5)
                        .color(MUTED),
                );
            }
            if !self.update_state.notes.is_empty() {
                ui.label(
                    RichText::new(&self.update_state.notes)
                        .size(10.5)
                        .color(DIM),
                );
            }
            if !self.update_state.error.is_empty() {
                ui.label(
                    RichText::new(&self.update_state.error)
                        .size(10.5)
                        .color(RED),
                );
            }
            ui.label(
                RichText::new(format!(
                    "Leave the address empty to use the official channel. Nothing installs on its own: you are told when a version is ready, and the setup asks before it replaces anything. Updates come from: {}",
                    update::DEFAULT_MANIFEST
                ))
                .size(10.5)
                .color(DIM),
            );
        });

        ui.add_space(12.0);
        card(ui, "Data", |ui| {
            ui.horizontal(|ui| {
                if ui.button("Open data folder").clicked() {
                    let _ = std::process::Command::new("explorer")
                        .arg(&self.store.dir)
                        .spawn();
                }
                if ui.button("Open exports folder").clicked() {
                    let _ = std::process::Command::new("explorer")
                        .arg(self.store.exports_dir())
                        .spawn();
                }
                if ui.button("Rescan flights").clicked() {
                    self.store.reload();
                    self.toast_msg(format!(
                        "Logbook reloaded · {} flights",
                        self.store.flights.len()
                    ));
                }
                if ui.button("Export CSV").clicked() {
                    match self.store.export_csv() {
                        Ok(path) => self.toast_msg(format!("CSV exported to {}", path.display())),
                        Err(err) => self.toast_err(format!("Export failed: {err}")),
                    }
                }
            });
            ui.add_space(6.0);
            ui.label(
                RichText::new(self.store.dir.display().to_string())
                    .size(10.5)
                    .family(egui::FontFamily::Monospace)
                    .color(DIM),
            );
            if self.store.load_errors > 0 {
                ui.label(
                    RichText::new(format!(
                        "{} flight file(s) could not be read",
                        self.store.load_errors
                    ))
                    .size(11.0)
                    .color(ORANGE),
                );
            }
            ui.add_space(10.0);
            if self.confirm_clear {
                ui.horizontal(|ui| {
                    ui.label(
                        RichText::new("Delete every recorded flight?")
                            .size(12.0)
                            .color(RED),
                    );
                    if ui.button("Yes, delete all").clicked() {
                        match self.store.clear_all() {
                            Ok(count) => {
                                self.selected = None;
                                self.toast_msg(format!("{count} flights deleted"));
                            }
                            Err(err) => self.toast_err(format!("Delete failed: {err}")),
                        }
                        self.confirm_clear = false;
                    }
                    if ui.button("Cancel").clicked() {
                        self.confirm_clear = false;
                    }
                });
            } else if ui.button("Delete all flights").clicked() {
                self.confirm_clear = true;
            }
        });

        ui.add_space(12.0);
        card(ui, "SimConnect link", |ui| {
            ui.label(
                RichText::new(&self.link_detail)
                    .size(12.0)
                    .color(if matches!(self.link, crate::sim::LinkState::Connected) {
                        GREEN
                    } else {
                        ACCENT
                    }),
            );
            if let Some(warning) = &self.sim_warning {
                ui.label(RichText::new(warning).size(11.0).color(ORANGE));
            }
            ui.add_space(6.0);
            ui.label(
                RichText::new(
                    "SimConnect.dll is loaded from the application folder. Keep it next to \
                     msfs-flight-tracker.exe — no other runtime is needed.",
                )
                .size(11.0)
                .color(DIM),
            );
        });

        ui.add_space(12.0);
        card(ui, "About", |ui| {
            kv(ui, "Version", env!("CARGO_PKG_VERSION"));
            kv(ui, "Engine", "Rust · egui (native, no browser)");
            kv(ui, "Simulator support", "MSFS 2020 / MSFS 2024 via SimConnect");
            if self.demo {
                ui.add_space(4.0);
                ui.label(
                    RichText::new("Demo mode: synthetic telemetry, data stored in a separate folder.")
                        .size(11.0)
                        .color(CYAN),
                );
            }
        });
    }
}
