use eframe::egui::{
    self, Color32, FontData, FontDefinitions, FontFamily, FontId, Stroke, TextStyle, Visuals,
};
use std::sync::Arc;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum Theme {
    #[default]
    Dark,
    Light,
}
impl Theme {
    pub fn palette(self) -> &'static Palette {
        match self {
            Self::Dark => &DARK,
            Self::Light => &LIGHT,
        }
    }
    pub fn apply(self, ctx: &egui::Context) {
        ctx.set_theme(match self {
            Self::Dark => egui::Theme::Dark,
            Self::Light => egui::Theme::Light,
        });
    }
}

#[derive(Clone, Copy)]
pub struct Palette {
    pub bg: Color32,
    pub sidebar: Color32,
    pub surface: Color32,
    pub hover: Color32,
    pub border: Color32,
    pub text: Color32,
    pub muted: Color32,
    pub dim: Color32,
    pub accent: Color32,
    pub selection_bg: Color32,
    pub green: Color32,
    pub red: Color32,
    pub blue: Color32,
    pub amber: Color32,
    pub code_bg: Color32,
    pub user_bg: Color32,
    pub accent_bg: Color32,
    pub error_bg: Color32,
    pub diff_add_bg: Color32,
    pub diff_remove_bg: Color32,
    pub diff_text: Color32,
    pub settings_bg: Color32,
    pub settings_nav: Color32,
    pub settings_card: Color32,
    pub switch_on: Color32,
    pub switch_off: Color32,
}

const DARK: Palette = Palette {
    bg: Color32::from_rgb(25, 25, 25),
    sidebar: Color32::from_rgb(32, 32, 32),
    surface: Color32::from_rgb(32, 32, 32),
    hover: Color32::from_rgb(44, 44, 44),
    border: Color32::from_rgb(50, 50, 48),
    text: Color32::from_rgb(232, 232, 230),
    muted: Color32::from_rgb(163, 163, 159),
    dim: Color32::from_rgb(119, 119, 115),
    accent: Color32::from_rgb(139, 124, 255),
    selection_bg: Color32::from_rgb(76, 68, 132),
    green: Color32::from_rgb(76, 195, 134),
    red: Color32::from_rgb(239, 122, 112),
    blue: Color32::from_rgb(63, 141, 224),
    amber: Color32::from_rgb(231, 177, 82),
    code_bg: Color32::from_rgb(22, 22, 22),
    user_bg: Color32::from_rgb(39, 39, 39),
    accent_bg: Color32::from_rgb(44, 40, 80),
    error_bg: Color32::from_rgb(49, 30, 34),
    diff_add_bg: Color32::from_rgb(22, 41, 31),
    diff_remove_bg: Color32::from_rgb(51, 25, 26),
    diff_text: Color32::from_rgb(177, 177, 188),
    settings_bg: Color32::from_rgb(20, 20, 19),
    settings_nav: Color32::from_rgb(25, 25, 24),
    settings_card: Color32::from_rgb(34, 34, 32),
    switch_on: Color32::from_rgb(161, 177, 220),
    switch_off: Color32::from_rgb(63, 63, 63),
};

const LIGHT: Palette = Palette {
    bg: Color32::from_rgb(250, 250, 248),
    sidebar: Color32::from_rgb(242, 242, 239),
    surface: Color32::from_rgb(255, 255, 253),
    hover: Color32::from_rgb(230, 230, 227),
    border: Color32::from_rgb(215, 215, 210),
    text: Color32::from_rgb(38, 38, 36),
    muted: Color32::from_rgb(95, 95, 90),
    dim: Color32::from_rgb(111, 111, 105),
    accent: Color32::from_rgb(101, 78, 210),
    selection_bg: Color32::from_rgb(185, 174, 232),
    green: Color32::from_rgb(29, 120, 76),
    red: Color32::from_rgb(185, 57, 49),
    blue: Color32::from_rgb(34, 106, 184),
    amber: Color32::from_rgb(147, 98, 15),
    code_bg: Color32::from_rgb(241, 241, 238),
    user_bg: Color32::from_rgb(235, 235, 231),
    accent_bg: Color32::from_rgb(237, 232, 252),
    error_bg: Color32::from_rgb(253, 235, 233),
    diff_add_bg: Color32::from_rgb(229, 244, 232),
    diff_remove_bg: Color32::from_rgb(253, 233, 231),
    diff_text: Color32::from_rgb(80, 80, 88),
    settings_bg: Color32::from_rgb(250, 250, 248),
    settings_nav: Color32::from_rgb(242, 242, 239),
    settings_card: Color32::from_rgb(255, 255, 253),
    switch_on: Color32::from_rgb(101, 78, 210),
    switch_off: Color32::from_rgb(190, 190, 184),
};

