//! Bounded ordinal navigation for one immutable, explicitly opened diff.
use super::*;

const MAX_BYTES: usize = 2 * 1024 * 1024;
const MAX_HUNKS: usize = 4096;
const WRAP_MAX_BYTES: usize = 64 * 1024;
const WRAP_MAX_ROWS: usize = 4096;

pub(in crate::app) struct Document(pub String, pub String, pub Navigation, pub bool);
impl From<(String, String)> for Document {
    fn from((path, source): (String, String)) -> Self {
        let navigation = Navigation::new(&source);
        Self(path, source, navigation, false)
    }
}
pub(in crate::app) struct Navigation {
    rows: Vec<u32>,
    selected: Option<usize>,
    target: Option<u32>,
    comment_target: Option<u32>,
    limited: bool,
    header: String,
    wrap: bool,
    wrap_available: bool,
    layout_width: f32,
    focus: egui::Id,
    controls: [Option<egui::Id>; 10],
    untracked: bool,
    comment_digest: Option<[u8; 32]>,
    comments_stale: bool,
}
impl Default for Navigation {
    fn default() -> Self {
        static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);
        Self {
            rows: Vec::new(),
            selected: None,
            target: None,
            comment_target: None,
            limited: false,
            header: String::new(),
            wrap: false,
            wrap_available: false,
            layout_width: 0.0,
            focus: egui::Id::new((
                "file-review",
                NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed),
            )),
            controls: [None; 10],
            untracked: false,
            comment_digest: None,
            comments_stale: false,
        }
    }
}
impl Navigation {
    pub(in crate::app) fn comment_digest(&self) -> Option<[u8; 32]> {
        (!self.comments_stale)
            .then_some(self.comment_digest)
            .flatten()
    }
    pub(in crate::app) fn stale_comments(&mut self) {
        self.comments_stale = true;
    }
    pub(in crate::app) fn selection_response(&self, ctx: &egui::Context) -> Option<egui::Response> {
        ctx.read_response(self.controls[5]?)
    }
    pub(in crate::app) fn set_untracked(&mut self, untracked: bool) {
        self.untracked = untracked;
    }
    pub(super) fn untracked(&self) -> bool {
        self.untracked
    }
    pub fn finish_focus(&self, ctx: &egui::Context) {
        if self.owns_focus(ctx)
            && let Some(id) = ctx.memory(|memory| memory.focused())
        {
            ctx.memory_mut(|memory| {
                memory.set_focus_lock_filter(
                    id,
                    egui::EventFilter {
                        escape: true,
                        vertical_arrows: true,
                        horizontal_arrows: true,
                        ..Default::default()
                    },
                )
            });
        }
    }
    pub fn restore_label_focus(&self, ctx: &egui::Context) {
        if self.controls[5].is_some_and(|id| ctx.memory(|memory| memory.has_focus(id))) {
            self.focus(ctx);
        }
    }
    pub fn focus_id(&self) -> egui::Id {
        self.focus
    }
    pub fn focus(&self, ctx: &egui::Context) {
        ctx.memory_mut(|memory| memory.request_focus(self.focus));
        ctx.request_repaint();
    }
    pub fn register(&mut self, slot: usize, id: egui::Id) {
        self.controls[slot] = Some(id);
    }
    pub fn owns_focus(&self, ctx: &egui::Context) -> bool {
        let escape = ctx.input(|input| input.key_pressed(egui::Key::Escape));
        ctx.memory(|memory| {
            memory.focused().map_or_else(
                || {
                    escape
                        && (memory.had_focus_last_frame(self.focus)
                            || self
                                .controls
                                .iter()
                                .flatten()
                                .any(|id| memory.had_focus_last_frame(*id)))
                },
                |id| id == self.focus || self.controls.contains(&Some(id)),
            )
        })
    }

