//! Bounded before/after image diff for a selected image path in a commit.
use super::*;
type BlobPair = (Vec<u8>, Vec<u8>);
type ImagePair = (egui::ColorImage, egui::ColorImage);
#[derive(Default)]
pub(super) struct Image {
    pending: Option<workspace::read_only::Request<Result<BlobPair, String>>>,
    result: Option<Result<BlobPair, String>>,
    decoded: Option<Result<ImagePair, String>>,
    control: Option<egui::Id>,
    key: Option<egui::Id>,
    loading: bool,
    textures: Vec<egui::TextureHandle>,
}
impl Image {
    pub(super) fn poll(&mut self) {
        let result = self.pending.as_ref().and_then(|p| match p.try_recv() {
            Ok(r) => Some(r),
            Err(mpsc::TryRecvError::Empty) => None,
            Err(_) => Some(Err("Image diff worker ended. Back and select again.".into())),
        });
        if let Some(result) = result {
            self.pending = None;
            self.result = Some(result);
            self.loading = false;
        }
        let decoded = self.result.as_ref().and_then(|r| match r {
            Ok((before, after)) => {
                if self.decoded.is_none() {
                    Some((
                        crate::app::files::image_preview::decode(before, None, None),
                        crate::app::files::image_preview::decode(after, None, None),
                    ))
                } else {
                    None
                }
            }
            Err(_) => None,
        });
        if let Some((before_res, after_res)) = decoded {
            self.decoded = Some(match (before_res, after_res) {
                (Ok(before), Ok(after)) => Ok((before, after)),
                _ => Err("Image blob could not be decoded within pixel limits.".into()),
            });
        }
    }
    fn start(
        &mut self,
        ctx: &egui::Context,
        owner: &Owner,
        detail: &workspace::history::details::Detail,
        demo: bool,
        path: &str,
        base: usize,
    ) {
        if self.pending.is_some() || self.result.is_some() || self.loading {
            return;
        }
        if demo {
            self.decoded = Some(Err("Demo mode has no image blobs.".into()));
            return;
        }
        let Some(permit) = permit(ctx) else {
            self.result = Some(Err(
                "A local Git inspection is still finishing. Back and select again to retry.".into(),
            ));
            return;
        };
        let root = owner.root.clone();
        let hash = detail.oid.clone();
        let parent = detail.parents.get(base).cloned();
        let path = path.to_owned();
        let (tx, rx) = mpsc::channel();
        let cancel = Arc::new(AtomicBool::new(false));
        self.pending = Some(workspace::read_only::Request::new(rx, cancel.clone()));
        let ctx = ctx.clone();
        if std::thread::Builder::new()
            .name("pui-image-diff".into())
            .spawn(move || {
                let result = workspace::history::image::read_image_diff(
                    &root,
                    &hash,
                    parent.as_deref(),
                    &path,
                    &cancel,
                )
                .map(|d| (d.before, d.after));
                drop(permit);
                if !cancel.load(Ordering::Acquire) {
                    let _ = tx.send(result);
                    ctx.request_repaint();
                }
            })
            .is_err()
        {
            self.pending = None;
            self.result = Some(Err(
                "Cannot start image diff worker. Back and select again.".into(),
            ));
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
        if !workspace::history::image::is_image(path) {
            return;
        }
        let key = self.key.take();
        let response = ui
            .push_id(("image-diff", path), |ui| {
                ui.add_enabled(
                    self.pending.is_none() && self.result.is_none() && !self.loading,
                    egui::Button::new(theme::small(ui, "Image diff")).frame(false),
                )
            })
            .inner;
        self.control = Some(response.id);
        #[cfg(test)]
        ui.ctx().data_mut(|d| {
            d.insert_temp(
                popup_id().with(("image-diff", path)),
                (response.id, response.rect, response.enabled()),
            )
        });
        if response.clicked() || key == Some(response.id) {
            self.loading = true;
            self.start(ui.ctx(), owner, detail, demo, path, base);
        }
        if self.pending.is_some() {
            ui.label(theme::small(ui, "Reading image blobs…"));
        } else if let Some(Ok((before, after))) = &self.decoded {
            ui.label(theme::small(
                ui,
                format!("Image diff · {} (before ← → after)", path.escape_debug()),
            ));
            if self.textures.len() < 2 {
                self.textures = vec![
                    ui.ctx().load_texture(
                        format!("image-diff-before-{path}"),
                        before.clone(),
                        egui::TextureOptions::LINEAR,
                    ),
                    ui.ctx().load_texture(
                        format!("image-diff-after-{path}"),
                        after.clone(),
                        egui::TextureOptions::LINEAR,
                    ),
                ];
            }
            egui::ScrollArea::horizontal().show(ui, |ui| {
                ui.horizontal(|ui| {
                    if let [before_tex, after_tex] = &self.textures[..] {
                        ui.image((
                            before_tex.id(),
                            egui::Vec2::new(before.size[0] as f32, before.size[1] as f32),
                        ));
                        ui.add_space(8.0);
                        ui.image((
                            after_tex.id(),
                            egui::Vec2::new(after.size[0] as f32, after.size[1] as f32),
                        ));
                    }
                });
            });
        } else if let Some(Err(error)) = &self.decoded {
            ui.add(egui::Label::new(theme::small(ui, error)).wrap());
        }
    }
}
#[cfg(test)]
mod tests;
