//! Drafts are ephemeral. Explicitly attached non-image files get private durable copies.
use super::*;
use crate::media::{Attachment, Gallery};
use std::{
    fs::File,
    io::Read,
    path::Path,
    sync::atomic::{AtomicBool, Ordering},
};

const MAX_ATTACHMENTS: usize = 8;
const MAX_BYTES: u64 = 64 * 1024 * 1024;
pub(super) struct Item {
    id: u64,
    name: String,
    pub image: Option<Attachment>,
    error: Option<String>,
}
pub(super) struct DraftImages {
    items: Vec<Item>,
    next: u64,
    revision: u64,
    limit_notice: bool,
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
            revision: 0,
            limit_notice: false,
            tx,
            rx,
            gallery: Gallery::default(),
        }
    }
}
impl DraftImages {
    pub(super) fn revision(&self) -> u64 {
        self.revision
    }
    pub(super) fn append_reviewed(
        &mut self,
        name: String,
        image: Attachment,
    ) -> Result<(), String> {
        if self.items.len() >= MAX_ATTACHMENTS {
            return Err("Up to 8 attachments; nothing added.".into());
        }
        if image.file.is_some()
            || image.local_path.is_some()
            || image.data.len() > 12 * 1024 * 1024
            || !matches!(
                image.mime.as_str(),
                "image/png" | "image/jpeg" | "image/webp"
            )
        {
            return Err("Unsupported bounded browser image.".into());
        }
        self.next = self.next.checked_add(1).ok_or("Attachment ID exhausted")?;
        self.revision = self.revision.wrapping_add(1);
        self.items.push(Item {
            id: self.next,
            name,
            image: Some(image),
            error: None,
        });
        Ok(())
    }
    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }
    pub fn ready(&self) -> bool {
        !self.items.is_empty() && self.items.iter().all(|i| i.image.is_some())
    }
    pub fn clear(&mut self) {
        self.revision = self.revision.wrapping_add(1);
        self.items.clear();
        self.limit_notice = false;
    }
    pub fn set_images(&mut self, images: Vec<Attachment>) {
        self.clear();
        for image in images {
            self.next += 1;
            self.items.push(Item {
                id: self.next,
                name: image
                    .file
                    .as_ref()
                    .map(|f| f.name.clone())
                    .unwrap_or_else(|| "Queued image".into()),
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
        if self.items.len() >= MAX_ATTACHMENTS {
            self.limit_notice = true;
            return;
        }
        self.next += 1;
        self.revision = self.revision.wrapping_add(1);
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
                    "No image on clipboard. Copy an image, or use Attach files.".to_owned()
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
                self.revision = self.revision.wrapping_add(1);
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
        if self.limit_notice {
            ui.label(
                RichText::new("Up to 8 files per message. Additional files were not attached.")
                    .size(11.0)
                    .color(colors.amber),
            );
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
                    if remove_button(ui, rect, &item.name).clicked() {
                        remove = Some(item.id);
                    }
                });
            }
        });
        if let Some(id) = remove {
            self.revision = self.revision.wrapping_add(1);
            self.items.retain(|i| i.id != id);
            self.limit_notice = false;
        }
        ui.add_space(6.0);
    }
}

fn remove_button(ui: &mut egui::Ui, thumbnail: egui::Rect, name: &str) -> egui::Response {
    let colors = crate::theme::palette(ui.ctx());
    // Keep a generous hit target inside the preview without inheriting the
    // application's full-size button padding or expanding the attachment row.
    let rect = egui::Rect::from_min_size(
        thumbnail.right_top() + egui::vec2(-24.0, 2.0),
        Vec2::splat(22.0),
    );
    let response = ui.interact(rect, ui.id().with("remove-attachment"), Sense::click());
    let active = response.hovered() || response.has_focus();
    ui.painter().circle_filled(
        rect.center(),
        9.0,
        if active { colors.hover } else { colors.bg },
    );
    ui.painter().circle_stroke(
        rect.center(),
        9.0,
        egui::Stroke::new(1.0, if active { colors.muted } else { colors.border }),
    );
    icons::draw(
        ui,
        rect,
        Icon::Close,
        if active { colors.text } else { colors.muted },
    );
    let label = format!("Remove {name}");
    response.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, &label));
    response
        .on_hover_cursor(egui::CursorIcon::PointingHand)
        .on_hover_text(label)
}

