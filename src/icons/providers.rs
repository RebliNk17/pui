//! Rasterized SVG brand logos for supported model providers, cached per theme.
use eframe::egui::{self, Color32, ColorImage, Rect, TextureHandle, TextureOptions, Ui, Vec2};
use resvg::{tiny_skia, usvg};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
enum Brand {
    Claude,
    Gemini,
    OpenAi,
    OpenRouter,
    DeepSeek,
    Xai,
    Custom,
}

impl Brand {
    fn from_provider(provider: &str) -> Self {
        let base = provider
            .split('~')
            .next()
            .unwrap_or(provider)
            .to_ascii_lowercase();
        if base.contains("anthropic") || base.contains("claude") {
            Self::Claude
        } else if base.contains("antigravity")
            || base.contains("google")
            || base.contains("gemini")
            || base.contains("vertex")
        {
            Self::Gemini
        } else if base.contains("openai")
            || base.contains("chatgpt")
            || base.contains("codex")
            || base.contains("gpt-")
            || base.starts_with("o1")
            || base.starts_with("o3")
            || base.starts_with("o4")
        {
            Self::OpenAi
        } else if base.contains("openrouter") {
            Self::OpenRouter
        } else if base.contains("deepseek") {
            Self::DeepSeek
        } else if base.contains("xai") || base.contains("grok") {
            Self::Xai
        } else {
            Self::Custom
        }
    }

