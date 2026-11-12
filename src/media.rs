//! Lazy, bounded attachment decoding. Only visible images create GPU textures.
use base64::{Engine as _, engine::general_purpose::STANDARD};
use eframe::egui::{self, ColorImage, TextureHandle, TextureOptions, Vec2};
use serde_json::Value;
use std::{
    collections::HashMap,
    hash::{Hash, Hasher},
    io::Cursor,
    sync::{Arc, mpsc},
    thread,
};

const MAX_ENCODED: usize = 32 * 1024 * 1024;
const MAX_PIXELS: u64 = 24_000_000;
const MAX_TEXTURES: usize = 32;

/// Non-image files are retained locally; Pi reads them through its normal tools.
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct FileAttachment {
    pub path: std::path::PathBuf,
    pub name: String,
    pub size: u64,
}

pub const FILE_MARKER: &str = "\n\nPui attached files (local paths; read with tools as needed):\n";

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct Attachment {
    pub key: u64,
    pub data: Arc<str>,
    pub mime: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub file: Option<FileAttachment>,
}
impl Attachment {
    pub fn from_file(file: FileAttachment) -> Self {
        let mut hasher = std::collections::hash_map::DefaultHasher::new();
        file.path.hash(&mut hasher);
        Self {
            key: hasher.finish(),
            data: Arc::from(""),
            mime: "application/octet-stream".into(),
            file: Some(file),
        }
    }
    pub fn from_bytes(bytes: &[u8]) -> Result<Self, String> {
        if bytes.len() > 8 * 1024 * 1024 {
            return Err("Image is larger than 8 MB".into());
        }
        let format = image::guess_format(bytes)
            .map_err(|_| "Unsupported image; choose PNG, JPEG, GIF or WebP")?;
        let mime = match format {
            image::ImageFormat::Png => "image/png",
            image::ImageFormat::Jpeg => "image/jpeg",
            image::ImageFormat::Gif => "image/gif",
            image::ImageFormat::WebP => "image/webp",
            _ => return Err("Unsupported image; choose PNG, JPEG, GIF or WebP".into()),
        };
        let dimensions = image::ImageReader::with_format(Cursor::new(bytes), format)
            .into_dimensions()
            .map_err(|_| "Cannot read image dimensions")?;
        if dimensions.0 == 0
            || dimensions.1 == 0
            || u64::from(dimensions.0) * u64::from(dimensions.1) > MAX_PIXELS
        {
            return Err("Image exceeds the 24 megapixel limit".into());
        }
        Self::from_json(
            &serde_json::json!({"type":"image","data":STANDARD.encode(bytes),"mimeType":mime}),
        )
        .ok_or("Cannot attach image".into())
    }
    pub fn to_json(&self) -> Value {
        serde_json::json!({"type":"image","data":self.data.as_ref(),"mimeType":self.mime})
    }
    pub fn from_json(block: &Value) -> Option<Self> {
        if block["type"] != "image" {
            return None;
        }
        let data = block["data"].as_str()?;
        let mut hasher = std::collections::hash_map::DefaultHasher::new();
        data.hash(&mut hasher);
        Some(Self {
            key: hasher.finish(),
            data: Arc::from(data),
            mime: block["mimeType"].as_str().unwrap_or("image/png").into(),
            file: None,
        })
    }
}

/// Only recognize a complete trailing manifest. No filesystem reads on history load.
pub fn file_manifest(text: &str) -> Option<(&str, Vec<FileAttachment>)> {
    let (body, manifest) = text.rsplit_once(FILE_MARKER)?;
    let files: Vec<FileAttachment> = serde_json::from_str(manifest).ok()?;
    if files.is_empty() || files.len() > 8 || files.iter().any(|f| !f.path.is_absolute()) {
        return None;
    }
    Some((body, files))
}

pub fn attachments(content: &Value) -> Vec<Attachment> {
    content
        .as_array()
        .map(|a| a.iter().filter_map(Attachment::from_json).collect())
        .unwrap_or_default()
}

struct Decoded {
    key: u64,
    large: bool,
    result: Result<ColorImage, String>,
}
enum Entry {
    Pending,
    Ready(TextureHandle),
    Failed(String),
}
struct Cached {
    entry: Entry,
    used: u64,
}

