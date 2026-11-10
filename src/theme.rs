use eframe::egui::{
    self, Color32, FontData, FontDefinitions, FontFamily, FontId, Stroke, TextStyle, Visuals,
};
use std::sync::Arc;

#[cfg(test)]
mod bidi_tests;
mod fonts;
mod styles;
pub use styles::{DarkStyle, LightStyle, Styles};

/// Shared logical-pixel height for app controls. Chat controls opt into compact spacing.
pub const CONTROL_HEIGHT: f32 = 32.0;
pub const COMPACT_HEIGHT: f32 = 28.0;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum Theme {
    #[default]
    Dark,
    Light,
}
impl Theme {
    pub fn palette(self) -> &'static Palette {
        Styles::default().palette(self)
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

const NEUTRAL_DARK: Palette = Palette {
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

const NEUTRAL_LIGHT: Palette = Palette {
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
    let mode = match ctx.theme() {
        egui::Theme::Dark => Theme::Dark,
        egui::Theme::Light => Theme::Light,
    };
    styles(ctx).palette(mode)
}

pub fn install(ctx: &egui::Context) {
    ctx.set_fonts(fonts::definitions());
    Styles::default().apply(ctx, Theme::Dark);
}

pub fn styles(ctx: &egui::Context) -> Styles {
    ctx.data(|data| data.get_temp(egui::Id::new("color-styles")))
        .unwrap_or_default()
}

impl Styles {
    pub fn apply(self, ctx: &egui::Context, mode: Theme) {
        let installed = ctx.data(|data| data.get_temp::<Styles>(egui::Id::new("color-styles")));
        if installed != Some(self) {
            install_style(
                ctx,
                Theme::Dark,
                egui::Theme::Dark,
                self.palette(Theme::Dark),
            );
            install_style(
                ctx,
                Theme::Light,
                egui::Theme::Light,
                self.palette(Theme::Light),
            );
            ctx.data_mut(|data| data.insert_temp(egui::Id::new("color-styles"), self));
            ctx.request_repaint();
        }
        mode.apply(ctx);
    }
}

fn install_style(ctx: &egui::Context, mode: Theme, theme: egui::Theme, colors: &Palette) {
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
    style.visuals.widgets.inactive.bg_stroke = Stroke::new(1.0, colors.border);
    style.visuals.widgets.hovered.weak_bg_fill = colors.hover;
    style.visuals.widgets.active.weak_bg_fill = colors.hover;
    style.visuals.widgets.hovered.bg_fill = colors.hover;
    style.visuals.widgets.hovered.bg_stroke = Stroke::new(1.0, colors.dim);
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
    style.spacing.interact_size.y = CONTROL_HEIGHT;
    style.spacing.button_padding = egui::vec2(12.0, 6.0);
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

/// Tiny status indicators need more color and area than text in light mode.
/// Keep dark-mode colors/geometry unchanged and preserve each semantic hue.
pub fn status_dot(ui: &egui::Ui, center: egui::Pos2, color: Color32, radius: f32) {
    let colors = palette(ui.ctx());
    let (color, radius) = if ui.ctx().theme() == egui::Theme::Light {
        let color = if color == colors.accent {
            if styles(ui.ctx()).light == LightStyle::Neutral {
                Color32::from_rgb(80, 42, 198)
            } else {
                Color32::from_rgb(
                    (color.r() as f32 * 0.8) as u8,
                    (color.g() as f32 * 0.8) as u8,
                    (color.b() as f32 * 0.8) as u8,
                )
            }
        } else if color == colors.green {
            Color32::from_rgb(0, 101, 50)
        } else if color == colors.red {
            Color32::from_rgb(165, 30, 36)
        } else if color == colors.blue {
            Color32::from_rgb(15, 80, 160)
        } else if color == colors.amber {
            Color32::from_rgb(127, 72, 0)
        } else if color == colors.dim || color == colors.muted {
            Color32::from_rgb(82, 82, 76)
        } else {
            color
        };
        let color = if styles(ui.ctx()).light == LightStyle::Neutral {
            color
        } else {
            Color32::from_rgb(
                (color.r() as f32 * 0.9) as u8,
                (color.g() as f32 * 0.9) as u8,
                (color.b() as f32 * 0.9) as u8,
            )
        };
        (color, radius + 1.5)
    } else {
        (color, radius)
    };
    ui.painter().circle_filled(center, radius, color);
}

/// Matching input geometry for settings search, resource forms and toolbars.
pub fn text_field<'a>(text: &'a mut String, hint: &str) -> egui::TextEdit<'a> {
    egui::TextEdit::singleline(text)
        .hint_text(hint)
        .font(FontId::proportional(12.0))
        .margin(egui::Margin::symmetric(10, 7))
        .min_size(egui::vec2(0.0, CONTROL_HEIGHT))
}

/// A quiet selected state shared by settings choices and resource filters.
pub fn choice(ui: &mut egui::Ui, selected: bool, label: impl Into<String>) -> egui::Response {
    let colors = palette(ui.ctx());
    ui.add(
        egui::Button::new(egui::RichText::new(label).size(12.0).color(if selected {
            colors.accent
        } else {
            colors.muted
        }))
        .selected(selected)
        .fill(if selected {
            colors.accent_bg
        } else {
            Color32::TRANSPARENT
        })
        .stroke(Stroke::NONE)
        .corner_radius(7)
        .min_size(egui::vec2(0.0, CONTROL_HEIGHT)),
    )
}

pub fn slider_style(ui: &mut egui::Ui) {
    let colors = palette(ui.ctx());
    ui.spacing_mut().slider_rail_height = 4.0;
    ui.visuals_mut().widgets.inactive.bg_fill = colors.border;
    ui.visuals_mut().slider_trailing_fill = true;
}

pub fn compact_controls(ui: &mut egui::Ui) {
    ui.spacing_mut().interact_size.y = COMPACT_HEIGHT;
    ui.spacing_mut().button_padding = egui::vec2(6.0, 5.0);
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
    fn hebrew_letters_have_glyphs_in_every_text_family() {
        let ctx = egui::Context::default();
        install(&ctx);
        let _ = ctx.run(egui::RawInput::default(), |ctx| {
            for family in [
                FontFamily::Proportional,
                FontFamily::Monospace,
                FontFamily::Name("body-bold".into()),
                FontFamily::Name("pi-serif".into()),
            ] {
                let font = FontId::new(14.0, family);
                ctx.fonts(|fonts| {
                    for c in "אבגדהוזחטיךכלםמןנסעףפץצקרשת".chars() {
                        assert!(
                            fonts.has_glyph(&font, c),
                            "Missing Hebrew glyph {c} in {font:?}"
                        );
                    }
                });
            }
        });
    }
    #[cfg(target_os = "macos")]
    #[test]
    fn installed_language_fallbacks_cover_common_scripts() {
        let ctx = egui::Context::default();
        install(&ctx);
        let _ = ctx.run(egui::RawInput::default(), |ctx| {
            for family in [
                FontFamily::Proportional,
                FontFamily::Monospace,
                FontFamily::Name("body-bold".into()),
                FontFamily::Name("pi-serif".into()),
            ] {
                let font = FontId::new(14.0, family);
                ctx.fonts(|fonts| {
                    for c in "שלום Ελληνικά Привет مرحبا हिन्दी ไทย 日本語 中文 한국어"
                        .chars()
                        .filter(|c| !c.is_whitespace())
                    {
                        assert!(
                            fonts.has_glyph(&font, c),
                            "Missing language glyph {c} in {font:?}"
                        );
                    }
                });
            }
        });
    }
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
            assert_eq!(palette(&dark).bg, Theme::Dark.palette().bg);
            assert!(dark.style().visuals.dark_mode);
        }
    }

    #[test]
    fn classic_is_the_default_and_preserves_the_original_colors() {
        let defaults = Styles::default();
        assert_eq!(defaults.dark, DarkStyle::Neutral);
        assert_eq!(defaults.light, LightStyle::Neutral);
        assert_eq!(DarkStyle::ALL[0].label(), "Classic");
        assert_eq!(LightStyle::ALL[0].label(), "Classic");
        let dark = defaults.palette(Theme::Dark);
        assert_eq!(dark.bg, Color32::from_rgb(25, 25, 25));
        assert_eq!(dark.sidebar, Color32::from_rgb(32, 32, 32));
        assert_eq!(dark.accent, Color32::from_rgb(139, 124, 255));
        assert_eq!(dark.dim, Color32::from_rgb(119, 119, 115));
        let light = defaults.palette(Theme::Light);
        assert_eq!(light.bg, Color32::from_rgb(250, 250, 248));
        assert_eq!(light.accent, Color32::from_rgb(101, 78, 210));
        // Existing saved Neutral selections still map to the original palette.
        let saved: Styles =
            serde_json::from_str(r#"{"dark":"Neutral","light":"Neutral"}"#).unwrap();
        assert_eq!(saved, defaults);
    }

    fn all_styles() -> Vec<(Theme, Styles)> {
        DarkStyle::ALL
            .into_iter()
            .map(|dark| {
                (
                    Theme::Dark,
                    Styles {
                        dark,
                        ..Default::default()
                    },
                )
            })
            .chain(LightStyle::ALL.into_iter().map(|light| {
                (
                    Theme::Light,
                    Styles {
                        light,
                        ..Default::default()
                    },
                )
            }))
            .collect()
    }

    #[test]
    fn styles_update_native_widgets_without_leaking_across_contexts() {
        let first = egui::Context::default();
        let second = egui::Context::default();
        install(&first);
        install(&second);
        first.set_zoom_factor(1.15);
        let _ = first.run(egui::RawInput::default(), |_| {});
        for (mode, selected) in all_styles() {
            selected.apply(&first, mode);
            let colors = selected.palette(mode);
            assert_eq!(palette(&first).bg, colors.bg);
            assert_eq!(palette(&first).accent, colors.accent);
            assert_eq!(first.style().visuals.panel_fill, colors.bg);
            assert_eq!(first.style().visuals.hyperlink_color, colors.accent);
            assert_eq!(first.style().visuals.selection.bg_fill, colors.selection_bg);
            assert_eq!(first.zoom_factor(), 1.15);
            assert_eq!(styles(&second), Styles::default());
            assert_eq!(palette(&second).bg, Theme::Dark.palette().bg);
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
        for (theme, styles) in all_styles() {
            styles.apply(&ctx, theme);
            let colors = styles.palette(theme);
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
    fn sidebar_status_dots_are_stronger_in_light_mode_and_unchanged_in_dark() {
        let ctx = egui::Context::default();
        install(&ctx);
        for (mode, selected) in all_styles() {
            selected.apply(&ctx, mode);
            let colors = selected.palette(mode);
            for color in [
                colors.accent,
                colors.green,
                colors.red,
                colors.blue,
                colors.amber,
                colors.dim,
                colors.muted,
            ] {
                for radius in [2.5, 3.0] {
                    let output = ctx.run(egui::RawInput::default(), |ctx| {
                        egui::CentralPanel::default().show(ctx, |ui| {
                            status_dot(ui, egui::pos2(30.0, 30.0), color, radius);
                        });
                    });
                    let dot = output
                        .shapes
                        .iter()
                        .find_map(|s| match &s.shape {
                            egui::Shape::Circle(c) => Some(c),
                            _ => None,
                        })
                        .expect("status dot is painted");
                    assert_eq!(dot.fill.a(), 255);
                    if mode == Theme::Light {
                        assert_eq!(dot.radius, radius + 1.5);
                        for background in [colors.sidebar, colors.surface, colors.hover] {
                            let contrast =
                                (luminance(background) + 0.05) / (luminance(dot.fill) + 0.05);
                            assert!(contrast >= 5.5, "{color:?} on {background:?}: {contrast}");
                        }
                    } else {
                        assert_eq!(dot.radius, radius);
                        assert_eq!(dot.fill, color);
                    }
                }
            }
        }
    }

    #[test]
    fn all_styles_keep_text_and_status_colors_readable() {
        for (mode, styles) in all_styles() {
            let colors = styles.palette(mode);
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
                    let (a, b) = (luminance(background), luminance(foreground));
                    let contrast = (a.max(b) + 0.05) / (a.min(b) + 0.05);
                    // Preserve the original Classic dark palette exactly, including its dim metadata.
                    let minimum = if mode == Theme::Dark
                        && styles.dark == DarkStyle::Neutral
                        && foreground == colors.dim
                    {
                        3.0
                    } else {
                        4.5
                    };
                    assert!(
                        contrast >= minimum,
                        "{styles:?} {foreground:?} on {background:?}: {contrast}"
                    );
                }
            }
        }
    }
}