    fn svg(self, dark: bool) -> &'static str {
        match self {
            // Official Claude 12-petal sunburst silhouette (#D97757)
            Self::Claude => {
                r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24">
<path fill="#D97757" d="M12 2.2c.55 0 1 .45 1 1v5.1l3.6-3.6a1 1 0 1 1 1.42 1.42L14.4 9.72l5.1-.02a1 1 0 1 1 0 2l-5.08.02 3.6 3.6a1 1 0 1 1-1.42 1.42L13 13.14v5.66a1 1 0 1 1-2 0v-5.66l-3.6 3.6a1 1 0 1 1-1.42-1.42l3.6-3.6-5.08-.02a1 1 0 1 1 0-2l5.1.02L5.98 6.12A1 1 0 1 1 7.4 4.7L11 8.3V3.2c0-.55.45-1 1-1Z"/>
<path fill="#D97757" d="M12 2.2c.4 0 .75.28.86.68l1.2 4.46 3.92-2.42a.9.9 0 0 1 1.24.3.9.9 0 0 1-.28 1.24L15 8.88l4.46 1.2a.9.9 0 0 1-.46 1.74L14.54 10.6l2.42 3.92a.9.9 0 0 1-1.54.94L13 11.54l-1.2 4.46a.9.9 0 0 1-1.74-.46l1.2-4.46-3.92 2.42a.9.9 0 0 1-.94-1.54l3.92-2.42-4.46-1.2a.9.9 0 0 1 .46-1.74l4.46 1.2L8.36 4.48a.9.9 0 0 1 1.54-.94l2.42 3.92 1.2-4.46a.9.9 0 0 1 .48-.8Z" opacity="0.9"/>
</svg>"##
            }
            // Official Google Gemini 4-point curved star with blue->violet gradient
            Self::Gemini => {
                r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24">
<defs>
  <linearGradient id="g" x1="0%" y1="0%" x2="100%" y2="100%">
    <stop offset="0%" stop-color="#4E8DF5"/>
    <stop offset="55%" stop-color="#9B72CB"/>
    <stop offset="100%" stop-color="#D96570"/>
  </linearGradient>
</defs>
<path fill="url(#g)" d="M12 1.5C12 7.3 16.7 12 22.5 12C16.7 12 12 16.7 12 22.5C12 16.7 7.3 12 1.5 12C7.3 12 12 7.3 12 1.5Z"/>
</svg>"##
            }
            // OpenAI 6-petal interlocking blossom
            Self::OpenAi => {
                if dark {
                    r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24">
<path fill="#FFFFFF" d="M22.28 9.82a5.98 5.98 0 0 0-.52-4.91 6.05 6.05 0 0 0-6.51-2.9A6.07 6.07 0 0 0 4.98 4.18a5.98 5.98 0 0 0-4 2.9 6.05 6.05 0 0 0 .74 7.1 5.98 5.98 0 0 0 .51 4.91 6.05 6.05 0 0 0 6.51 2.9A5.98 5.98 0 0 0 13.26 24a6.06 6.06 0 0 0 5.77-4.21 5.99 5.99 0 0 0 4-2.9 6.06 6.06 0 0 0-.75-7.07Zm-9.02 12.61a4.48 4.48 0 0 1-2.88-1.04l.14-.08 4.78-2.76a.8.8 0 0 0 .39-.68v-6.74l2.02 1.17a.07.07 0 0 1 .04.05v5.58a4.5 4.5 0 0 1-4.49 4.5Zm-9.66-4.13a4.47 4.47 0 0 1-.54-3.01l.14.08 4.78 2.76a.77.77 0 0 0 .78 0l5.84-3.37v2.33a.08.08 0 0 1-.03.06L9.74 19.95a4.5 4.5 0 0 1-6.14-1.65ZM2.34 7.9a4.49 4.49 0 0 1 2.37-1.97V11.6a.77.77 0 0 0 .39.68l5.81 3.35-2.02 1.17a.08.08 0 0 1-.07 0L4.04 14.04A4.5 4.5 0 0 1 2.34 7.9Zm16.6 3.86-5.84-3.37 2.02-1.16a.08.08 0 0 1 .07 0l4.78 2.76a4.5 4.5 0 0 1-.68 8.11v-5.67a.79.79 0 0 0-.35-.67Zm2.01-3.02-.14-.09-4.77-2.78a.78.78 0 0 0-.79 0L9.41 9.23V6.9a.07.07 0 0 1 .03-.06l4.78-2.76a4.5 4.5 0 0 1 6.73 4.66ZM8.31 12.86 6.29 11.7a.08.08 0 0 1-.04-.06V6.07a4.5 4.5 0 0 1 7.38-3.45l-.14.08-4.78 2.76a.8.8 0 0 0-.39.68Zm1.1-2.36 2.6-1.5 2.6 1.5v3l-2.6 1.5-2.6-1.5Z"/>
</svg>"##
                } else {
                    r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24">
<path fill="#111111" d="M22.28 9.82a5.98 5.98 0 0 0-.52-4.91 6.05 6.05 0 0 0-6.51-2.9A6.07 6.07 0 0 0 4.98 4.18a5.98 5.98 0 0 0-4 2.9 6.05 6.05 0 0 0 .74 7.1 5.98 5.98 0 0 0 .51 4.91 6.05 6.05 0 0 0 6.51 2.9A5.98 5.98 0 0 0 13.26 24a6.06 6.06 0 0 0 5.77-4.21 5.99 5.99 0 0 0 4-2.9 6.06 6.06 0 0 0-.75-7.07Zm-9.02 12.61a4.48 4.48 0 0 1-2.88-1.04l.14-.08 4.78-2.76a.8.8 0 0 0 .39-.68v-6.74l2.02 1.17a.07.07 0 0 1 .04.05v5.58a4.5 4.5 0 0 1-4.49 4.5Zm-9.66-4.13a4.47 4.47 0 0 1-.54-3.01l.14.08 4.78 2.76a.77.77 0 0 0 .78 0l5.84-3.37v2.33a.08.08 0 0 1-.03.06L9.74 19.95a4.5 4.5 0 0 1-6.14-1.65ZM2.34 7.9a4.49 4.49 0 0 1 2.37-1.97V11.6a.77.77 0 0 0 .39.68l5.81 3.35-2.02 1.17a.08.08 0 0 1-.07 0L4.04 14.04A4.5 4.5 0 0 1 2.34 7.9Zm16.6 3.86-5.84-3.37 2.02-1.16a.08.08 0 0 1 .07 0l4.78 2.76a4.5 4.5 0 0 1-.68 8.11v-5.67a.79.79 0 0 0-.35-.67Zm2.01-3.02-.14-.09-4.77-2.78a.78.78 0 0 0-.79 0L9.41 9.23V6.9a.07.07 0 0 1 .03-.06l4.78-2.76a4.5 4.5 0 0 1 6.73 4.66ZM8.31 12.86 6.29 11.7a.08.08 0 0 1-.04-.06V6.07a4.5 4.5 0 0 1 7.38-3.45l-.14.08-4.78 2.76a.8.8 0 0 0-.39.68Zm1.1-2.36 2.6-1.5 2.6 1.5v3l-2.6 1.5-2.6-1.5Z"/>
</svg>"##
                }
            }
            Self::OpenRouter => {
                r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24">
<path fill="none" stroke="#8B7CF6" stroke-width="2.2" stroke-linecap="round" stroke-linejoin="round" d="M3 12h4l4-6h9M17 3l3 3-3 3M7 12l4 6h9M17 15l3 3-3 3"/>
</svg>"##
            }
            Self::DeepSeek => {
                r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24">
<path fill="#4D6BFE" d="M19.5 11.5c0 4.2-3.6 7.5-8 7.5-3.4 0-6.3-1.9-7.5-4.7 1.3.4 2.7.2 3.7-.6-1.7-1.1-2.6-3-2.2-5 1.1.8 2.4 1.2 3.8 1.1.9-2.2 3.1-3.8 5.7-3.8 2.5 0 4.5 2.5 4.5 5.5Z"/>
<circle cx="14.5" cy="10.2" r="1.2" fill="#FFFFFF"/>
</svg>"##
            }
            Self::Xai => {
                if dark {
                    r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24">
<path fill="#FFFFFF" d="M4 4h3.4l5.2 6.8L17.4 4H20l-6.2 8.2L20.4 20H17l-5.6-7.3L6.2 20H3.6l6.5-8.6L4 4Z"/>
</svg>"##
                } else {
                    r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24">
<path fill="#111111" d="M4 4h3.4l5.2 6.8L17.4 4H20l-6.2 8.2L20.4 20H17l-5.6-7.3L6.2 20H3.6l6.5-8.6L4 4Z"/>
</svg>"##
                }
            }
            Self::Custom => "",
        }
    }
}