/// Resolve per context, not globally: windows and tests may use different themes.
pub fn palette(ctx: &egui::Context) -> &'static Palette {
    match ctx.theme() {
        egui::Theme::Dark => &DARK,
        egui::Theme::Light => &LIGHT,
    }
}

pub fn install(ctx: &egui::Context) {
    let mut fonts = FontDefinitions::default();
    // System typography on macOS; egui's bundled fonts remain the portable fallback.
    #[cfg(target_os = "macos")]
    for (name, path, family) in [
        (
            "system",
            "/System/Library/Fonts/Supplemental/Arial.ttf",
            FontFamily::Proportional,
        ),
        (
            "system-mono",
            "/System/Library/Fonts/SFNSMono.ttf",
            FontFamily::Monospace,
        ),
    ] {
        if let Ok(bytes) = std::fs::read(path) {
            fonts
                .font_data
                .insert(name.into(), Arc::new(FontData::from_owned(bytes)));
            fonts
                .families
                .entry(family)
                .or_default()
                .insert(0, name.into());
        }
    }
    if let Ok(bytes) = std::fs::read("/System/Library/Fonts/Supplemental/Arial Bold.ttf") {
        fonts
            .font_data
            .insert("body-bold".into(), Arc::new(FontData::from_owned(bytes)));
        fonts.families.insert(
            FontFamily::Name("body-bold".into()),
            vec!["body-bold".into()],
        );
    } else {
        fonts.families.insert(
            FontFamily::Name("body-bold".into()),
            fonts.families[&FontFamily::Proportional].clone(),
        );
    }
    let serif = std::fs::read("/System/Library/Fonts/Supplemental/Georgia Bold Italic.ttf").ok();
    if let Some(bytes) = serif {
        fonts
            .font_data
            .insert("pi-serif".into(), Arc::new(FontData::from_owned(bytes)));
        fonts
            .families
            .insert(FontFamily::Name("pi-serif".into()), vec!["pi-serif".into()]);
    } else {
        fonts.families.insert(
            FontFamily::Name("pi-serif".into()),
            fonts.families[&FontFamily::Proportional].clone(),
        );
    }
    ctx.set_fonts(fonts);
    for (mode, theme) in [
        (Theme::Dark, egui::Theme::Dark),
        (Theme::Light, egui::Theme::Light),
    ] {
        install_style(ctx, mode, theme);
    }
    Theme::Dark.apply(ctx);
}

fn install_style(ctx: &egui::Context, mode: Theme, theme: egui::Theme) {
    let colors = mode.palette();
    let mut style = (*ctx.style_of(theme)).clone();
    style.visuals = match mode {
        Theme::Dark => Visuals::dark(),
        Theme::Light => Visuals::light(),
    };
    style.visuals.override_text_color = Some(colors.text);
    style.visuals.weak_text_color = Some(colors.muted);
    style.visuals.warn_fg_color = colors.amber;
    style.visuals.error_fg_color = colors.red;
    style.interaction.selectable_labels = false;
    style.visuals.interact_cursor = Some(egui::CursorIcon::PointingHand);
    style.visuals.panel_fill = colors.bg;
    style.visuals.window_fill = colors.sidebar;
    style.visuals.extreme_bg_color = colors.bg;
    style.visuals.faint_bg_color = colors.surface;
    style.visuals.window_stroke = Stroke::new(1.0, colors.border);
    style.visuals.window_corner_radius = 12.into();
    style.visuals.selection.bg_fill = colors.selection_bg;
    style.visuals.text_cursor.blink = false;
    style.visuals.selection.stroke = Stroke::new(1.0, colors.accent);
    for widget in [
        &mut style.visuals.widgets.inactive,
        &mut style.visuals.widgets.active,
        &mut style.visuals.widgets.hovered,
    ] {
        widget.corner_radius = 7.into();
        widget.bg_stroke = Stroke::NONE;
    }
    style.visuals.widgets.inactive.bg_fill = colors.surface;
    style.visuals.widgets.inactive.weak_bg_fill = colors.surface;
    style.visuals.widgets.hovered.weak_bg_fill = colors.hover;
    style.visuals.widgets.active.weak_bg_fill = colors.hover;
    style.visuals.widgets.hovered.bg_fill = colors.hover;
    style.visuals.widgets.active.bg_fill = colors.hover;
    for widget in [
        &mut style.visuals.widgets.noninteractive,
        &mut style.visuals.widgets.inactive,
        &mut style.visuals.widgets.active,
        &mut style.visuals.widgets.hovered,
        &mut style.visuals.widgets.open,
    ] {
        widget.fg_stroke = Stroke::new(1.0, colors.text);
    }
    style.visuals.widgets.open.bg_fill = colors.hover;
    style.visuals.widgets.open.weak_bg_fill = colors.hover;
    style.visuals.widgets.noninteractive.bg_stroke = Stroke::new(1.0, colors.border);
    style.visuals.hyperlink_color = colors.accent;
    style.spacing.item_spacing = egui::vec2(8.0, 8.0);
    style.spacing.button_padding = egui::vec2(10.0, 7.0);
    style.spacing.scroll.bar_width = 4.0;
    style.spacing.scroll.floating = true;
    style
        .text_styles
        .insert(TextStyle::Body, FontId::proportional(14.0));
    style
        .text_styles
        .insert(TextStyle::Button, FontId::proportional(13.0));
    style
        .text_styles
        .insert(TextStyle::Small, FontId::proportional(11.0));
    style
        .text_styles
        .insert(TextStyle::Monospace, FontId::monospace(12.0));
    ctx.set_style_of(theme, style);
}

