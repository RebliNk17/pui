//! Explicit current-page capture, frozen-pixel editing, review, then draft-only handoff.
use super::*;
use crate::app::files::budget::Lease;
use std::sync::Arc;
mod pixels;
mod transport;
mod ui;
use pixels::{Edits, Frozen, Tool};
pub(super) use transport::State;
#[cfg(target_os = "macos")]
pub(super) use transport::{Command, Reply, Ticket, decode};

pub(in crate::app) struct DraftImage {
    pub page: String,
    pub epoch: u64,
    pub request: u64,
    pub draft_token: u64,
    pub image: crate::media::Attachment,
}
enum Phase {
    Disclosure,
    Capturing,
    Editing,
    Rendering,
    Review,
}
struct Review {
    image: crate::media::Attachment,
    preview: Option<egui::ColorImage>,
}
pub(super) struct Editor {
    pub(super) key: String,
    epoch: u64,
    draft_token: u64,
    phase: Phase,
    ticket: Option<transport::Ticket>,
    frozen: Option<Frozen>,
    texture: Option<egui::TextureHandle>,
    review_texture: Option<egui::TextureHandle>,
    review: Option<Review>,
    edits: Option<Edits>,
    history: Vec<Edits>,
    tool: Tool,
    drag: Option<[u32; 2]>,
    render_rx: Option<mpsc::Receiver<Result<Review, String>>>,
    error: Option<String>,
    modal_layer: Option<egui::LayerId>,
    focus_cancel: bool,
    handoff_ctx: Option<egui::Context>,
}
impl Editor {
    fn new(key: &str, epoch: u64, draft_token: u64) -> Self {
        Self {
            key: key.into(),
            epoch,
            draft_token,
            phase: Phase::Disclosure,
            ticket: None,
            frozen: None,
            texture: None,
            review_texture: None,
            review: None,
            edits: None,
            history: Vec::new(),
            tool: Tool::Crop,
            drag: None,
            render_rx: None,
            error: None,
            modal_layer: None,
            focus_cancel: true,
            handoff_ctx: None,
        }
    }
    fn poll(&mut self, ctx: &egui::Context) {
        if let Some(ticket) = self.ticket.as_ref() {
            let result = ticket.mailbox.lock().ok().and_then(|mut slot| slot.take());
            if let Some(result) = result {
                match result {
                    Ok(mut frozen) => {
                        self.edits = Some(Edits::full(&frozen.pixels));
                        if let Some(image) = frozen.preview.take() {
                            self.texture = Some(ctx.load_texture(
                                format!("browser-capture-{}", ticket.request),
                                image,
                                egui::TextureOptions::LINEAR,
                            ));
                        }
                        self.frozen = Some(frozen);
                        self.phase = Phase::Editing;
                    }
                    Err(error) => {
                        self.error = Some(error);
                        self.phase = Phase::Disclosure;
                    }
                }
            }
        }
        if let Some(rx) = &self.render_rx {
            let result = match rx.try_recv() {
                Ok(result) => Some(result),
                Err(mpsc::TryRecvError::Disconnected) => {
                    Some(Err("Screenshot worker ended. Review again.".into()))
                }
                Err(_) => None,
            };
            if let Some(result) = result {
                self.render_rx = None;
                match result {
                    Ok(mut review) => {
                        if let Some(image) = review.preview.take() {
                            self.review_texture = Some(ctx.load_texture(
                                "browser-capture-review",
                                image,
                                egui::TextureOptions::LINEAR,
                            ));
                        }
                        self.review = Some(review);
                        self.phase = Phase::Review;
                    }
                    Err(error) => {
                        self.error = Some(error);
                        self.phase = Phase::Editing;
                    }
                }
            }
        }
    }
    fn review(&mut self, ctx: &egui::Context) {
        let (Some(frozen), Some(edits), Some(ticket)) = (&self.frozen, &self.edits, &self.ticket)
        else {
            return;
        };
        let (frozen, edits, ticket, ctx) =
            (frozen.clone(), edits.clone(), ticket.clone(), ctx.clone());
        let (tx, rx) = mpsc::sync_channel(1);
        self.render_rx = Some(rx);
        self.phase = Phase::Rendering;
        self.error = None;
        if std::thread::Builder::new()
            .name("pui-screenshot-review".into())
            .spawn(move || {
                if ticket.cancelled() {
                    return;
                }
                let result = pixels::render(&frozen, &edits).map(|(image, preview)| Review {
                    image,
                    preview: Some(preview),
                });
                if !ticket.cancelled() {
                    let _ = tx.send(result);
                    ctx.request_repaint();
                }
            })
            .is_err()
        {
            self.render_rx = None;
            self.phase = Phase::Editing;
            self.error = Some("Cannot start screenshot worker.".into());
        }
    }
    fn edit(&mut self, tool: Tool, from: [u32; 2], to: [u32; 2]) {
        if let (Some(frozen), Some(edits)) = (&self.frozen, &mut self.edits) {
            let previous = edits.clone();
            if edits.apply(
                tool,
                from,
                to,
                [frozen.pixels.width(), frozen.pixels.height()],
            ) {
                if self.history.len() == 32 {
                    self.history.remove(0);
                }
                self.history.push(previous);
            }
        }
    }
    fn add(&mut self, can_add: bool) -> Option<DraftImage> {
        if !can_add
            || !matches!(self.phase, Phase::Review)
            || self.ticket.as_ref().is_none_or(|t| t.cancelled())
        {
            return None;
        }
        let review = self.review.take()?;
        let ticket = self.ticket.as_ref()?;
        Some(DraftImage {
            page: self.key.clone(),
            epoch: self.epoch,
            request: ticket.request,
            draft_token: self.draft_token,
            image: review.image,
        })
    }
}
impl Drop for Editor {
    fn drop(&mut self) {
        if let Some(ctx) = &self.handoff_ctx {
            context_capture::browser::cancel(ctx, self.draft_token);
        }
        if let Some(ticket) = &self.ticket {
            ticket
                .cancel
                .store(true, std::sync::atomic::Ordering::Release);
        }
    }
}
impl BrowserView {
    pub(super) fn begin_screenshot(
        &mut self,
        ctx: &egui::Context,
        key: &str,
        draft_token: u64,
    ) -> Result<(), String> {
        if key != self.active || self.unavailable {
            return Err("Show the current browser page before capturing.".into());
        }
        let state = self.state(key);
        if state.loading {
            return Err("Wait for the current page to finish loading.".into());
        }
        #[cfg(not(target_os = "macos"))]
        return Err("Page screenshots require bundled Chromium on macOS.".into());
        #[cfg(target_os = "macos")]
        {
            self.inspection = None;
            self.viewport_editor = None;
            self.engine.cancel_inspection(key);
            let mut editor = Editor::new(key, state.screenshot.epoch, draft_token);
            editor.handoff_ctx = Some(ctx.clone());
            self.screenshot = Some(editor);
            Ok(())
        }
    }
    pub(in crate::app) fn cancel_screenshot(&mut self) {
        if let Some(editor) = self.screenshot.take() {
            #[cfg(target_os = "macos")]
            self.engine.cancel_screenshot(&editor.key);
            drop(editor);
        }
    }
    /// Call every visible-page frame, including while the screenshot modal hides
    /// native Chromium. `can_add` must reflect the immutable activation binding.
    pub(super) fn screenshot_ui(
        &mut self,
        ctx: &egui::Context,
        key: &str,
        draft_token: u64,
        can_add: bool,
    ) -> Option<DraftImage> {
        let epoch = self.state(key).screenshot.epoch;
        let stale = self.screenshot.as_ref().is_some_and(|e| {
            e.key != key
                || e.epoch != epoch
                || e.draft_token != draft_token
                || e.ticket.as_ref().is_some_and(|t| t.cancelled())
        });
        if stale {
            self.cancel_screenshot();
            return None;
        }
        let editor = self.screenshot.as_mut()?;
        editor.poll(ctx);
        let decision = ui::show(ctx, editor, can_add);
        if decision.cancel {
            self.cancel_screenshot();
            return None;
        }
        if decision.capture {
            #[cfg(target_os = "macos")]
            match self.engine.capture_screenshot(key, ctx) {
                Ok(ticket) => {
                    editor.epoch = ticket.epoch;
                    editor.ticket = Some(ticket);
                    editor.phase = Phase::Capturing;
                    editor.error = None;
                }
                Err(error) => editor.error = Some(error),
            }
        }
        if decision.review {
            editor.review(ctx);
        }
        let result = if decision.add {
            editor.add(can_add)
        } else {
            None
        };
        if result.is_some() {
            if let Some(editor) = &mut self.screenshot {
                editor.handoff_ctx = None;
            }
            self.cancel_screenshot();
        }
        result
    }
}
#[cfg(test)]
mod handoff_tests;
#[cfg(test)]
mod modal_tests;
#[cfg(test)]
mod tests;