fn texture(ctx: &egui::Context, brand: Brand, dark: bool) -> Option<TextureHandle> {
    if brand == Brand::Custom {
        return None;
    }
    let id = egui::Id::new(("provider-brand-logo", brand, dark));
    if let Some(tex) = ctx.data(|d| d.get_temp::<TextureHandle>(id)) {
        return Some(tex);
    }
    let svg = brand.svg(dark);
    let tree = usvg::Tree::from_str(svg, &usvg::Options::default()).ok()?;
    let size = 64u32;
    let mut pixmap = tiny_skia::Pixmap::new(size, size)?;
    let tree_size = tree.size();
    let sx = size as f32 / tree_size.width().max(1.0);
    let sy = size as f32 / tree_size.height().max(1.0);
    resvg::render(
        &tree,
        tiny_skia::Transform::from_scale(sx, sy),
        &mut pixmap.as_mut(),
    );
    let mut rgba = Vec::with_capacity((size * size * 4) as usize);
    for px in pixmap.pixels() {
        let a = px.alpha();
        if a == 0 {
            rgba.extend_from_slice(&[0, 0, 0, 0]);
        } else {
            let r = ((px.red() as u16 * 255) / a as u16).min(255) as u8;
            let g = ((px.green() as u16 * 255) / a as u16).min(255) as u8;
            let b = ((px.blue() as u16 * 255) / a as u16).min(255) as u8;
            rgba.extend_from_slice(&[r, g, b, a]);
        }
    }
    let image = ColorImage::from_rgba_unmultiplied([size as usize, size as usize], &rgba);
    let tex = ctx.load_texture(
        format!("provider-logo-{brand:?}-{dark}"),
        image,
        TextureOptions::LINEAR,
    );
    ctx.data_mut(|d| d.insert_temp(id, tex.clone()));
    Some(tex)
}

pub fn has_provider_logo(provider_or_model: &str) -> bool {
    Brand::from_provider(provider_or_model) != Brand::Custom
}

pub fn draw_provider(ui: &mut Ui, rect: Rect, provider: &str) {
    let brand = Brand::from_provider(provider);
    if let Some(tex) = texture(ui.ctx(), brand, ui.visuals().dark_mode) {
        ui.painter().image(
            tex.id(),
            rect,
            Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)),
            Color32::WHITE,
        );
    } else {
        let colors = crate::theme::palette(ui.ctx());
        let scale = (rect.width().min(rect.height()) / 16.0).max(0.5);
        super::draw_scaled(ui, rect, super::Icon::Globe, colors.muted, scale);
    }
}

pub fn provider_inline(ui: &mut Ui, provider: &str) {
    let (rect, _) = ui.allocate_exact_size(Vec2::splat(16.0), egui::Sense::hover());
    draw_provider(ui, rect, provider);
}