    pub fn new(source: &str) -> Self {
        Self::build(source, None)
    }
    /// Inline rows share navigation/rendering, but do not support local comments.
    /// Their owner is the exact repository/session, source and path, not global focus.
    pub(in crate::app) fn inline(source: &str, owner: egui::Id) -> Self {
        Self::build(source, Some(owner))
    }
    pub(in crate::app) fn control_ids(&self) -> impl Iterator<Item = egui::Id> + '_ {
        self.controls[..3].iter().flatten().copied()
    }
    fn build(source: &str, inline_owner: Option<egui::Id>) -> Self {
        let mut nav = Self {
            wrap_available: source.len() <= WRAP_MAX_BYTES
                && source.lines().take(WRAP_MAX_ROWS + 1).count() <= WRAP_MAX_ROWS,
            ..Default::default()
        };
        if let Some(owner) = inline_owner {
            nav.focus = owner;
        }
        if source.len() > MAX_BYTES {
            nav.limited = true;
            return nav;
        }
        // Bound once, not in the render/idle loop. No second source body is retained.
        if inline_owner.is_none() {
            use sha2::Digest;
            nav.comment_digest = Some(sha2::Sha256::digest(source.as_bytes()).into());
        }
        for row in changes::lines::rows(source).filter(|row| row.header && row.located) {
            if nav.rows.len() == MAX_HUNKS {
                nav.rows.clear();
                nav.limited = true;
                break;
            }
            nav.rows.push(row.index as u32);
        }
        nav
    }
    fn choose(&mut self, next: usize, source: &str, ctx: &egui::Context) {
        let Some(&row) = self.rows.get(next) else {
            return;
        };
        self.selected = Some(next);
        self.comment_target = None;
        self.target = Some(row);
        self.header = source
            .lines()
            .nth(row as usize)
            .unwrap_or_default()
            .chars()
            .filter(|ch| !ch.is_control())
            .take(160)
            .collect();
        context_capture::clear_source(ctx);
        let mut selection = egui::text_selection::LabelSelectionState::load(ctx);
        selection.clear_selection();
        selection.store(ctx);
        self.focus(ctx);
    }
    fn previous(&self) -> Option<usize> {
        self.selected.and_then(|selected| selected.checked_sub(1))
    }
    fn next(&self) -> Option<usize> {
        let next = self.selected.map_or(0, |selected| selected + 1);
        (next < self.rows.len()).then_some(next)
    }
    fn summary(&self) -> String {
        if self.limited {
            "Hunk navigation unavailable · exceeds 2 MiB / 4,096-hunk limit".into()
        } else if self.rows.is_empty() {
            "No unified hunks · read-only".into()
        } else if let Some(selected) = self.selected {
            format!("Target hunk {} of {}", selected + 1, self.rows.len())
        } else {
            format!("{} hunks · no target chosen", self.rows.len())
        }
    }
    pub fn inputs(&mut self, ctx: &egui::Context, source: &str) {
        // Called before composer/global keys in the app and locally by isolated
        // utility renderers. Popup/modal keys always get first refusal.
        if ctx.memory(|memory| memory.top_modal_layer().is_none()) && !egui::Popup::is_any_open(ctx)
        {
            let mut selected = self.selected;
            let mut requested = None;
            ctx.input_mut(|input| {
                input.events.retain(|event| {
                    let egui::Event::Key {
                        key,
                        pressed: true,
                        modifiers,
                        ..
                    } = event
                    else {
                        return true;
                    };
                    if !modifiers.matches_exact(egui::Modifiers::ALT) {
                        return true;
                    }
                    let target = match key {
                        egui::Key::ArrowUp => selected.and_then(|index| index.checked_sub(1)),
                        egui::Key::ArrowDown => Some(selected.map_or(0, |index| index + 1))
                            .filter(|index| *index < self.rows.len()),
                        egui::Key::Home => (!self.rows.is_empty()).then_some(0),
                        egui::Key::End => self.rows.len().checked_sub(1),
                        _ => return true,
                    };
                    if let Some(index) = target {
                        selected = Some(index);
                        requested = Some(index);
                    }
                    false
                })
            });
            if let Some(index) = requested {
                self.choose(index, source, ctx);
            }
        }
    }
    pub fn show(&mut self, ui: &mut egui::Ui, source: &str) {
        if let Some(preferred) = wrap_preference::get(ui.ctx()) {
            let wrap = preferred && self.wrap_available;
            if self.wrap != wrap {
                self.wrap = wrap;
                context_capture::clear_source(ui.ctx());
                let mut selection = egui::text_selection::LabelSelectionState::load(ui.ctx());
                selection.clear_selection();
                selection.store(ui.ctx());
                self.target = self.comment_target.or_else(|| {
                    self.selected
                        .and_then(|index| self.rows.get(index))
                        .copied()
                });
            }
        }
        if prompt_ui::keyboard_available(ui) && self.owns_focus(ui.ctx()) {
            self.inputs(ui.ctx(), source);
        }
        ui.scope(|ui| {
            ui.spacing_mut().item_spacing.x=3.0;
            let previous = self.previous();
            let next = self.next();
            let before = (!self.rows.is_empty()).then(|| chrome::step_button(ui,chrome::Direction::Up,previous.is_some() && !egui::Popup::is_any_open(ui.ctx()),"Previous hunk · Alt/Option+↑"));
            if let Some(before)=&before { self.register(0,before.id); }
            if before.as_ref().is_some_and(|before|before.clicked())
                && let Some(index) = previous
            {
                self.choose(index, source, ui.ctx());
            }
            let after = (!self.rows.is_empty()).then(|| chrome::step_button(ui,chrome::Direction::Down,next.is_some() && !egui::Popup::is_any_open(ui.ctx()),"Next hunk · Alt/Option+↓"));
            if let Some(after)=&after { self.register(1,after.id); }
            if after.as_ref().is_some_and(|after|after.clicked())
                && let Some(index) = next
            {
                self.choose(index, source, ui.ctx());
            }
            let wrap = ui.add_enabled(self.wrap_available && !egui::Popup::is_any_open(ui.ctx()), egui::Button::new(theme::small(ui,"Wrap")).frame(false).selected(self.wrap))
                .on_hover_text("Wrap lines · remembered for file reviews and expanded Changes rows in this window and after restart. Source and Copy stay unchanged. Large diffs keep the unwrapped fallback.")
                .on_disabled_hover_text(if self.wrap_available { "Close the open menu to change wrapping." } else { "Wrapping unavailable above 64 KiB or 4,096 diff rows. Unwrapped read-only view remains available." });
            wrap.widget_info(||egui::WidgetInfo::selected(egui::WidgetType::Button,wrap.enabled(),self.wrap,"Wrap lines"));
            self.register(2, wrap.id);
            if wrap.clicked() {
                self.wrap = !self.wrap;
                if wrap_preference::get(ui.ctx()).is_some() {
                    wrap_preference::init(ui.ctx(), self.wrap);
                }
                context_capture::clear_source(ui.ctx());
                let mut selection = egui::text_selection::LabelSelectionState::load(ui.ctx());
                selection.clear_selection(); selection.store(ui.ctx());
                self.target = self.comment_target.or_else(|| self.selected.and_then(|index| self.rows.get(index)).copied());
                self.focus(ui.ctx());
            }
            #[cfg(test)]
            ui.ctx().data_mut(|data| { data.insert_temp(egui::Id::new("diff-wrap-lines"), wrap.rect); data.insert_temp(egui::Id::new("diff-wrap-lines-focus"), wrap.id); });
            let summary=if self.limited {"Hunk limit reached".into()} else if let Some(selected)=self.selected {format!("Hunk {} / {}",selected+1,self.rows.len())} else if self.rows.len()==1 {"1 hunk".into()} else {format!("{} hunks",self.rows.len())};
            if self.limited || !self.rows.is_empty() {
                ui.add(egui::Label::new(theme::small(ui,summary)).wrap())
                    .on_hover_text(format!("{}\n{}\nAlt/Option+Home: first · Alt/Option+End: last\nTarget is the last navigation choice, not a live scroll-position tracker.",self.summary(),self.header));
            }
            #[cfg(test)]
            ui.ctx().data_mut(|data| {
                if let Some(before)=before {
                    data.insert_temp(egui::Id::new("diff-previous-hunk"),before.rect);
                    data.insert_temp(egui::Id::new("diff-previous-hunk-focus"),before.id);
                }
                if let Some(after)=after {
                    data.insert_temp(egui::Id::new("diff-next-hunk"),after.rect);
                    data.insert_temp(egui::Id::new("diff-next-hunk-focus"),after.id);
                }
            });
        });
    }
    pub fn wrap(&self) -> bool {
        self.wrap
    }
    pub fn layout_width(&mut self, width: f32) {
        if self.wrap && self.target.is_none() && (self.layout_width - width).abs() > 1.0 {
            self.target = self.comment_target.or_else(|| {
                self.selected
                    .and_then(|index| self.rows.get(index))
                    .copied()
            });
        }
        self.layout_width = width;
    }
    /// Saved-note reveal in this exact immutable document, independent of a
    /// hunk-navigation choice. No lookup, file open, selection or note mutation.
    pub(in crate::app) fn reveal_comment(&mut self, ctx: &egui::Context, row: usize) -> bool {
        if self.comment_digest().is_none() || self.untracked || row > MAX_BYTES {
            return false;
        }
        context_capture::clear_source(ctx);
        let mut selection = egui::text_selection::LabelSelectionState::load(ctx);
        selection.clear_selection();
        selection.store(ctx);
        self.comment_target = Some(row as u32);
        self.target = self.comment_target;
        self.focus(ctx);
        true
    }
    pub fn take_target(&mut self) -> Option<usize> {
        self.target.take().map(|row| row as usize)
    }
}