pub struct Gallery {
    cache: HashMap<(u64, bool), Cached>,
    tx: mpsc::Sender<Decoded>,
    rx: mpsc::Receiver<Decoded>,
    pub selected: Option<Attachment>,
    collection: Vec<Attachment>,
    index: usize,
    zoom: f32,
    pan: Vec2,
    tick: u64,
    pending: usize,
}
impl Default for Gallery {
    fn default() -> Self {
        let (tx, rx) = mpsc::channel();
        Self {
            cache: HashMap::new(),
            tx,
            rx,
            selected: None,
            collection: Vec::new(),
            index: 0,
            zoom: 1.0,
            pan: Vec2::ZERO,
            tick: 0,
            pending: 0,
        }
    }
}
impl Gallery {
    pub fn poll(&mut self, ctx: &egui::Context) -> bool {
        let mut changed = false;
        for decoded in self.rx.try_iter() {
            self.pending = self.pending.saturating_sub(1);
            // A viewer may close/navigate while its decode is still in flight.
            // Never upload an obsolete full-resolution result to the GPU.
            if decoded.large && self.selected.as_ref().map(|image| image.key) != Some(decoded.key) {
                self.cache.remove(&(decoded.key, true));
                continue;
            }
            let entry = match decoded.result {
                Ok(image) => Entry::Ready(ctx.load_texture(
                    format!("attachment-{}-{}", decoded.key, decoded.large),
                    image,
                    TextureOptions::LINEAR,
                )),
                Err(error) => Entry::Failed(error),
            };
            self.cache.insert(
                (decoded.key, decoded.large),
                Cached {
                    entry,
                    used: self.tick,
                },
            );
            changed = true;
        }
        changed
    }
    fn request(&mut self, ui: &egui::Ui, image: &Attachment, large: bool) {
        self.tick = self.tick.wrapping_add(1);
        let key = (image.key, large);
        if let Some(entry) = self.cache.get_mut(&key) {
            entry.used = self.tick;
            return;
        }
        if self.pending >= 2 {
            return;
        }
        if self.cache.len() >= MAX_TEXTURES {
            let oldest = self
                .cache
                .iter()
                .filter(|(_, v)| !matches!(v.entry, Entry::Pending))
                .min_by_key(|(_, v)| v.used)
                .map(|(key, _)| *key);
            if let Some(oldest) = oldest {
                self.cache.remove(&oldest);
            }
        }
        if large {
            self.cache
                .retain(|(_, is_large), entry| !*is_large || matches!(entry.entry, Entry::Pending));
        }
        self.cache.insert(
            key,
            Cached {
                entry: Entry::Pending,
                used: self.tick,
            },
        );
        self.pending += 1;
        let attachment = image.clone();
        let tx = self.tx.clone();
        let ctx = ui.ctx().clone();
        // Keep the source resolution in the viewer, limited only by the GPU.
        let side = if large {
            ui.input(|i| i.max_texture_side).min(16000) as u32
        } else {
            480
        };
        thread::spawn(move || {
            let result = decode(&attachment, side);
            let _ = tx.send(Decoded {
                key: attachment.key,
                large,
                result,
            });
            ctx.request_repaint();
        });
    }
    pub fn open(&mut self, images: &[Attachment], index: usize) {
        if images.is_empty() {
            return;
        }
        let selected = &images[index.min(images.len() - 1)];
        if selected.file.is_some() {
            return;
        }
        let selected_key = selected.key;
        self.collection = images
            .iter()
            .filter(|i| i.file.is_none())
            .cloned()
            .collect();
        if self.collection.is_empty() {
            return;
        }
        self.index = self
            .collection
            .iter()
            .position(|i| i.key == selected_key)
            .unwrap_or(0);
        self.selected = Some(self.collection[self.index].clone());
        self.zoom = 1.0;
        self.pan = Vec2::ZERO;
    }
    fn navigate(&mut self, index: usize) {
        if index < self.collection.len() {
            self.index = index;
            self.selected = Some(self.collection[index].clone());
            self.zoom = 1.0;
            self.pan = Vec2::ZERO;
        }
    }
    pub fn thumbnail(
        &mut self,
        ui: &mut egui::Ui,
        image: &Attachment,
        size: Vec2,
    ) -> egui::Response {
        if let Some(file) = &image.file {
            let colors = crate::theme::palette(ui.ctx());
            let (slot, response) = ui.allocate_exact_size(size, egui::Sense::hover());
            ui.painter().rect_filled(slot, 8, colors.surface);
            let mut job = egui::text::LayoutJob::simple(
                format!("{}\n{} bytes", file.name, file.size),
                egui::FontId::proportional(10.0),
                colors.text,
                (size.x - 10.0).max(1.0),
            );
            job.wrap.max_rows = 3;
            let galley = ui.painter().layout_job(job);
            ui.painter()
                .galley(slot.center() - galley.size() / 2.0, galley, colors.text);
            return response.on_hover_text(format!(
                "{}\n{} bytes\n{}",
                file.name,
                file.size,
                file.path.display()
            ));
        }
        let colors = crate::theme::palette(ui.ctx());
        let (slot, response) = ui.allocate_exact_size(size, egui::Sense::click());
        if !ui.is_rect_visible(slot) {
            return response;
        }
        self.request(ui, image, false);
        ui.painter().rect_filled(slot, 8, colors.surface);
        match self.cache.get(&(image.key, false)).map(|c| &c.entry) {
            Some(Entry::Ready(texture)) => {
                let size = fit(texture.size_vec2(), size);
                egui::Image::new((texture.id(), size))
                    .corner_radius(8)
                    .paint_at(ui, egui::Rect::from_center_size(slot.center(), size));
            }
            entry => {
                let (label, color) = match entry {
                    Some(Entry::Failed(_)) => ("Unavailable", colors.red),
                    _ => ("Loading…", colors.muted),
                };
                ui.painter().text(
                    slot.center(),
                    egui::Align2::CENTER_CENTER,
                    label,
                    egui::FontId::proportional(10.0),
                    color,
                );
            }
        }
        response
            .on_hover_cursor(egui::CursorIcon::ZoomIn)
            .on_hover_text("Open image")
    }
    pub fn thumbnails(&mut self, ui: &mut egui::Ui, images: &[Attachment]) {
        self.strip(ui, images, Vec2::new(112.0, 76.0));
    }
    pub fn user_thumbnails(&mut self, ui: &mut egui::Ui, images: &[Attachment]) {
        self.strip(ui, images, Vec2::new(88.0, 64.0));
    }
    fn strip(&mut self, ui: &mut egui::Ui, images: &[Attachment], size: Vec2) {
        if images.is_empty() {
            return;
        }
        ui.horizontal_wrapped(|ui| {
            ui.spacing_mut().item_spacing = Vec2::splat(8.0);
            for (index, image) in images.iter().enumerate() {
                if self.thumbnail(ui, image, size).clicked() {
                    self.open(images, index);
                }
            }
        });
    }
    pub fn is_open(&self) -> bool {
        self.selected.is_some()
    }
    pub fn close(&mut self) {
        self.selected = None;
        self.collection.clear();
        self.cache.retain(|(_, large), _| !*large);
        // Late full-resolution results are discarded by poll before upload.
    }
    /// Release disposable GPU/decoded caches when this chat leaves the screen.
    /// A fresh channel prevents late workers from resurrecting parked textures.
    pub fn release_textures(&mut self) {
        *self = Self::default();
    }
    pub fn viewer(&mut self, ctx: &egui::Context) {
        let colors = crate::theme::styles(ctx).palette(crate::theme::Theme::Dark);
        let Some(image) = self.selected.clone() else {
            return;
        };
        self.poll(ctx);
        if self.zoom == 0.0 {
            self.zoom = 1.0;
        }
        let count = self.collection.len().max(1);
        let mut next = None;
        let mut close = false;
        ctx.input_mut(|i| {
            close = i.consume_key(egui::Modifiers::NONE, egui::Key::Escape);
            if i.consume_key(egui::Modifiers::NONE, egui::Key::ArrowLeft) {
                next = Some(self.index.saturating_sub(1));
            }
            if i.consume_key(egui::Modifiers::NONE, egui::Key::ArrowRight) {
                next = Some((self.index + 1).min(count - 1));
            }
            if i.consume_key(egui::Modifiers::NONE, egui::Key::Home) {
                next = Some(0);
            }
            if i.consume_key(egui::Modifiers::NONE, egui::Key::End) {
                next = Some(count - 1);
            }
        });
        let screen = ctx.screen_rect();
        ctx.memory_mut(|m| {
            m.set_modal_layer(egui::LayerId::new(
                egui::Order::Foreground,
                egui::Id::new("image-theater"),
            ))
        });
        egui::Area::new(egui::Id::new("image-theater"))
            .order(egui::Order::Foreground)
            .fixed_pos(screen.min)
            .sense(egui::Sense::click_and_drag())
            .show(ctx, |ui| {
                ui.set_style(ctx.style_of(egui::Theme::Dark));
                ui.set_min_size(screen.size());
                ui.set_max_size(screen.size());
                ui.painter()
                    .rect_filled(screen, 0, egui::Color32::from_black_alpha(245));
                let backdrop = ui.interact(screen, ui.id().with("backdrop"), egui::Sense::click());
                let toolbar = egui::Rect::from_min_size(
                    screen.min + egui::vec2(18.0, 12.0),
                    Vec2::new(screen.width() - 36.0, 36.0),
                );
                ui.scope_builder(
                    egui::UiBuilder::new()
                        .max_rect(toolbar)
                        .layout(egui::Layout::left_to_right(egui::Align::Center)),
                    |ui| {
                        ui.label(
                            egui::RichText::new(format!("{} of {}", self.index + 1, count))
                                .color(egui::Color32::WHITE),
                        );
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            if crate::icons::button(
                                ui,
                                crate::icons::Icon::Close,
                                "Close image · Esc",
                            )
                            .clicked()
                            {
                                close = true;
                            }
                        });
                    },
                );
                let stage = egui::Rect::from_min_max(
                    screen.min + egui::vec2(64.0, 64.0),
                    screen.max - egui::vec2(64.0, if count > 1 { 110.0 } else { 28.0 }),
                );
                self.request(ui, &image, true);
                match self.cache.get(&(image.key, true)).map(|c| &c.entry) {
                    Some(Entry::Ready(texture)) => {
                        // Texture dimensions are physical pixels; egui uses logical points.
                        let native = texture.size_vec2() / ctx.pixels_per_point();
                        let fitted = fit(native, stage.size());
                        let size = fitted * self.zoom;
                        self.pan = clamp_pan(self.pan, size, stage.size());
                        let rect = egui::Rect::from_center_size(stage.center() + self.pan, size);
                        let can_pan = size.x > stage.width() || size.y > stage.height();
                        let response = ui.interact(
                            rect.intersect(stage),
                            ui.id().with("image"),
                            if can_pan {
                                egui::Sense::click_and_drag()
                            } else {
                                egui::Sense::click()
                            },
                        );
                        if can_pan && response.dragged() {
                            self.pan += ctx.input(|i| i.pointer.delta());
                        }
                        let old_zoom = self.zoom;
                        if response.clicked() {
                            self.zoom = if self.zoom > 1.0 {
                                1.0
                            } else {
                                (native.x / fitted.x).clamp(2.0, 8.0)
                            };
                        } else if response.hovered() {
                            ctx.input_mut(|i| {
                                let scroll = i.smooth_scroll_delta.y;
                                i.smooth_scroll_delta = Vec2::ZERO;
                                self.zoom = (self.zoom * (scroll * 0.003).exp() * i.zoom_delta())
                                    .clamp(1.0, 8.0);
                            });
                        }
                        if self.zoom != old_zoom {
                            ctx.request_repaint();
                            // Keep the detail under the pointer in place when zooming.
                            let anchor = response.hover_pos().unwrap_or(stage.center());
                            let ratio = self.zoom / old_zoom;
                            self.pan = self.pan * ratio + (stage.center() - anchor) * (ratio - 1.0);
                        }
                        let size = fitted * self.zoom;
                        self.pan = clamp_pan(self.pan, size, stage.size());
                        let rect = egui::Rect::from_center_size(stage.center() + self.pan, size);
                        response
                            .on_hover_cursor(
                                if can_pan && ctx.input(|i| i.pointer.is_decidedly_dragging()) {
                                    egui::CursorIcon::Grabbing
                                } else if self.zoom > 1.0 {
                                    egui::CursorIcon::ZoomOut
                                } else {
                                    egui::CursorIcon::ZoomIn
                                },
                            )
                            .on_hover_text(if can_pan {
                                "Click to fit · Scroll to zoom · Drag to pan"
                            } else if self.zoom > 1.0 {
                                "Click to fit · Scroll to zoom"
                            } else {
                                "Click to zoom in · Scroll to zoom"
                            });
                        let mut stage_ui = ui.new_child(
                            egui::UiBuilder::new()
                                .id_salt("image-stage-paint")
                                .max_rect(stage),
                        );
                        stage_ui.set_clip_rect(stage);
                        egui::Image::new((texture.id(), size)).paint_at(&stage_ui, rect);
                    }
                    entry => {
                        let label = match entry {
                            Some(Entry::Failed(error)) => error.as_str(),
                            _ => "Loading image…",
                        };
                        ui.painter().text(
                            stage.center(),
                            egui::Align2::CENTER_CENTER,
                            label,
                            egui::FontId::proportional(14.0),
                            colors.muted,
                        );
                    }
                }
                if count > 1 {
                    for (right, label, index) in [
                        (false, "‹", self.index.saturating_sub(1)),
                        (true, "›", (self.index + 1).min(count - 1)),
                    ] {
                        let center = egui::pos2(
                            if right {
                                screen.right() - 30.0
                            } else {
                                screen.left() + 30.0
                            },
                            stage.center().y,
                        );
                        let rect = egui::Rect::from_center_size(center, Vec2::new(40.0, 60.0));
                        if ui
                            .put(
                                rect,
                                egui::Button::new(egui::RichText::new(label).size(32.0)),
                            )
                            .clicked()
                        {
                            next = Some(index);
                        }
                    }
                    let strip = egui::Rect::from_min_size(
                        egui::pos2(screen.left() + 64.0, screen.bottom() - 94.0),
                        Vec2::new(screen.width() - 128.0, 82.0),
                    );
                    let images = self.collection.clone();
                    ui.scope_builder(egui::UiBuilder::new().max_rect(strip), |ui| {
                        egui::ScrollArea::horizontal()
                            .id_salt("theater-filmstrip")
                            .show(ui, |ui| {
                                ui.horizontal(|ui| {
                                    for (index, image) in images.iter().enumerate() {
                                        let response =
                                            self.thumbnail(ui, image, Vec2::new(80.0, 60.0));
                                        if index == self.index {
                                            if ui.ctx().data(|d| {
                                                d.get_temp::<usize>(egui::Id::new(
                                                    "theater-current-index",
                                                ))
                                            }) != Some(index)
                                            {
                                                response.scroll_to_me(Some(egui::Align::Center));
                                                ui.ctx().data_mut(|d| {
                                                    d.insert_temp(
                                                        egui::Id::new("theater-current-index"),
                                                        index,
                                                    )
                                                });
                                            }
                                            ui.painter().rect_stroke(
                                                response.rect,
                                                8,
                                                egui::Stroke::new(2.0, colors.accent),
                                                egui::StrokeKind::Inside,
                                            );
                                        }
                                        if response.clicked() {
                                            next = Some(index);
                                        }
                                    }
                                });
                            });
                    });
                }
                close |= backdrop.clicked();
            });
        if let Some(index) = next {
            self.navigate(index);
            ctx.request_repaint();
        }
        if close {
            self.close();
        }
    }
}
fn fit(size: Vec2, max: Vec2) -> Vec2 {
    size * (max.x / size.x).min(max.y / size.y).min(1.0)
}
fn clamp_pan(pan: Vec2, size: Vec2, viewport: Vec2) -> Vec2 {
    let limit = ((size - viewport) * 0.5).max(Vec2::ZERO);
    pan.clamp(-limit, limit)
}
fn decode(image: &Attachment, side: u32) -> Result<ColorImage, String> {
    if !matches!(
        image.mime.as_str(),
        "image/png" | "image/jpeg" | "image/jpg" | "image/gif" | "image/webp"
    ) {
        return Err("Unsupported image format".into());
    }
    if image.data.len() > MAX_ENCODED {
        return Err("Image exceeds the 24 MiB attachment limit".into());
    }
    let bytes = STANDARD
        .decode(image.data.as_bytes())
        .map_err(|_| "Invalid image data")?;
    let dimensions = image::ImageReader::new(Cursor::new(&bytes))
        .with_guessed_format()
        .map_err(|_| "Invalid image format")?
        .into_dimensions()
        .map_err(|_| "Cannot read image dimensions")?;
    if dimensions.0 == 0
        || dimensions.1 == 0
        || u64::from(dimensions.0) * u64::from(dimensions.1) > MAX_PIXELS
    {
        return Err("Image dimensions exceed the display limit".into());
    }
    let mut reader = image::ImageReader::new(Cursor::new(bytes))
        .with_guessed_format()
        .map_err(|_| "Invalid image format")?;
    let mut limits = image::Limits::default();
    limits.max_alloc = Some(128 * 1024 * 1024);
    limits.max_image_width = Some(16000);
    limits.max_image_height = Some(16000);
    reader.limits(limits);
    let decoded = reader.decode().map_err(|_| "Could not decode image")?;
    let decoded = if dimensions.0 > side || dimensions.1 > side {
        decoded.resize(side, side, image::imageops::FilterType::Lanczos3)
    } else {
        decoded
    }
    .into_rgba8();
    Ok(ColorImage::from_rgba_unmultiplied(
        [decoded.width() as usize, decoded.height() as usize],
        decoded.as_raw(),
    ))
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn real_png_decodes_and_scales() {
        let data = STANDARD.encode(include_bytes!("../assets/icon.png"));
        let attachment = Attachment::from_json(
            &serde_json::json!({"type":"image","data":data,"mimeType":"image/png"}),
        )
        .unwrap();
        let image = decode(&attachment, 128).unwrap();
        assert_eq!(image.size, [128, 128]);
    }
    #[test]
    fn viewer_decode_preserves_source_pixels_and_respects_gpu_limit() {
        let source = image::RgbaImage::from_fn(2400, 24, |x, y| {
            image::Rgba([(x % 256) as u8, (y % 256) as u8, (x % 2 * 255) as u8, 255])
        });
        let mut bytes = Cursor::new(Vec::new());
        source
            .write_to(&mut bytes, image::ImageFormat::Png)
            .unwrap();
        let attachment = Attachment::from_bytes(bytes.get_ref()).unwrap();
        let full = decode(&attachment, 4096).unwrap();
        assert_eq!(full.size, [2400, 24]);
        for x in [0, 1, 1599, 2399] {
            assert_eq!(
                full.pixels[x],
                egui::Color32::from_rgba_unmultiplied(
                    source.get_pixel(x as u32, 0).0[0],
                    0,
                    source.get_pixel(x as u32, 0).0[2],
                    255
                )
            );
        }
        assert_eq!(decode(&attachment, 800).unwrap().size, [800, 8]);
        assert_eq!(decode(&attachment, 480).unwrap().size, [480, 5]);
    }
    #[test]
    fn pan_is_clamped_independently_on_each_overflowing_axis() {
        let viewport = egui::vec2(1000.0, 700.0);
        let pan = egui::vec2(900.0, -900.0);
        assert_eq!(clamp_pan(pan, viewport, viewport), Vec2::ZERO);
        assert_eq!(clamp_pan(pan, viewport * 0.5, viewport), Vec2::ZERO);
        assert_eq!(
            clamp_pan(pan, egui::vec2(1400.0, 500.0), viewport),
            egui::vec2(200.0, 0.0)
        );
        assert_eq!(
            clamp_pan(pan, egui::vec2(800.0, 1100.0), viewport),
            egui::vec2(0.0, -200.0)
        );
    }

    struct ViewerFixture {
        ctx: egui::Context,
        gallery: Gallery,
        time: f64,
    }
    impl ViewerFixture {
        fn new(size: [usize; 2], pixels_per_point: f32, count: usize) -> Self {
            let ctx = egui::Context::default();
            crate::theme::install(&ctx);
            ctx.set_pixels_per_point(pixels_per_point);
            let mut gallery = Gallery::default();
            let images: Vec<_> = (1..=count)
                .map(|key| Attachment {
                    key: key as u64,
                    data: "unused fixture".into(),
                    mime: "image/png".into(),
                    file: None,
                })
                .collect();
            for image in &images {
                for large in [false, true] {
                    gallery.cache.insert(
                        (image.key, large),
                        Cached {
                            entry: Entry::Ready(ctx.load_texture(
                                format!("fixture-{}-{large}", image.key),
                                ColorImage::filled(
                                    if large { size } else { [8, 8] },
                                    egui::Color32::WHITE,
                                ),
                                TextureOptions::LINEAR,
                            )),
                            used: 0,
                        },
                    );
                }
            }
            gallery.open(&images, 0);
            let mut fixture = Self {
                ctx,
                gallery,
                time: 0.0,
            };
            fixture.frame(vec![]);
            fixture.frame(vec![]);
            fixture
        }
        fn frame(&mut self, events: Vec<egui::Event>) {
            self.time += 0.1;
            let _ = self.ctx.run(
                egui::RawInput {
                    screen_rect: Some(egui::Rect::from_min_size(
                        egui::Pos2::ZERO,
                        egui::vec2(1200.0, 800.0),
                    )),
                    time: Some(self.time),
                    events,
                    ..Default::default()
                },
                |ctx| self.gallery.viewer(ctx),
            );
        }
        fn pointer(&mut self, pos: egui::Pos2, pressed: bool) {
            self.frame(vec![
                egui::Event::PointerMoved(pos),
                egui::Event::PointerButton {
                    pos,
                    button: egui::PointerButton::Primary,
                    pressed,
                    modifiers: egui::Modifiers::NONE,
                },
            ]);
        }
        fn click(&mut self, pos: egui::Pos2) {
            self.pointer(pos, true);
            self.pointer(pos, false);
            // egui hit-tests against the preceding frame's widget geometry.
            self.frame(vec![]);
        }
        fn drag(&mut self, from: egui::Pos2, to: egui::Pos2) {
            self.pointer(from, true);
            self.frame(vec![egui::Event::PointerMoved(to)]);
            self.pointer(to, false);
        }
    }

    #[test]
    fn image_click_toggles_zoom_without_closing_and_drag_does_not_click() {
        let mut fixture = ViewerFixture::new([800, 400], 1.0, 1);
        let center = egui::pos2(600.0, 418.0);
        fixture.drag(center, center + egui::vec2(50.0, 50.0));
        assert_eq!(fixture.gallery.pan, Vec2::ZERO);
        assert_eq!(fixture.gallery.zoom, 1.0);
        fixture.click(center);
        assert_eq!(fixture.gallery.zoom, 2.0);
        assert!(fixture.gallery.is_open());
        fixture.drag(center, center + egui::vec2(100.0, 150.0));
        assert_eq!(fixture.gallery.pan, egui::vec2(100.0, 46.0));
        assert_eq!(fixture.gallery.zoom, 2.0);
        assert!(fixture.gallery.is_open());
        fixture.click(center);
        assert_eq!(fixture.gallery.zoom, 1.0);
        assert_eq!(fixture.gallery.pan, Vec2::ZERO);
        assert!(fixture.gallery.is_open());
    }
    #[test]
    fn small_zoomed_images_cannot_pan() {
        let mut fixture = ViewerFixture::new([160, 80], 1.0, 1);
        let center = egui::pos2(600.0, 418.0);
        fixture.click(center);
        fixture.drag(center, center + egui::vec2(50.0, 40.0));
        assert_eq!(fixture.gallery.zoom, 2.0);
        assert_eq!(fixture.gallery.pan, Vec2::ZERO);
        assert!(fixture.gallery.is_open());
    }
    #[test]
    fn wheel_and_pinch_zoom_anchor_to_pointer_and_cannot_shrink_below_fit() {
        let mut fixture = ViewerFixture::new([1600, 800], 1.0, 1);
        let pointer = egui::pos2(800.0, 418.0);
        fixture.frame(vec![
            egui::Event::PointerMoved(pointer),
            egui::Event::Zoom(2.0),
        ]);
        assert_eq!(fixture.gallery.zoom, 2.0);
        assert_eq!(fixture.gallery.pan, egui::vec2(-200.0, 0.0));
        fixture.frame(vec![egui::Event::MouseWheel {
            unit: egui::MouseWheelUnit::Point,
            delta: egui::vec2(0.0, 40.0),
            modifiers: egui::Modifiers::NONE,
        }]);
        assert!(fixture.gallery.zoom > 2.0);
        fixture.frame(vec![egui::Event::Zoom(0.01)]);
        assert_eq!(fixture.gallery.zoom, 1.0);
        assert_eq!(fixture.gallery.pan, Vec2::ZERO);
        assert!(fixture.gallery.is_open());
    }
    #[test]
    fn dragging_outside_image_does_not_dismiss_viewer() {
        let mut fixture = ViewerFixture::new([800, 400], 1.0, 1);
        fixture.click(egui::pos2(600.0, 418.0));
        fixture.drag(egui::pos2(600.0, 418.0), egui::pos2(20.0, 200.0));
        assert_eq!(fixture.gallery.zoom, 2.0);
        assert!(fixture.gallery.is_open());
    }
    #[test]
    fn retina_images_are_sized_in_physical_pixels() {
        let mut fixture = ViewerFixture::new([800, 400], 2.0, 1);
        // At 2x this image is 400 points wide, not 800.
        fixture.click(egui::pos2(850.0, 418.0));
        assert!(!fixture.gallery.is_open());
    }
    #[test]
    fn backdrop_closes_both_inside_and_outside_stage() {
        for pos in [egui::pos2(100.0, 418.0), egui::pos2(20.0, 200.0)] {
            let mut fixture = ViewerFixture::new([800, 400], 1.0, 1);
            fixture.click(pos);
            assert!(
                !fixture.gallery.is_open(),
                "Backdrop click at {pos:?} must close"
            );
        }
    }
    #[test]
    fn viewer_controls_navigate_without_closing_and_close_icon_works() {
        let mut fixture = ViewerFixture::new([800, 400], 1.0, 3);
        fixture.click(egui::pos2(1170.0, 377.0));
        assert_eq!(fixture.gallery.index, 1);
        assert!(fixture.gallery.is_open());
        fixture.click(egui::pos2(100.0, 735.0));
        assert_eq!(fixture.gallery.index, 0);
        assert!(fixture.gallery.is_open());
        fixture.click(egui::pos2(1168.0, 30.0));
        assert!(!fixture.gallery.is_open());
    }

    #[test]
    fn closing_viewer_releases_large_texture_and_discards_late_decode() {
        let mut fixture = ViewerFixture::new([800, 400], 1.0, 1);
        let key = fixture.gallery.selected.as_ref().unwrap().key;
        fixture.gallery.close();
        assert!(!fixture.gallery.cache.contains_key(&(key, true)));
        assert!(fixture.gallery.cache.contains_key(&(key, false)));
        fixture.gallery.pending = 1;
        fixture
            .gallery
            .tx
            .send(Decoded {
                key,
                large: true,
                result: Ok(ColorImage::filled([4, 4], egui::Color32::WHITE)),
            })
            .unwrap();
        fixture.gallery.poll(&fixture.ctx);
        assert_eq!(fixture.gallery.pending, 0);
        assert!(!fixture.gallery.cache.contains_key(&(key, true)));
    }
    #[test]
    fn parked_gallery_disconnects_decoders_and_releases_all_textures() {
        let mut fixture = ViewerFixture::new([800, 400], 1.0, 1);
        let sender = fixture.gallery.tx.clone();
        fixture.gallery.release_textures();
        assert!(fixture.gallery.cache.is_empty());
        assert!(!fixture.gallery.is_open());
        assert!(
            sender
                .send(Decoded {
                    key: 1,
                    large: false,
                    result: Ok(ColorImage::filled([4, 4], egui::Color32::WHITE)),
                })
                .is_err()
        );
    }
    #[test]
    fn visible_attachment_creates_texture_and_opens_viewer() {
        let ctx = egui::Context::default();
        crate::theme::install(&ctx);
        let mut gallery = Gallery::default();
        let attachment=Attachment::from_json(&serde_json::json!({"type":"image","data":STANDARD.encode(include_bytes!("../assets/icon.png")),"mimeType":"image/png"})).unwrap();
        gallery.cache.insert(
            (attachment.key, false),
            Cached {
                entry: Entry::Ready(ctx.load_texture(
                    "fixture",
                    ColorImage::filled([32, 32], egui::Color32::WHITE),
                    TextureOptions::LINEAR,
                )),
                used: 0,
            },
        );
        let pos = egui::pos2(20.0, 20.0);
        for events in [
            vec![],
            vec![
                egui::Event::PointerMoved(pos),
                egui::Event::PointerButton {
                    pos,
                    button: egui::PointerButton::Primary,
                    pressed: true,
                    modifiers: egui::Modifiers::NONE,
                },
            ],
            vec![egui::Event::PointerButton {
                pos,
                button: egui::PointerButton::Primary,
                pressed: false,
                modifiers: egui::Modifiers::NONE,
            }],
        ] {
            let _ = ctx.run(
                egui::RawInput {
                    events,
                    ..Default::default()
                },
                |ctx| {
                    egui::CentralPanel::default().show(ctx, |ui| {
                        gallery.thumbnails(ui, std::slice::from_ref(&attachment))
                    });
                },
            );
        }
        assert_eq!(
            gallery.selected.as_ref().map(|a| a.key),
            Some(attachment.key)
        );
    }
    #[test]
    fn thumbnail_geometry_is_identical_while_pending_ready_or_evicted() {
        let ctx = egui::Context::default();
        crate::theme::install(&ctx);
        let mut gallery = Gallery::default();
        let image = Attachment {
            key: 1,
            data: "invalid".into(),
            mime: "image/png".into(),
            file: None,
        };
        let mut heights = Vec::new();
        for state in 0..3 {
            let entry = match state {
                0 => Entry::Pending,
                1 => Entry::Ready(ctx.load_texture(
                    "wide",
                    ColorImage::filled([400, 50], egui::Color32::WHITE),
                    TextureOptions::LINEAR,
                )),
                _ => Entry::Failed("Failed image".into()),
            };
            gallery.cache.insert((1, false), Cached { entry, used: 0 });
            let _ = ctx.run(egui::RawInput::default(), |ctx| {
                egui::CentralPanel::default().show(ctx, |ui| {
                    let top = ui.cursor().top();
                    gallery.thumbnails(ui, std::slice::from_ref(&image));
                    heights.push(ui.cursor().top() - top);
                });
            });
        }
        assert!(
            heights.windows(2).all(|h| h[0] == h[1]),
            "Thumbnail changed layout height: {heights:?}"
        );
    }
    #[test]
    fn offscreen_attachment_never_starts_a_decoder() {
        let ctx = egui::Context::default();
        crate::theme::install(&ctx);
        let mut gallery = Gallery::default();
        let image = Attachment {
            key: 1,
            data: "invalid".into(),
            mime: "image/png".into(),
            file: None,
        };
        let _ = ctx.run(egui::RawInput::default(), |ctx| {
            egui::CentralPanel::default().show(ctx, |ui| {
                ui.add_space(10000.0);
                gallery.thumbnails(ui, std::slice::from_ref(&image));
            });
        });
        assert_eq!(gallery.pending, 0);
        assert!(gallery.cache.is_empty());
    }
    #[test]
    fn theater_keyboard_navigation_zoom_and_escape() {
        let ctx = egui::Context::default();
        crate::theme::install(&ctx);
        let mut gallery = Gallery::default();
        let images: Vec<_> = (1..=3)
            .map(|key| Attachment {
                key,
                data: "invalid".into(),
                mime: "image/png".into(),
                file: None,
            })
            .collect();
        gallery.open(&images, 0);
        let key = |key| egui::Event::Key {
            key,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers: egui::Modifiers::NONE,
        };
        for (events, index) in [
            (vec![], 0),
            (vec![key(egui::Key::ArrowRight)], 1),
            (vec![key(egui::Key::End)], 2),
            (vec![key(egui::Key::Home)], 0),
        ] {
            let _ = ctx.run(
                egui::RawInput {
                    screen_rect: Some(egui::Rect::from_min_size(
                        egui::Pos2::ZERO,
                        egui::vec2(1200.0, 800.0),
                    )),
                    events,
                    ..Default::default()
                },
                |ctx| gallery.viewer(ctx),
            );
            assert_eq!(gallery.index, index);
        }
        gallery.zoom = 3.0;
        gallery.navigate(1);
        assert_eq!(gallery.zoom, 1.0);
        let _ = ctx.run(
            egui::RawInput {
                events: vec![key(egui::Key::Escape)],
                ..Default::default()
            },
            |ctx| gallery.viewer(ctx),
        );
        assert!(!gallery.is_open());
    }
    #[test]
    fn file_cards_have_fixed_geometry_and_never_start_image_decoders() {
        let ctx = egui::Context::default();
        crate::theme::install(&ctx);
        let mut gallery = Gallery::default();
        let file = Attachment::from_file(FileAttachment {
            path: "/fixture/report.csv".into(),
            name: "very-long-report-name.csv".repeat(8),
            size: 123,
        });
        let mut rect = None;
        let _ = ctx.run(egui::RawInput::default(), |ctx| {
            egui::CentralPanel::default().show(ctx, |ui| {
                rect = Some(gallery.thumbnail(ui, &file, Vec2::new(65.0, 55.0)).rect);
            });
        });
        assert_eq!(rect.unwrap().size(), Vec2::new(65.0, 55.0));
        assert_eq!(gallery.pending, 0);
        gallery.open(std::slice::from_ref(&file), 0);
        assert!(!gallery.is_open());
        assert!(file_manifest(&format!("text{FILE_MARKER}[]")).is_none());
        assert!(
            file_manifest(&format!(
                "text{FILE_MARKER}[{{\"path\":\"relative.csv\",\"name\":\"x\",\"size\":1}}]"
            ))
            .is_none()
        );
    }
    #[test]
    fn invalid_image_is_a_visible_error() {
        assert!(
            decode(
                &Attachment {
                    key: 0,
                    data: "not base64".into(),
                    mime: "image/png".into(),
                    file: None,
                },
                128
            )
            .is_err()
        );
    }
}
