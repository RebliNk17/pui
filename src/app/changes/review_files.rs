//! O(1) retained state for navigating one unchanged recorded-output projection.
//! Bodies are never snapshotted as a second list; only the selected diff is copied.
use super::*;
const MAX_FILES: usize = 256;
const MAX_PATH: usize = 4096;
const MAX_CURRENT_DIFF: usize = 2 * 1024 * 1024;

#[derive(Default)]
pub(super) struct Files {
    revision: Option<u64>,
    position: Option<usize>,
    count: usize,
    limited: bool,
}
impl Files {
    fn bind(&mut self, document: &diff_viewer::Document, outputs: &[RecordedFile], revision: u64) {
        if self.revision.is_some() {
            return;
        }
        self.revision = Some(revision);
        self.count = outputs.len();
        self.limited = outputs.len() > MAX_FILES
            || document.1.len() > MAX_CURRENT_DIFF
            || outputs
                .iter()
                .take(MAX_FILES)
                .any(|file| file.path.len() > MAX_PATH || file.diff.len() > MAX_CURRENT_DIFF);
        if !self.limited {
            self.position = outputs
                .iter()
                .rev()
                .position(|file| file.path == document.0 && file.diff == document.1);
        }
    }
    fn available(&self, revision: u64) -> bool {
        !self.limited && self.revision == Some(revision) && self.position.is_some()
    }
    fn previous(&self, revision: u64) -> Option<usize> {
        self.available(revision)
            .then_some(self.position?)
            .and_then(|position| position.checked_sub(1))
    }
    fn next(&self, revision: u64) -> Option<usize> {
        let next = self.position?.saturating_add(1);
        (self.available(revision) && next < self.count).then_some(next)
    }
    fn label(&self, revision: u64) -> String {
        if self.limited {
            "File navigation unavailable · 256 files / 4 KiB path / 2 MiB per-diff limit".into()
        } else if self.revision != Some(revision) {
            "Recorded file list changed · reopen review to navigate".into()
        } else if let Some(position) = self.position {
            format!("Recorded file {} of {}", position + 1, self.count)
        } else {
            "This snapshot is not in the recorded file list".into()
        }
    }
}

impl Changes {
    pub(in crate::app) fn bind_review_files(&mut self, outputs: &[RecordedFile], revision: u64) {
        if let Some(document) = &self.review
            && document.3
        {
            self.review_files.bind(document, outputs, revision);
        }
    }

    pub(super) fn review_file_inputs(
        &mut self,
        ctx: &egui::Context,
        outputs: &[RecordedFile],
        revision: u64,
    ) {
        let Some(document) = &self.review else {
            return;
        };
        if !document.3
            || !document.2.owns_focus(ctx)
            || egui::Popup::is_any_open(ctx)
            || ctx.memory(|memory| memory.top_modal_layer().is_some())
        {
            return;
        }
        self.review_files.bind(document, outputs, revision);
        let mut position = self.review_files.position;
        let available = self.review_files.available(revision);
        let mut requested = None;
        ctx.input_mut(|input| {
            input.events.retain(|event| {
                let egui::Event::Key {
                    key,
                    modifiers,
                    pressed: true,
                    ..
                } = event
                else {
                    return true;
                };
                if !modifiers.matches_exact(egui::Modifiers::ALT) {
                    return true;
                }
                if !matches!(key, egui::Key::ArrowLeft | egui::Key::ArrowRight) {
                    return true;
                }
                if available {
                    let next = match key {
                        egui::Key::ArrowLeft => {
                            position.and_then(|position| position.checked_sub(1))
                        }
                        _ => position
                            .map(|position| position + 1)
                            .filter(|position| *position < self.review_files.count),
                    };
                    if let Some(next) = next {
                        position = Some(next);
                        requested = Some(next);
                    }
                }
                // Owned by this focused review, even at an unavailable boundary.
                false
            })
        });
        if let Some(position) = requested {
            self.select_recorded_file(ctx, outputs, revision, position);
        }
    }

    pub(super) fn review_file_navigation(
        &mut self,
        ctx: &egui::Context,
        outputs: &[RecordedFile],
        revision: u64,
    ) -> diff_viewer::FileNavigation {
        self.bind_review_files(outputs, revision);
        let status = self.review_files.label(revision);
        let available = self.review_files.available(revision);
        let enabled = ctx.memory(|memory| memory.top_modal_layer().is_none())
            && !egui::Popup::is_any_open(ctx);
        diff_viewer::FileNavigation {
            previous: enabled && self.review_files.previous(revision).is_some(),
            next: enabled && self.review_files.next(revision).is_some(),
            position: self
                .review_files
                .position
                .map(|position| (position, self.review_files.count)),
            warning: (!available).then(|| status.clone()),
            status,
            git: false,
            loading: false,
        }
    }
    pub(super) fn step_recorded_file(
        &mut self,
        ctx: &egui::Context,
        outputs: &[RecordedFile],
        revision: u64,
        next: bool,
    ) {
        let target = if next {
            self.review_files.next(revision)
        } else {
            self.review_files.previous(revision)
        };
        if let Some(target) = target {
            self.select_recorded_file(ctx, outputs, revision, target);
        }
    }

    fn select_recorded_file(
        &mut self,
        ctx: &egui::Context,
        outputs: &[RecordedFile],
        revision: u64,
        position: usize,
    ) {
        if !self.review_files.available(revision) || position >= self.review_files.count {
            return;
        }
        let Some(file) = outputs.get(self.review_files.count - 1 - position) else {
            return;
        };
        let mut document: diff_viewer::Document = (file.path.clone(), file.diff.clone()).into();
        document.3 = true;
        context_capture::clear_source(ctx);
        let mut selection = egui::text_selection::LabelSelectionState::load(ctx);
        selection.clear_selection();
        selection.store(ctx);
        document.2.focus(ctx);
        self.review = Some(document);
        self.review_files.position = Some(position);
    }
}

#[cfg(test)]
mod selection_tests;
#[cfg(test)]
mod tests;
#[cfg(test)]
mod ui_tests;