pub(in crate::app) fn demo_wrap(ctx: &egui::Context) -> Document {
    // Explicit provider-free fixture; demo apps never save preferences.
    wrap_preference::init(ctx, true);
    let source = format!(
        "@@ -1,2 +1,2 @@\n-{}\n+{}\n context unchanged\n@@ -50 +50 @@\n-old second\n+new second\n",
        "Long original value with literal Unicode 世界 · ".repeat(12),
        "Long replacement keeps exact source characters e\u{301} · ".repeat(12)
    );
    let mut document: Document = ("src/wrapped-diff-fixture.rs".into(), source).into();
    document.2.wrap = true;
    document.2.choose(0, &document.1, ctx);
    document.3 = true;
    document
}

pub(in crate::app) fn demo(ctx: &egui::Context) -> Document {
    let source: String = (0..5)
        .map(|n| {
            format!(
                "@@ -{},20 +{},20 @@ fn example_{n}\n-old value {n}\n+new value {n}\n{}",
                n * 50 + 1,
                n * 50 + 1,
                (0..19)
                    .map(|line| format!(" unchanged context {n}-{line}\n"))
                    .collect::<String>()
            )
        })
        .collect();
    let mut document: Document = ("src/hunk-navigation-fixture.rs".into(), source).into();
    document.2.choose(2, &document.1, ctx);
    document.3 = true;
    document
}

#[cfg(test)]
mod safety_tests;
#[cfg(test)]
mod tests;
#[cfg(test)]
mod ui_tests;
#[cfg(test)]
mod wrap_selection_tests;
#[cfg(test)]
mod wrap_tests;
