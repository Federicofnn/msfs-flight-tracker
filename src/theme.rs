//! Dark cockpit theme shared by every screen.

use eframe::egui::{
    self, Color32, CornerRadius, FontFamily, FontId, Stroke, TextStyle, Visuals,
};

pub const BG: Color32 = Color32::from_rgb(8, 6, 8);
pub const PANEL: Color32 = Color32::from_rgb(14, 11, 13);
pub const CARD: Color32 = Color32::from_rgb(21, 17, 19);
pub const BORDER: Color32 = Color32::from_rgb(56, 40, 44);
pub const TEXT: Color32 = Color32::from_rgb(245, 238, 238);
pub const MUTED: Color32 = Color32::from_rgb(174, 152, 155);
pub const DIM: Color32 = Color32::from_rgb(122, 100, 104);
pub const ACCENT: Color32 = Color32::from_rgb(238, 66, 78);
pub const CYAN: Color32 = Color32::from_rgb(255, 150, 150);
pub const GREEN: Color32 = Color32::from_rgb(102, 214, 154);
pub const RED: Color32 = Color32::from_rgb(255, 84, 84);
pub const BLUE: Color32 = Color32::from_rgb(232, 148, 160);
pub const ORANGE: Color32 = Color32::from_rgb(255, 160, 92);

pub fn rating_color(rating: &str) -> Color32 {
    match rating {
        "BUTTER" => Color32::from_rgb(255, 208, 140),
        "SMOOTH" => GREEN,
        "GOOD" => Color32::from_rgb(150, 214, 150),
        "FIRM" => ACCENT,
        "HARD" => ORANGE,
        "VERY HARD" => RED,
        _ => Color32::from_rgb(255, 70, 70),
    }
}

pub fn apply(ctx: &egui::Context) {
    let mut style = (*ctx.style()).clone();
    let mut visuals = Visuals::dark();

    visuals.panel_fill = PANEL;
    visuals.window_fill = CARD;
    visuals.extreme_bg_color = Color32::from_rgb(12, 9, 11);
    visuals.faint_bg_color = CARD;
    visuals.hyperlink_color = CYAN;
    visuals.selection.bg_fill = Color32::from_rgb(74, 34, 40);
    visuals.selection.stroke = Stroke::new(1.0_f32, TEXT);

    for widget in [
        &mut visuals.widgets.noninteractive,
        &mut visuals.widgets.inactive,
        &mut visuals.widgets.hovered,
        &mut visuals.widgets.active,
        &mut visuals.widgets.open,
    ] {
        widget.corner_radius = CornerRadius::same(8);
    }
    visuals.widgets.noninteractive.bg_fill = CARD;
    visuals.widgets.noninteractive.bg_stroke = Stroke::new(1.0_f32, BORDER);
    visuals.widgets.noninteractive.fg_stroke = Stroke::new(1.0_f32, MUTED);

    visuals.widgets.inactive.bg_fill = Color32::from_rgb(26, 20, 23);
    visuals.widgets.inactive.weak_bg_fill = Color32::from_rgb(26, 20, 23);
    visuals.widgets.inactive.bg_stroke = Stroke::new(1.0_f32, BORDER);
    visuals.widgets.inactive.fg_stroke = Stroke::new(1.0_f32, TEXT);

    visuals.widgets.hovered.bg_fill = Color32::from_rgb(38, 28, 31);
    visuals.widgets.hovered.weak_bg_fill = Color32::from_rgb(38, 28, 31);
    visuals.widgets.hovered.bg_stroke = Stroke::new(1.0_f32, ACCENT);
    visuals.widgets.hovered.fg_stroke = Stroke::new(1.0_f32, TEXT);

    visuals.widgets.active.bg_fill = Color32::from_rgb(52, 34, 38);
    visuals.widgets.active.weak_bg_fill = Color32::from_rgb(52, 34, 38);
    visuals.widgets.active.bg_stroke = Stroke::new(1.0_f32, ACCENT);
    visuals.widgets.active.fg_stroke = Stroke::new(1.0_f32, TEXT);

    visuals.window_corner_radius = CornerRadius::same(14);
    style.visuals = visuals;

    style.text_styles.insert(
        TextStyle::Heading,
        FontId::new(19.0, FontFamily::Proportional),
    );
    style.text_styles.insert(
        TextStyle::Body,
        FontId::new(13.5, FontFamily::Proportional),
    );
    style
        .text_styles
        .insert(TextStyle::Button, FontId::new(13.0, FontFamily::Proportional));
    style.text_styles.insert(
        TextStyle::Small,
        FontId::new(11.0, FontFamily::Proportional),
    );
    style.text_styles.insert(
        TextStyle::Monospace,
        FontId::new(12.5, FontFamily::Monospace),
    );

    style.spacing.item_spacing = egui::vec2(10.0, 7.0);
    style.spacing.button_padding = egui::vec2(13.0, 7.0);
    style.spacing.scroll.bar_width = 8.0;

    ctx.set_style(style);
}
