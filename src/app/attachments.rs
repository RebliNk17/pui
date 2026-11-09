//! Composer attachments are ephemeral; bytes are loaded only after attach/drop/paste.
use super::*;
use crate::media::{Attachment, Gallery};
use std::{
    fs::File,
    io::Read,
    path::Path,
    sync::atomic::{AtomicBool, Ordering},
};

const MAX_IMAGES: usize = 8;
const MAX_BYTES: u64 = 8 * 1024 * 1024;
pub(super) struct Item {
    id: u64,
    name: String,
    pub image: Option<Attachment>,
    error: Option<String>,
}
pub(super) struct DraftImages {
    items: Vec<Item>,
    next: u64,
    tx: mpsc::Sender<(u64, Result<Attachment, String>)>,
    rx: Receiver<(u64, Result<Attachment, String>)>,
    pub gallery: Gallery,
}
impl Default for DraftImages {
    fn default() -> Self {
        let (tx, rx) = mpsc::channel();
        Self {
            items: Vec::new(),
            next: 0,
            tx,
            rx,
            gallery: Gallery::default(),
        }
    }
}
impl DraftImages {
    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }
    pub fn ready(&self) -> bool {
        !self.items.is_empty() && self.items.iter().all(|i| i.image.is_some())
    }
    pub fn clear(&mut self) {
        self.items.clear();
    }
    pub fn set_images(&mut self, images: Vec<Attachment>) {
        self.clear();
        for image in images {
            self.next += 1;
            self.items.push(Item {
                id: self.next,
                name: "Queued image".into(),
                image: Some(image),
                error: None,
            });
        }
    }
    pub fn images(&self) -> Vec<Attachment> {
        self.items.iter().filter_map(|i| i.image.clone()).collect()
    }
    fn start(
        &mut self,
        name: String,
        ctx: &egui::Context,
        load: impl FnOnce() -> Result<Attachment, String> + Send + 'static,
    ) {
        if self.items.len() >= MAX_IMAGES {
            return;
        }
        self.next += 1;
        let id = self.next;
        self.items.push(Item {
            id,
            name,
            image: None,
            error: None,
        });
        let tx = self.tx.clone();
        let ctx = ctx.clone();
        std::thread::spawn(move || {
            let _ = tx.send((id, load()));
            ctx.request_repaint();
        });
    }
    pub fn add_paths(&mut self, paths: Vec<PathBuf>, ctx: &egui::Context) {
        for path in paths {
            let name = path
                .file_name()
                .unwrap_or_default()
                .to_string_lossy()
                .into_owned();
            self.start(name, ctx, move || load_file(&path));
        }
    }
    pub fn clipboard(&mut self, ctx: &egui::Context) {
        self.start("Clipboard image".into(), ctx, || {
            let image = arboard::Clipboard::new()
                .and_then(|mut c| c.get_image())
                .map_err(|_| {
                    "No image on clipboard. Copy an image, or use Attach images.".to_owned()
                })?;
            if image.width as u64 * image.height as u64 > 24_000_000 {
                return Err("Clipboard image exceeds 24 megapixels".into());
            }
            let image = image::RgbaImage::from_raw(
                image.width as u32,
                image.height as u32,
                image.bytes.into_owned(),
            )
            .ok_or("Invalid clipboard image")?;
            let mut bytes = std::io::Cursor::new(Vec::new());
            image::DynamicImage::ImageRgba8(image)
                .write_to(&mut bytes, image::ImageFormat::Png)
                .map_err(|_| "Could not encode clipboard image")?;
            Attachment::from_bytes(bytes.get_ref())
        });
    }
    pub fn poll(&mut self, ctx: &egui::Context) {
        for (id, result) in self.rx.try_iter() {
            if let Some(item) = self.items.iter_mut().find(|i| i.id == id) {
                match result {
                    Ok(image) => item.image = Some(image),
                    Err(error) => item.error = Some(error),
                }
            }
        }
        self.gallery.poll(ctx);
    }
    pub fn show(&mut self, ui: &mut egui::Ui) {
        let colors = crate::theme::palette(ui.ctx());
        if self.items.is_empty() {
            return;
        }
        let mut remove = None;
        let images = self.images();
        ui.horizontal_wrapped(|ui| {
            ui.spacing_mut().item_spacing = egui::vec2(9.0, 7.0);
            for item in &self.items {
                ui.push_id(item.id, |ui| {
                    let rect = if let Some(image) = &item.image {
                        let response = self.gallery.thumbnail(ui, image, egui::vec2(65.0, 55.0));
                        if response.clicked() {
                            self.gallery.open(
                                &images,
                                images.iter().position(|i| i.key == image.key).unwrap_or(0),
                            );
                        }
                        response.on_hover_text(&item.name).rect
                    } else {
                        let (rect, response) =
                            ui.allocate_exact_size(egui::vec2(65.0, 55.0), Sense::hover());
                        ui.painter().rect_filled(rect, 8, colors.surface);
                        ui.painter().text(
                            rect.center(),
                            egui::Align2::CENTER_CENTER,
                            if item.error.is_some() {
                                "!"
                            } else {
                                "Loading…"
                            },
                            FontId::proportional(10.0),
                            if item.error.is_some() {
                                colors.red
                            } else {
                                colors.muted
                            },
                        );
                        response.on_hover_text(item.error.as_deref().unwrap_or(&item.name));
                        rect
                    };
                    let close = egui::Rect::from_center_size(rect.right_top(), Vec2::splat(18.0));
                    if ui
                        .put(close, egui::Button::new("×").fill(colors.bg))
                        .on_hover_text(format!("Remove {}", item.name))
                        .clicked()
                    {
                        remove = Some(item.id);
                    }
                });
            }
        });
        if let Some(id) = remove {
            self.items.retain(|i| i.id != id);
        }
        ui.add_space(6.0);
    }
}
fn load_file(path: &Path) -> Result<Attachment, String> {
    let file = File::open(path).map_err(|_| "Cannot open image")?;
    if file.metadata().map_err(|_| "Cannot inspect image")?.len() > MAX_BYTES {
        return Err("Image is larger than 8 MB".into());
    }
    let mut bytes = Vec::new();
    file.take(MAX_BYTES + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| "Cannot read image")?;
    Attachment::from_bytes(&bytes)
}

