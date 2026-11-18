//! Explicit object-only image read/decode on a worker with shared preview leases.
use super::*;
use crate::app::files::{budget::Lease, image_preview};
struct Loaded {
    images: [Option<egui::ColorImage>; 2],
    _leases: [Arc<Lease>; 2],
}
#[derive(Default)]
pub(in crate::app::changes::branches::history::details) struct Image {
    pending: Option<workspace::read_only::Request<Result<Loaded, String>>>,
    loaded: Option<Loaded>,
    error: Option<String>,
    pub(in crate::app::changes::branches::history::details) control: Option<egui::Id>,
    pub(in crate::app::changes::branches::history::details) key: Option<egui::Id>,
    textures: [Option<egui::TextureHandle>; 2],
}
impl Image {
    pub(super) fn pending(&self) -> bool {
        self.pending.is_some()
    }
    pub(super) fn poll(&mut self) {
        let result = self.pending.as_ref().and_then(|r| match r.try_recv() {
            Ok(r) => Some(r),
            Err(mpsc::TryRecvError::Empty) => None,
            Err(_) => Some(Err(
                "Image worker ended; select the file again to retry.".into()
            )),
        });
        if let Some(result) = result {
            self.pending = None;
            match result {
                Ok(images) => self.loaded = Some(images),
                Err(e) => self.error = Some(e),
            }
        }
    }
    fn start(
        &mut self,
        ctx: &egui::Context,
        owner: &Owner,
        detail: &workspace::history::details::Detail,
        path: &str,
        base: usize,
    ) {
        if self.pending.is_some() || self.loaded.is_some() || self.error.is_some() {
            return;
        }
        let Some(permit) = permit(ctx) else {
            self.error = Some("Git busy; select file again explicitly.".into());
            return;
        };
        let root = owner.root.clone();
        let hash = detail.oid.clone();
        let parent = detail.parents.get(base).cloned();
        let path = path.to_owned();
        let encoded = Lease::new(ctx);
        let leases = [Lease::new(ctx), Lease::new(ctx)];
        let (tx, rx) = mpsc::channel();
        let cancel = Arc::new(AtomicBool::new(false));
        self.pending = Some(workspace::read_only::Request::new(rx, cancel.clone()));
        let ctx = ctx.clone();
        if std::thread::Builder::new()
            .name("pui-history-images".into())
            .spawn(move || {
                let result = (|| {
                    // Reserve both bounded encoded inputs before Git allocates them.
                    encoded.reserve(32 * 1024 * 1024)?;
                    let blobs = workspace::history::image::read_image_diff(
                        &root,
                        &hash,
                        parent.as_deref(),
                        &path,
                        &cancel,
                    )?;
                    let decode = |bytes: &[u8], lease: &Lease| {
                        if bytes.is_empty() {
                            Ok(None)
                        } else {
                            image_preview::decode(bytes, Some(lease), Some(&cancel)).map(Some)
                        }
                    };
                    let images = [
                        decode(&blobs.before, &leases[0])?,
                        decode(&blobs.after, &leases[1])?,
                    ];
                    Ok(Loaded {
                        images,
                        _leases: leases,
                    })
                })();
                drop(encoded);
                drop(permit);
                if !cancel.load(Ordering::Acquire) {
                    let _ = tx.send(result);
                    ctx.request_repaint();
                }
            })
            .is_err()
        {
            self.pending = None;
            self.error = Some("Cannot start image worker.".into());
        }
    }
    pub(super) fn show(
        &mut self,
        ui: &mut egui::Ui,
        owner: &Owner,
        detail: &workspace::history::details::Detail,
        demo: bool,
        path: &str,
        base: usize,
    ) {
        self.control = None;
        let key = self.key.take();
        if !workspace::history::image::is_image(path) {
            return;
        }
        let enabled =
            !demo && self.pending.is_none() && self.loaded.is_none() && self.error.is_none();
        let button = ui
            .push_id(
                (
                    "image-diff",
                    &owner.identity,
                    &owner.root,
                    &detail.oid,
                    detail.parents.get(base),
                    path,
                ),
                |ui| prompt_ui::button(ui, "Image diff", prompt_ui::Button::Secondary, enabled),
            )
            .inner;
        self.control = Some(button.id);
        #[cfg(test)]
        ui.ctx().data_mut(|d| {
            d.insert_temp(
                popup_id().with(("image-diff", path)),
                (button.id, button.rect, button.enabled()),
            )
        });
        if enabled && (button.clicked() || key == Some(button.id)) {
            self.start(ui.ctx(), owner, detail, path, base);
        }
        if self.pending.is_some() {
            ui.label(theme::small(ui, "Reading and decoding local image blobs…"));
        }
        if let Some(error) = &self.error {
            ui.add(egui::Label::new(theme::small(ui, error)).wrap());
        }
        if let Some(loaded) = &mut self.loaded {
            for i in 0..2 {
                if self.textures[i].is_none()
                    && let Some(pixels) = loaded.images[i].take()
                {
                    self.textures[i] = Some(ui.ctx().load_texture(
                        format!("history-image-{i}"),
                        pixels,
                        egui::TextureOptions::LINEAR,
                    ));
                }
            }
            ui.label(theme::small(
                ui,
                "Before / After · first frame only · local objects, no fetch",
            ));
            let width = (ui.available_width() - 12.0).max(2.0) / 2.0;
            ui.horizontal_top(|ui| {
                for (i, label) in ["Before", "After"].iter().enumerate() {
                    ui.vertical(|ui| {
                        ui.set_width(width);
                        ui.label(theme::small(ui, *label));
                        if let Some(tex) = &self.textures[i] {
                            let size = tex.size_vec2();
                            let scale = (width / size.x).min(180.0 / size.y).min(1.0);
                            ui.image((tex.id(), size * scale));
                        } else {
                            ui.label(theme::small(ui, "No image in this tree"));
                        }
                    });
                }
            });
        }
    }
}
#[cfg(test)]
mod tests;