fn load_file(path: &Path) -> Result<Attachment, String> {
    let metadata = std::fs::metadata(path).map_err(|_| "Cannot inspect file")?;
    if !metadata.is_file() {
        return Err("Only regular files can be attached".into());
    }
    let file = File::open(path).map_err(|_| "Cannot open file")?;
    let metadata = file.metadata().map_err(|_| "Cannot inspect file")?;
    if !metadata.is_file() {
        return Err("Only regular files can be attached".into());
    }
    if metadata.len() > MAX_BYTES {
        return Err("File is larger than 64 MiB".into());
    }
    let mut bytes = Vec::new();
    file.take(MAX_BYTES + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| "Cannot read file")?;
    if bytes.len() as u64 > MAX_BYTES {
        return Err("File is larger than 64 MiB".into());
    }
    if matches!(
        image::guess_format(&bytes),
        Ok(image::ImageFormat::Png
            | image::ImageFormat::Jpeg
            | image::ImageFormat::Gif
            | image::ImageFormat::WebP)
    ) {
        return Attachment::from_bytes(&bytes);
    }
    let home = dirs::data_local_dir()
        .ok_or("Cannot locate attachment storage")?
        .join("pui/attachments-v1");
    retain_file(&home, path, &bytes)
}

/// Copies are immutable per attachment and survive chat switches/restarts. Never
/// delete them on Send: saved Pi conversations still reference these paths.
fn retain_file(home: &Path, source: &Path, bytes: &[u8]) -> Result<Attachment, String> {
    use std::io::Write;
    let directory = home.join(uuid::Uuid::new_v4().to_string());
    let mut builder = std::fs::DirBuilder::new();
    builder.recursive(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::DirBuilderExt;
        builder.mode(0o700);
    }
    builder
        .create(&directory)
        .map_err(|_| "Cannot create attachment storage")?;
    let name = source.file_name().ok_or("File has no name")?;
    let path = directory.join(name);
    let mut options = std::fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    options
        .open(&path)
        .and_then(|mut f| f.write_all(bytes))
        .map_err(|_| "Cannot retain attached file")?;
    Ok(Attachment::from_file(crate::media::FileAttachment {
        path,
        name: name.to_string_lossy().into_owned(),
        size: bytes.len() as u64,
    }))
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
                .set_title("Attach files")
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

    fn attachment_frame(
        ctx: &egui::Context,
        draft: &mut DraftImages,
        events: Vec<egui::Event>,
    ) -> Vec<egui::Pos2> {
        let output = ctx.run(
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(400.0, 200.0),
                )),
                events,
                ..Default::default()
            },
            |ctx| {
                egui::CentralPanel::default().show(ctx, |ui| draft.show(ui));
            },
        );
        output
            .shapes
            .iter()
            .filter_map(|shape| match &shape.shape {
                egui::Shape::Circle(circle)
                    if circle.radius == 9.0 && circle.fill != egui::Color32::TRANSPARENT =>
                {
                    Some(circle.center)
                }
                _ => None,
            })
            .collect()
    }

    #[test]
    fn attachment_remove_is_compact_and_stays_inside_the_preview() {
        for mode in [theme::Theme::Light, theme::Theme::Dark] {
            let ctx = egui::Context::default();
            theme::install(&ctx);
            mode.apply(&ctx);
            for compact in [false, true] {
                let _ = ctx.run(egui::RawInput::default(), |ctx| {
                    egui::CentralPanel::default().show(ctx, |ui| {
                        if compact {
                            theme::compact_controls(ui);
                        }
                        let (preview, _) =
                            ui.allocate_exact_size(egui::vec2(65.0, 55.0), Sense::hover());
                        let cursor = ui.cursor();
                        let response = remove_button(ui, preview, "image.png");
                        assert_eq!(response.rect.size(), Vec2::splat(22.0));
                        assert!(preview.contains_rect(response.rect));
                        assert_eq!(ui.cursor(), cursor, "Overlay must not expand the row");
                    });
                });
            }
        }
    }

    #[test]
    fn attachment_remove_only_removes_its_item_without_opening_the_gallery() {
        let ctx = egui::Context::default();
        theme::install(&ctx);
        let image = Attachment::from_bytes(include_bytes!("../../assets/icon.png")).unwrap();
        let file = Attachment::from_file(crate::media::FileAttachment {
            path: PathBuf::from("/fixture/report.csv"),
            name: "report.csv".into(),
            size: 12,
        });
        let mut draft = DraftImages::default();
        draft.set_images(vec![image, file]);
        draft.items.push(Item {
            id: 3,
            name: "Loading image".into(),
            image: None,
            error: None,
        });
        draft.items.push(Item {
            id: 4,
            name: "Failed image".into(),
            image: None,
            error: Some("Unavailable".into()),
        });
        for id in [2, 1, 3, 4] {
            attachment_frame(&ctx, &mut draft, vec![]);
            let buttons = attachment_frame(&ctx, &mut draft, vec![]);
            assert_eq!(buttons.len(), draft.items.len());
            let index = draft.items.iter().position(|item| item.id == id).unwrap();
            let pos = buttons[index];
            for pressed in [true, false] {
                attachment_frame(
                    &ctx,
                    &mut draft,
                    vec![
                        egui::Event::PointerMoved(pos),
                        egui::Event::PointerButton {
                            pos,
                            button: egui::PointerButton::Primary,
                            pressed,
                            modifiers: egui::Modifiers::NONE,
                        },
                    ],
                );
            }
            assert!(!draft.items.iter().any(|item| item.id == id));
            assert!(
                !draft.gallery.is_open(),
                "Remove must not activate the thumbnail"
            );
        }
        assert!(draft.is_empty());
    }

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
    fn csv_binary_and_empty_files_are_retained_privately_without_text_conversion() {
        let home = std::env::temp_dir().join(format!("pui-attachments-{}", uuid::Uuid::new_v4()));
        for (name, bytes) in [
            ("report file.csv", b"name,value\na,2\n".as_slice()),
            ("binary.pdf", b"%PDF\0\xff".as_slice()),
            ("empty", b"".as_slice()),
        ] {
            let attachment = retain_file(&home, Path::new(name), bytes).unwrap();
            let file = attachment.file.as_ref().unwrap();
            assert_eq!(file.name, name);
            assert_eq!(file.size, bytes.len() as u64);
            assert_eq!(std::fs::read(&file.path).unwrap(), bytes);
            assert!(file.path.is_absolute());
            assert!(attachment.data.is_empty());
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                assert_eq!(
                    std::fs::metadata(&file.path).unwrap().permissions().mode() & 0o777,
                    0o600
                );
                assert_eq!(
                    std::fs::metadata(file.path.parent().unwrap())
                        .unwrap()
                        .permissions()
                        .mode()
                        & 0o777,
                    0o700
                );
            }
        }
    }
    #[test]
    fn picker_and_drop_loading_accept_csv_and_failures_block_send() {
        let root = std::env::temp_dir().join(format!("pui-attach-input-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir(&root).unwrap();
        let source = root.join("report.csv");
        let bytes = b"name,value\na,2\n";
        std::fs::write(&source, bytes).unwrap();
        let attachment = load_file(&source).unwrap();
        std::fs::write(&source, "changed").unwrap();
        assert_eq!(
            std::fs::read(&attachment.file.unwrap().path).unwrap(),
            bytes
        );
        assert!(load_file(&root).unwrap_err().contains("regular"));
        let oversized = root.join("large.zip");
        File::create(&oversized)
            .unwrap()
            .set_len(MAX_BYTES + 1)
            .unwrap();
        assert!(load_file(&oversized).unwrap_err().contains("64 MiB"));
        let ctx = egui::Context::default();
        let mut draft = DraftImages::default();
        draft.add_paths(vec![root.join("missing.csv")], &ctx);
        for _ in 0..100 {
            draft.poll(&ctx);
            if draft.items[0].error.is_some() {
                break;
            }
            std::thread::sleep(Duration::from_millis(2));
        }
        assert!(!draft.ready());
        assert!(draft.items[0].error.is_some());
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