impl Pui {
    pub(super) fn image_inputs(&mut self, ctx: &egui::Context) {
        self.draft_images.poll(ctx);
        if let Some(paths) = self
            .image_picker_rx
            .as_ref()
            .and_then(|r| r.try_recv().ok())
        {
            self.image_picker_rx = None;
            self.draft_images.add_paths(paths, ctx);
        }
        #[cfg(not(target_os = "macos"))]
        if ctx.memory(|m| m.has_focus(egui::Id::new("composer")))
            && ctx.input_mut(|i| i.consume_key(egui::Modifiers::CTRL, egui::Key::V))
        {
            self.paste_requested.store(true, Ordering::Relaxed);
        }
        if self.paste_requested.swap(false, Ordering::Relaxed) {
            self.draft_images.clipboard(ctx);
            self.focus_composer = true;
        }
        let dropped = ctx.input_mut(|i| std::mem::take(&mut i.raw.dropped_files));
        let paths = dropped
            .into_iter()
            .filter_map(|f| f.path)
            .collect::<Vec<_>>();
        if !paths.is_empty() {
            self.draft_images.add_paths(paths, ctx);
            self.focus_composer = true;
        }
    }
    pub(super) fn pick_images(&mut self) {
        if self.image_picker_rx.is_some() {
            return;
        }
        let (tx, rx) = mpsc::channel();
        self.image_picker_rx = Some(rx);
        let ctx = self.ctx.clone();
        std::thread::spawn(move || {
            let files = rfd::FileDialog::new()
                .set_title("Attach images")
                .add_filter("Images", &["png", "jpg", "jpeg", "gif", "webp"])
                .pick_files()
                .unwrap_or_default();
            let _ = tx.send(files);
            ctx.request_repaint();
        });
    }
}

/// egui-winit consumes Cmd-V even when its text clipboard is empty. Observe
/// only this application's paste key, never global keys or periodic clipboard polling.
#[cfg(target_os = "macos")]
pub(super) fn clipboard_has_image() -> bool {
    objc2_app_kit::NSPasteboard::generalPasteboard()
        .types()
        .is_some_and(|types| {
            types.iter().any(|t| {
                matches!(
                    t.to_string().as_str(),
                    "public.png" | "public.tiff" | "public.jpeg"
                )
            })
        })
}
#[cfg(target_os = "macos")]
pub(super) struct PasteMonitor(objc2::rc::Retained<objc2::runtime::AnyObject>);
#[cfg(target_os = "macos")]
impl PasteMonitor {
    pub fn install(ctx: &egui::Context, requested: std::sync::Arc<AtomicBool>) -> Option<Self> {
        use objc2_app_kit::{NSEvent, NSEventMask, NSEventModifierFlags, NSPasteboard};
        let ctx = ctx.clone();
        let block =
            block2::RcBlock::new(move |event: std::ptr::NonNull<NSEvent>| -> *mut NSEvent {
                // SAFETY: AppKit supplies a valid event for the duration of the local callback.
                let e = unsafe { event.as_ref() };
                if e.keyCode() == 9
                    && e.modifierFlags().contains(NSEventModifierFlags::Command)
                    && ctx.memory(|m| m.has_focus(egui::Id::new("composer")))
                {
                    let has_image =
                        NSPasteboard::generalPasteboard()
                            .types()
                            .is_some_and(|types| {
                                types.iter().any(|t| {
                                    matches!(
                                        t.to_string().as_str(),
                                        "public.png" | "public.tiff" | "public.jpeg"
                                    )
                                })
                            });
                    if has_image {
                        requested.store(true, Ordering::Relaxed);
                        ctx.request_repaint();
                        return std::ptr::null_mut();
                    }
                }
                event.as_ptr()
            });
        // SAFETY: the copied block returns the original event or null and is removed on drop.
        unsafe {
            NSEvent::addLocalMonitorForEventsMatchingMask_handler(NSEventMask::KeyDown, &block)
        }
        .map(Self)
    }
}
#[cfg(target_os = "macos")]
impl Drop for PasteMonitor {
    fn drop(&mut self) {
        unsafe {
            objc2_app_kit::NSEvent::removeMonitor(&self.0);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn files_use_detected_mime_not_filename() {
        let image = load_file(Path::new(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/assets/icon.png"
        )))
        .unwrap();
        assert_eq!(image.mime, "image/png");
        assert_eq!(image.to_json()["type"], "image");
    }
    #[test]
    fn attachment_limit_and_removal_of_loading_jobs() {
        let ctx = egui::Context::default();
        let mut draft = DraftImages::default();
        for _ in 0..10 {
            draft.start("bad".into(), &ctx, || Err("Invalid".into()));
        }
        assert_eq!(draft.items.len(), 8);
        draft.clear();
        std::thread::sleep(Duration::from_millis(10));
        draft.poll(&ctx);
        assert!(draft.is_empty());
    }
}
