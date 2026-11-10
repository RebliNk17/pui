//! Explicit fallback chains: egui does not ask the OS to substitute missing glyphs.
use super::*;
use std::sync::OnceLock;

pub(super) fn definitions() -> FontDefinitions {
    static FONTS: OnceLock<FontDefinitions> = OnceLock::new();
    FONTS.get_or_init(build).clone()
}

fn add(fonts: &mut FontDefinitions, name: &str, data: FontData) {
    fonts.font_data.insert(name.into(), Arc::new(data));
}
fn file(fonts: &mut FontDefinitions, name: &str, path: &str) -> bool {
    let Ok(bytes) = std::fs::read(path) else {
        return false;
    };
    add(fonts, name, FontData::from_owned(bytes));
    true
}
fn first(fonts: &mut FontDefinitions, name: &str, paths: &[&str]) -> bool {
    paths.iter().any(|path| file(fonts, name, path))
}

fn build() -> FontDefinitions {
    let mut fonts = FontDefinitions::default();
    #[cfg(target_os = "macos")]
    for (name, path, family) in [
        (
            "system",
            "/System/Library/Fonts/SFNS.ttf",
            FontFamily::Proportional,
        ),
        (
            "system-mono",
            "/System/Library/Fonts/SFNSMono.ttf",
            FontFamily::Monospace,
        ),
    ] {
        if file(&mut fonts, name, path) {
            fonts
                .families
                .entry(family)
                .or_default()
                .insert(0, name.into());
        }
    }

    let mut fallback = Vec::new();
    #[cfg(target_os = "macos")]
    if file(
        &mut fonts,
        "system-hebrew",
        "/System/Library/Fonts/SFHebrew.ttf",
    ) {
        fallback.push("system-hebrew".into());
    }
    add(
        &mut fonts,
        "noto-hebrew",
        FontData::from_static(include_bytes!("../../assets/fonts/NotoSansHebrew.ttf")),
    );
    fallback.push("noto-hebrew".into());

    // Broad coverage without scanning directories, invoking font utilities or networking.
    // Missing optional files are normal on other OS versions/distributions.
    #[cfg(target_os = "macos")]
    for (name, path) in [
        (
            "unicode",
            "/System/Library/Fonts/Supplemental/Arial Unicode.ttf",
        ),
        ("arabic", "/System/Library/Fonts/SFArabic.ttf"),
        ("armenian", "/System/Library/Fonts/SFArmenian.ttf"),
        ("georgian", "/System/Library/Fonts/SFGeorgian.ttf"),
        ("cjk", "/System/Library/Fonts/ヒラギノ角ゴシック W3.ttc"),
        ("korean", "/System/Library/Fonts/AppleSDGothicNeo.ttc"),
        (
            "devanagari",
            "/System/Library/Fonts/Supplemental/Devanagari Sangam MN.ttc",
        ),
        ("thai", "/System/Library/Fonts/ThonburiUI.ttc"),
    ] {
        if file(&mut fonts, name, path) {
            fallback.push(name.into());
        }
    }
    #[cfg(target_os = "windows")]
    {
        let root = std::env::var_os("WINDIR")
            .map(std::path::PathBuf::from)
            .unwrap_or_else(|| "C:\\Windows".into())
            .join("Fonts");
        for (name, filename) in [
            ("unicode", "arial.ttf"),
            ("cjk", "msyh.ttc"),
            ("japanese", "meiryo.ttc"),
            ("korean", "malgun.ttf"),
            ("symbols", "seguisym.ttf"),
        ] {
            if file(&mut fonts, name, &root.join(filename).to_string_lossy()) {
                fallback.push(name.into());
            }
        }
    }
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    for (name, paths) in [
        (
            "unicode",
            &[
                "/usr/share/fonts/truetype/dejavu/DejaVuSans.ttf",
                "/usr/share/fonts/dejavu-sans-fonts/DejaVuSans.ttf",
            ][..],
        ),
        (
            "arabic",
            &[
                "/usr/share/fonts/truetype/noto/NotoSansArabic-Regular.ttf",
                "/usr/share/fonts/noto/NotoSansArabic-Regular.ttf",
            ][..],
        ),
        (
            "cjk",
            &[
                "/usr/share/fonts/opentype/noto/NotoSansCJK-Regular.ttc",
                "/usr/share/fonts/noto-cjk/NotoSansCJK-Regular.ttc",
            ][..],
        ),
        (
            "devanagari",
            &[
                "/usr/share/fonts/truetype/noto/NotoSansDevanagari-Regular.ttf",
                "/usr/share/fonts/noto/NotoSansDevanagari-Regular.ttf",
            ][..],
        ),
        (
            "thai",
            &[
                "/usr/share/fonts/truetype/noto/NotoSansThai-Regular.ttf",
                "/usr/share/fonts/noto/NotoSansThai-Regular.ttf",
            ][..],
        ),
    ] {
        if first(&mut fonts, name, paths) {
            fallback.push(name.into());
        }
    }
    for family in [FontFamily::Proportional, FontFamily::Monospace] {
        fonts
            .families
            .entry(family)
            .or_default()
            .extend(fallback.iter().cloned());
    }
    // Named fonts need the same fallback chain as body text, not a single Latin font.
    for (name, path) in [
        (
            "body-bold",
            "/System/Library/Fonts/Supplemental/Arial Bold.ttf",
        ),
        (
            "pi-serif",
            "/System/Library/Fonts/Supplemental/Georgia Bold Italic.ttf",
        ),
    ] {
        let mut chain = fonts.families[&FontFamily::Proportional].clone();
        if first(&mut fonts, name, &[path]) {
            chain.insert(0, name.into());
        }
        fonts.families.insert(FontFamily::Name(name.into()), chain);
    }
    fonts
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn embedded_hebrew_works_without_any_system_fonts() {
        let mut fonts = FontDefinitions::default();
        add(
            &mut fonts,
            "noto-hebrew",
            FontData::from_static(include_bytes!("../../assets/fonts/NotoSansHebrew.ttf")),
        );
        for chain in fonts.families.values_mut() {
            chain.push("noto-hebrew".into());
        }
        let ctx = egui::Context::default();
        ctx.set_fonts(fonts);
        let _ = ctx.run(egui::RawInput::default(), |ctx| {
            for family in [FontFamily::Proportional, FontFamily::Monospace] {
                ctx.fonts(|f| {
                    assert!(f.has_glyphs(&FontId::new(14.0, family), "שלום עולם ךםןףץ שָׁלוֹם"))
                });
            }
        });
    }
    #[test]
    fn mixed_hebrew_text_edit_preserves_unicode_input_and_glyphs() {
        let ctx = egui::Context::default();
        crate::theme::install(&ctx);
        for mode in [Theme::Dark, Theme::Light] {
            mode.apply(&ctx);
            let original = "שלום English 123 עולם";
            let mut value = original.to_owned();
            for index in 0..3 {
                let _ = ctx.run(
                    egui::RawInput {
                        events: if index == 2 {
                            vec![egui::Event::Text("!".into())]
                        } else {
                            vec![]
                        },
                        ..Default::default()
                    },
                    |ctx| {
                        egui::CentralPanel::default().show(ctx, |ui| {
                            let output = egui::TextEdit::multiline(&mut value)
                                .id_salt(mode == Theme::Light)
                                .font(FontId::proportional(14.0))
                                .show(ui);
                            output.response.request_focus();
                            assert_eq!(output.galley.job.text, value);
                            ctx.fonts(|f| {
                                assert!(f.has_glyphs(&FontId::proportional(14.0), &value))
                            });
                        });
                    },
                );
            }
            assert_eq!(
                value,
                format!("{original}!"),
                "Do not reverse or rewrite logical input order"
            );
        }
    }
    #[test]
    fn optional_missing_font_keeps_existing_definitions() {
        let mut fonts = FontDefinitions::default();
        let original = fonts.font_data.len();
        assert!(!file(&mut fonts, "absent", "/no-such-pui-font.ttf"));
        assert_eq!(fonts.font_data.len(), original);
    }
    #[test]
    fn fallback_definitions_are_shared_across_windows() {
        let first = definitions();
        let second = definitions();
        assert!(Arc::ptr_eq(
            &first.font_data["noto-hebrew"],
            &second.font_data["noto-hebrew"]
        ));
        for chain in first.families.values() {
            assert!(chain.iter().any(|n| n == "noto-hebrew"));
        }
    }
}