pub fn small(ui: &egui::Ui, text: impl Into<String>) -> egui::RichText {
    egui::RichText::new(text)
        .size(11.0)
        .color(palette(ui.ctx()).muted)
}
pub fn caption(ui: &egui::Ui, text: impl Into<String>) -> egui::RichText {
    egui::RichText::new(text)
        .size(10.0)
        .color(palette(ui.ctx()).dim)
        .strong()
        .extra_letter_spacing(1.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn themes_are_context_local_and_keep_scale() {
        let dark = egui::Context::default();
        let light = egui::Context::default();
        install(&dark);
        install(&light);
        light.set_zoom_factor(1.2);
        let _ = light.run(egui::RawInput::default(), |_| {});
        for mode in [Theme::Light, Theme::Dark, Theme::Light] {
            mode.apply(&light);
            let colors = mode.palette();
            let visuals = &light.style().visuals;
            assert_eq!(palette(&light).bg, colors.bg);
            assert_eq!(visuals.panel_fill, colors.bg);
            assert_eq!(visuals.override_text_color, Some(colors.text));
            assert_eq!(visuals.hyperlink_color, colors.accent);
            assert_eq!(visuals.dark_mode, mode == Theme::Dark);
            assert_eq!(light.zoom_factor(), 1.2);
            assert_eq!(palette(&dark).bg, DARK.bg);
            assert!(dark.style().visuals.dark_mode);
        }
    }

    fn luminance(color: Color32) -> f32 {
        [color.r(), color.g(), color.b()]
            .into_iter()
            .zip([0.2126, 0.7152, 0.0722])
            .map(|(channel, weight)| {
                let channel = channel as f32 / 255.0;
                weight
                    * if channel <= 0.04045 {
                        channel / 12.92
                    } else {
                        ((channel + 0.055) / 1.055).powf(2.4)
                    }
            })
            .sum()
    }

    #[test]
    fn selection_is_distinct_from_surfaces_and_keeps_text_readable() {
        let contrast = |a: Color32, b: Color32| {
            let (a, b) = (luminance(a), luminance(b));
            (a.max(b) + 0.05) / (a.min(b) + 0.05)
        };
        let ctx = egui::Context::default();
        install(&ctx);
        for theme in [Theme::Dark, Theme::Light] {
            theme.apply(&ctx);
            let colors = theme.palette();
            assert_eq!(ctx.style().visuals.selection.bg_fill, colors.selection_bg);
            assert_eq!(colors.selection_bg.a(), 255);
            assert!(contrast(colors.text, colors.selection_bg) >= 4.5);
            for surface in [
                colors.bg,
                colors.sidebar,
                colors.surface,
                colors.hover,
                colors.user_bg,
            ] {
                assert!(contrast(colors.selection_bg, surface) >= 1.5);
            }
        }
    }

    #[test]
    fn light_text_and_status_colors_have_readable_contrast() {
        let colors = Theme::Light.palette();
        for background in [colors.bg, colors.sidebar, colors.surface] {
            for foreground in [
                colors.text,
                colors.muted,
                colors.dim,
                colors.accent,
                colors.green,
                colors.red,
                colors.blue,
                colors.amber,
            ] {
                let contrast = (luminance(background) + 0.05) / (luminance(foreground) + 0.05);
                assert!(
                    contrast >= 4.5,
                    "{foreground:?} on {background:?}: {contrast}"
                );
            }
        }
    }
}
