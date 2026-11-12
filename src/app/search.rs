//! Unified, local Spotlight search; never alters the sidebar's geometry or Pi history.
use super::*;
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};

mod files;
mod index;
#[cfg(test)]
mod tests;

#[derive(Clone, Copy, Default, PartialEq, Eq, Debug)]
pub(super) enum Scope {
    #[default]
    All,
    Chats,
    Messages,
    Settings,
    Files,
}
impl Scope {
    const ALL: [Self; 5] = [
        Self::All,
        Self::Chats,
        Self::Messages,
        Self::Settings,
        Self::Files,
    ];
    fn label(self) -> &'static str {
        match self {
            Self::All => "All",
            Self::Chats => "Chats",
            Self::Messages => "Messages",
            Self::Settings => "Settings",
            Self::Files => "Files",
        }
    }
    fn messages(self) -> bool {
        matches!(self, Self::All | Self::Messages)
    }
}
#[derive(Clone)]
pub(super) struct Candidate {
    pub title: String,
    pub project: PathBuf,
    pub cwd: PathBuf,
    pub path: Option<PathBuf>,
    pub target: sidebar_tree::Target,
}
#[derive(Clone, Copy, PartialEq, Eq)]
enum Kind {
    Chat,
    Message,
    Setting,
    File,
}
#[derive(Clone)]
struct Hit {
    candidate: Candidate,
    detail: String,
    kind: Kind,
    message: Option<Locator>,
}
#[derive(Clone)]
struct Locator {
    role: String,
    timestamp: i64,
    fingerprint: u64,
}
struct Jump {
    path: Option<PathBuf>,
    agent: String,
    target: sidebar_tree::Target,
    message: Locator,
}
#[derive(Default)]
pub(super) struct Search {
    scope: Scope,
    selected: usize,
    hits: Vec<Hit>,
    messages: index::Found,
    files: files::Files,
    rx: Option<Receiver<index::Found>>,
    cancel: Arc<AtomicBool>,
    requested: Option<(String, Scope, u64)>,
    changed_at: f64,
    source_revision: u64,
    source_changed_at: f64,
    jump: Option<Jump>,
    pub(super) notice: Option<String>,
}
impl Search {
    fn cancel(&mut self) {
        self.files.cancel();
        self.cancel.store(true, Ordering::Relaxed);
        self.rx = None;
        self.requested = None;
    }
}
impl Drop for Search {
    fn drop(&mut self) {
        self.cancel.store(true, Ordering::Relaxed);
    }
}

/// One 30pt line for every result type. Metadata stays inline, never under
/// the title; full title/location and message excerpt remain available on hover.
fn result_row(ui: &mut egui::Ui, hit: &Hit, selected: bool) -> egui::Response {
    let colors = theme::palette(ui.ctx());
    let (rect, response) =
        ui.allocate_exact_size(egui::vec2(ui.available_width(), 30.0), Sense::click());
    if selected || response.hovered() {
        ui.painter().rect_filled(
            rect,
            5,
            if selected {
                colors.accent_bg
            } else {
                colors.hover
            },
        );
    }
    let painter = ui.painter().with_clip_rect(rect.intersect(ui.clip_rect()));
    let center = rect.center().y;
    icons::draw(
        ui,
        egui::Rect::from_center_size(egui::pos2(rect.left() + 15.0, center), Vec2::splat(14.0)),
        if hit.kind == Kind::Setting {
            Icon::Sliders
        } else {
            Icon::File
        },
        colors.muted,
    );
    let left = rect.left() + 30.0;
    let right = rect.right() - 9.0;
    let available = (right - left).max(1.0);
    let metadata = match hit.kind {
        Kind::Setting => hit
            .detail
            .strip_prefix("Settings · ")
            .unwrap_or("Settings")
            .to_owned(),
        Kind::Chat => sessions::project_name(&hit.candidate.project),
        Kind::File => "File".into(),
        Kind::Message => format!(
            "Message · {}",
            sessions::project_name(&hit.candidate.project)
        ),
    };
    // Page-level settings already name their destination: don't print
    // “General · General” just to fill a metadata column.
    let metadata = if hit.kind == Kind::Setting && metadata == hit.candidate.title {
        "Settings".into()
    } else {
        metadata
    };
    let mut meta_job = egui::text::LayoutJob::simple(
        metadata,
        FontId::proportional(10.5),
        colors.dim,
        (available * 0.30).min(140.0),
    );
    meta_job.wrap.max_rows = 1;
    let meta = painter.layout_job(meta_job);
    let title_width = (available - meta.size().x - 14.0).max(1.0);
    let mut job = egui::text::LayoutJob::default();
    let format = egui::TextFormat {
        font_id: FontId::proportional(12.0),
        color: colors.text,
        ..Default::default()
    };
    job.append(&hit.candidate.title, 0.0, format);
    if hit.kind == Kind::Message {
        job.append(
            &format!(" — {}", hit.detail),
            0.0,
            egui::TextFormat {
                font_id: FontId::proportional(11.5),
                color: colors.muted,
                ..Default::default()
            },
        );
    }
    job.wrap.max_width = title_width;
    job.wrap.max_rows = 1;
    job.wrap.break_anywhere = true;
    let title = painter.layout_job(job);
    painter.galley(
        egui::pos2(left, center - title.size().y / 2.0),
        title,
        colors.text,
    );
    painter.galley(
        egui::pos2(right - meta.size().x, center - meta.size().y / 2.0),
        meta,
        colors.dim,
    );
    let tooltip = format!("{}\n{}", hit.candidate.title, hit.detail);
    response
        .on_hover_cursor(egui::CursorIcon::PointingHand)
        .on_hover_text(tooltip)
}

pub(super) fn text_fingerprint(text: &str) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    text.chars()
        .take(64 * 1024)
        .collect::<String>()
        .hash(&mut hasher);
    hasher.finish()
}

impl Pui {
    pub(super) fn search_blocked(&self) -> bool {
        self.agents.close_prompt
            || self.tasks.attachment_confirming()
            || self.section_view.confirming()
            || self.terminals.confirming_stop()
            || self.history_view.is_open()
            || self.deletion.confirming()
            || self.deletion.in_progress()
            || self.automation.confirming()
            || self.menu.about
            || self.updates.open
            || self.bypass_pending.is_some()
            || !self.state.dialogs.is_empty()
            || self.tasks.search_dialogs_pending()
            || self.transcript.gallery.is_open()
            || self.draft_images.gallery.is_open()
            || (self.settings_open && self.settings_view.navigation_blocked())
    }
    pub(super) fn open_search(&mut self) {
        if self.search_blocked() {
            return;
        }
        self.search_open = true;
        // Claim keyboard ownership before composer/terminal render this frame.
        self.ctx
            .memory_mut(|m| m.request_focus(egui::Id::new("spotlight-query")));
        self.focus_search = true;
        self.focus_composer = false;
        self.close_model_popovers();
        self.spotlight.cancel();
        self.spotlight.messages = Default::default();
        self.spotlight.selected = 0;
        self.spotlight.notice = None;
        self.spotlight.changed_at = self.ctx.input(|i| i.time) - 1.0;
        self.files.hide_web();
        egui::Popup::close_all(&self.ctx);
    }
    fn close_search(&mut self) {
        self.search_open = false;
        self.focus_search = false;
        self.search.clear();
        self.spotlight.cancel();
        self.spotlight.hits.clear();
        self.spotlight.messages = Default::default();
        self.ctx
            .memory_mut(|m| m.surrender_focus(egui::Id::new("spotlight-query")));
        self.ctx.request_repaint();
    }
    pub(super) fn search_inputs(&mut self, ctx: &egui::Context) {
        if ctx.input_mut(|input| input.consume_key(egui::Modifiers::COMMAND, egui::Key::P)) {
            self.open_quick_files();
        }
        let shortcut = ctx.input_mut(|i| i.consume_key(egui::Modifiers::COMMAND, egui::Key::K));
        if shortcut || (self.focus_search && !self.search_open) {
            self.open_search();
        }
        if self.search_open
            && !self.search_blocked()
            && ctx.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::Escape))
        {
            self.close_search();
        }
    }
    fn search_candidates(&self) -> Vec<Candidate> {
        fn collect(branch: sidebar_tree::Branch, out: &mut Vec<Candidate>) {
            if !branch.hidden {
                out.push(Candidate {
                    title: branch.title,
                    project: branch.project,
                    cwd: branch.cwd,
                    path: branch.path,
                    target: branch.target,
                });
            }
            // Hidden ancestors do not hide independently visible child chats.
            for child in branch.children {
                collect(child, out);
            }
        }
        let mut out = Vec::new();
        for branch in sidebar_tree::forest(self) {
            collect(branch, &mut out);
        }
        if !out.iter().any(|c| {
            c.path.as_ref().is_some_and(|p| {
                self.state
                    .session_file
                    .as_deref()
                    .is_some_and(|s| sync::identity(p) == sync::identity(std::path::Path::new(s)))
            })
        }) && (self.state.session_file.is_some() || !self.state.messages.is_empty())
            && self
                .state
                .session_file
                .as_ref()
                .and_then(|s| self.prefs.chats.get(std::path::Path::new(s)))
                .is_none_or(|meta| !meta.hidden)
        {
            out.insert(
                0,
                Candidate {
                    title: self
                        .state
                        .chat_title()
                        .unwrap_or_else(|| "Current conversation".into()),
                    project: self.prefs.cwd.clone(),
                    cwd: self.prefs.cwd.clone(),
                    path: self.state.session_file.as_ref().map(PathBuf::from),
                    target: sidebar_tree::Target::Agent(self.agents.active.clone()),
                },
            );
        }
        out.truncate(512);
        out
    }
    fn search_sources(&self, candidates: Vec<Candidate>) -> Vec<index::Source> {
        // Snapshot only a bounded text tail on the UI thread; parsing/matching
        // runs on the worker. No tools, attachments, or full State clones.
        let mut budget = 8 * 1024 * 1024;
        candidates.into_iter().filter_map(|candidate| {
            let same = |state: &State| candidate.path.as_ref().is_some_and(|path| state.session_file.as_deref().is_some_and(|s| sync::identity(path) == sync::identity(std::path::Path::new(s))));
            let active = same(&self.state) || matches!(&candidate.target, sidebar_tree::Target::Agent(id) if id == &self.agents.active);
            let state = if active { Some(&self.state) } else {
                self.agents.parked.iter().find(|(id, chat)| same(&chat.state) || matches!(&candidate.target, sidebar_tree::Target::Agent(target) if target == *id)).and_then(|(_, chat)| chat.search_state())
            };
            let state = match &candidate.target {
                sidebar_tree::Target::Task(_, Some(run)) => self.tasks.search_state(run).or(state),
                _ => state,
            };
            // Never guess a different window's in-memory selected leaf.
            if candidate.path.as_ref().is_some_and(|p| self.other_window_status(p).is_some()) && state.is_none() { return None; }
            let mut limited = false;
            // Demo mode never reads real sessions. A loaded empty branch stays
            // empty; do not fall back to disk merely because it has no matches.
            let live = state.map(|state| {
                let mut messages = Vec::new();
                let mut local = 2 * 1024 * 1024;
                for message in state.messages.iter().rev().filter(|m| matches!(m.role.as_str(), "user" | "assistant")) {
                    let cost = message.text.len().min(256 * 1024);
                    if cost > budget || cost > local || messages.len() >= 1024 { limited = true; break; }
                    budget -= cost;
                    local -= cost;
                    limited |= message.text.chars().count() > 64 * 1024;
                    if let Some(text) = index::Text::from_message(message) { messages.push(text); }
                }
                messages.reverse();
                messages
            }).or_else(|| self.demo.then(Vec::new));
            Some(index::Source { candidate, live, limited })
        }).collect()
    }
    fn refresh_search(&mut self, ctx: &egui::Context) {
        if self.spotlight.scope == Scope::Files {
            self.spotlight.cancel.store(true, Ordering::Relaxed);
            self.spotlight.rx = None;
            self.spotlight.requested = None;
            self.spotlight.messages = Default::default();
            self.refresh_quick_files(ctx);
            return;
        }
        self.spotlight.files.cancel();
        let query = self.search.trim().to_lowercase();
        let scope = self.spotlight.scope;
        let revision = self.search_revision();
        let now = ctx.input(|i| i.time);
        if self.spotlight.source_revision != revision {
            self.spotlight.source_revision = revision;
            self.spotlight.source_changed_at = now;
        }
        let key = (query.clone(), scope, revision);
        if self.spotlight.requested.as_ref() != Some(&key) {
            self.spotlight.cancel();
            self.spotlight.messages = Default::default();
            if query.is_empty() || !scope.messages() {
                self.spotlight.requested = Some(key);
            } else if now - self.spotlight.changed_at >= 0.15
                && now - self.spotlight.source_changed_at >= 0.15
            {
                self.spotlight.requested = Some(key);
                self.spotlight.cancel = Arc::new(AtomicBool::new(false));
                let cancel = self.spotlight.cancel.clone();
                let sources = self.search_sources(self.search_candidates());
                let (tx, rx) = mpsc::channel();
                self.spotlight.rx = Some(rx);
                let ctx = ctx.clone();
                std::thread::spawn(move || {
                    let found = index::search(sources, &query, &cancel);
                    if !cancel.load(Ordering::Relaxed) {
                        let _ = tx.send(found);
                        ctx.request_repaint();
                    }
                });
            } else {
                ctx.request_repaint_after(Duration::from_millis(160));
            }
        }
        if let Some(found) = self.spotlight.rx.as_ref().and_then(|rx| rx.try_recv().ok()) {
            self.spotlight.messages = found;
            self.spotlight.rx = None;
        }
        self.build_search_hits();
    }
    fn search_revision(&self) -> u64 {
        use std::hash::{Hash, Hasher};
        let mut h = std::collections::hash_map::DefaultHasher::new();
        self.state.transcript_revision.hash(&mut h);
        self.state.prose_revision.hash(&mut h);
        self.state.messages.len().hash(&mut h);
        for (id, chat) in &self.agents.parked {
            id.hash(&mut h);
            chat.state.transcript_revision.hash(&mut h);
            chat.state.prose_revision.hash(&mut h);
            chat.state.messages.len().hash(&mut h);
        }
        for session in &self.sessions {
            session.path.hash(&mut h);
            session.modified.hash(&mut h);
        }
        for candidate in self.search_candidates() {
            candidate.title.hash(&mut h);
            candidate.path.hash(&mut h);
            candidate
                .path
                .as_deref()
                .and_then(|p| self.other_window_status(p))
                .hash(&mut h);
        }
        self.tasks.search_revision().hash(&mut h);
        self.deletion.in_progress().hash(&mut h);
        h.finish()
    }
    fn build_search_hits(&mut self) {
        let query = self.search.trim().to_lowercase();
        let scope = self.spotlight.scope;
        let mut hits = Vec::new();
        if matches!(scope, Scope::All | Scope::Chats) {
            for candidate in self.search_candidates() {
                if query.is_empty()
                    || index::matches(
                        &format!("{} {}", candidate.title, candidate.project.display()),
                        &query,
                    )
                {
                    let detail = format!("Chat · {}", sessions::project_name(&candidate.project));
                    hits.push(Hit {
                        candidate,
                        detail,
                        kind: Kind::Chat,
                        message: None,
                    });
                    if hits.len() >= 20 {
                        break;
                    }
                }
            }
        }
        if matches!(scope, Scope::All | Scope::Settings) {
            for entry in settings::search_entries() {
                if query.is_empty()
                    || index::matches(
                        &format!("{} {} {}", entry.title, entry.page, entry.keywords),
                        &query,
                    )
                {
                    hits.push(Hit {
                        candidate: Candidate {
                            title: entry.title.into(),
                            project: PathBuf::new(),
                            cwd: PathBuf::new(),
                            path: None,
                            target: sidebar_tree::Target::Saved(PathBuf::new()),
                        },
                        detail: format!("Settings · {}", entry.page),
                        kind: Kind::Setting,
                        message: None,
                    });
                }
            }
        }
        if scope.messages() && !query.is_empty() {
            hits.extend(self.spotlight.messages.hits.iter().cloned());
        }
        self.spotlight.hits = hits;
        self.spotlight.selected = self
            .spotlight
            .selected
            .min(self.spotlight.hits.len().saturating_sub(1));
    }
    pub(super) fn spotlight(&mut self, ctx: &egui::Context) {
        if !self.search_open || self.search_blocked() {
            return;
        }
        self.refresh_search(ctx);
        let mut activate = None;
        let mut close = false;
        let response = prompt_ui::modal(ctx, egui::Id::new("spotlight-search"), 560.0, |ui| {
            let colors = theme::palette(ctx);
            close |= prompt_ui::close(ui);
            prompt_ui::heading(
                ui,
                if self.spotlight.scope == Scope::Files {
                    "Quick Open"
                } else {
                    "Search"
                },
                "",
            );
            ui.add_space(4.0);
            // Reserve navigation keys before TextEdit can treat them as caret
            // movement or submit them to a previously focused control.
            let up = ui.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::ArrowUp));
            let down = ui.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::ArrowDown));
            let enter = ui.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::Enter));
            ui.horizontal(|ui| {
                icons::inline(ui, Icon::Search, colors.muted);
                let width = ui.available_width();
                let input = ui.add_sized(
                    [width, 32.0],
                    egui::TextEdit::singleline(&mut self.search)
                        .id(egui::Id::new("spotlight-query"))
                        .hint_text(if self.spotlight.scope == Scope::Files {
                            "Workspace path, optionally :line"
                        } else {
                            "Search everything…"
                        })
                        .char_limit(256)
                        .font(FontId::proportional(15.0))
                        .frame(false),
                );
                if self.focus_search {
                    input.request_focus();
                    self.focus_search = false;
                }
                if input.changed() {
                    self.spotlight.changed_at = ui.input(|i| i.time);
                    self.spotlight.selected = 0;
                    self.refresh_search(ctx);
                }
            });
            ui.separator();
            ui.horizontal_wrapped(|ui| {
                ui.spacing_mut().item_spacing.x = 2.0;
                for scope in Scope::ALL {
                    if theme::choice(ui, self.spotlight.scope == scope, scope.label()).clicked() {
                        self.spotlight.scope = scope;
                        self.spotlight.selected = 0;
                        self.refresh_search(ctx);
                    }
                }
            });
            let mut moved = false;
            if prompt_ui::keyboard_available(ui) {
                for (pressed, forward) in [(down, true), (up, false)] {
                    if pressed && !self.spotlight.hits.is_empty() {
                        let count = self.spotlight.hits.len();
                        self.spotlight.selected =
                            (self.spotlight.selected + if forward { 1 } else { count - 1 }) % count;
                        moved = true;
                    }
                }
                if enter {
                    activate = self.spotlight.hits.get(self.spotlight.selected).cloned();
                }
            }
            let max_height = (ctx.screen_rect().height() - 310.0).clamp(70.0, 330.0);
            egui::ScrollArea::vertical()
                .id_salt("spotlight-results")
                .max_height(max_height)
                .auto_shrink([false, true])
                .show(ui, |ui| {
                    ui.spacing_mut().item_spacing.y = 2.0;
                    if self.spotlight.hits.is_empty() {
                        ui.add(
                            egui::Label::new(theme::small(
                                ui,
                                if self.search.trim().is_empty()
                                    && self.spotlight.scope == Scope::Messages
                                {
                                    "Type to search conversation text."
                                } else if self.spotlight.scope == Scope::Files
                                    && self.spotlight.files.pending()
                                {
                                    "Finding workspace filenames…"
                                } else if self.spotlight.rx.is_some() {
                                    "Searching messages…"
                                } else {
                                    "No results. Try another name, phrase or setting."
                                },
                            ))
                            .wrap(),
                        );
                    }
                    for (index, hit) in self.spotlight.hits.iter().enumerate() {
                        let selected = self.spotlight.selected == index;
                        let response = result_row(ui, hit, selected);
                        #[cfg(test)]
                        ctx.data_mut(|data| {
                            data.insert_temp(egui::Id::new(("spotlight-row", index)), response.rect)
                        });
                        if moved && selected {
                            response.scroll_to_me(Some(Align::Center));
                        }
                        response.widget_info(|| {
                            egui::WidgetInfo::labeled(
                                egui::WidgetType::Button,
                                true,
                                &hit.candidate.title,
                            )
                        });
                        if response.clicked() {
                            activate = Some(hit.clone());
                        }
                    }
                });
            if self.spotlight.scope == Scope::Files {
                ui.add(
                    egui::Label::new(theme::small(
                        ui,
                        "Filenames only · workspace scope · Enter opens read-only",
                    ))
                    .wrap(),
                );
                if let Some(note) = self.spotlight.files.note() {
                    ui.add(egui::Label::new(theme::small(ui, note)).wrap());
                }
            }
            if self.spotlight.rx.is_some() {
                ui.label(theme::small(ui, "Searching saved messages…"));
            }
            if self.spotlight.messages.limited || self.spotlight.messages.unreadable > 0 {
                ui.add(egui::Label::new(theme::small(ui, "Some history was unavailable or reached the search limit. Refine your query.")).wrap());
            }
            if let Some(notice) = &self.spotlight.notice {
                ui.add(egui::Label::new(theme::small(ui, notice)).wrap());
            }
            ui.separator();
            prompt_ui::footer(ui, |ui| {
                if prompt_ui::button(ui, "Close", prompt_ui::Button::Secondary, true).clicked() {
                    close = true;
                }
                ui.add(
                    egui::Label::new(theme::small(ui, "↑ ↓ select · Enter open · Esc close"))
                        .truncate(),
                );
            });
        });
        if response.should_close() || close {
            self.close_search();
        } else if let Some(hit) = activate {
            self.activate_search_hit(hit);
        }
    }
    fn activate_search_hit(&mut self, hit: Hit) {
        if self.search_blocked() {
            return;
        }
        if hit.kind == Kind::File {
            self.activate_quick_file(&hit);
            return;
        }
        if hit.kind != Kind::Setting
            && !self.search_candidates().iter().any(|candidate| {
                candidate.target == hit.candidate.target && candidate.path == hit.candidate.path
            })
        {
            self.spotlight.notice =
                Some("That conversation was hidden, deleted or is no longer available.".into());
            return;
        }
        if hit.kind == Kind::Setting {
            if let Some(entry) = settings::search_entries()
                .into_iter()
                .find(|entry| entry.title == hit.candidate.title)
            {
                self.remember_navigation();
                self.settings_view.select(entry.page);
                self.settings_open = true;
                self.close_search();
                self.remember_navigation();
            }
            return;
        }
        if self.open_search_candidate(&hit.candidate) {
            if let Some(message) = hit.message {
                self.spotlight.jump = Some(Jump {
                    path: hit.candidate.path,
                    agent: self.agents.active.clone(),
                    target: hit.candidate.target,
                    message,
                });
            }
            self.close_search();
        } else {
            self.spotlight.notice = Some("That conversation is no longer available.".into());
        }
    }
    pub(super) fn search_jump(&mut self) {
        let Some(jump) = &self.spotlight.jump else {
            return;
        };
        if let sidebar_tree::Target::Task(task, Some(run)) = &jump.target {
            if self.tasks.search_jump(
                task,
                run,
                &jump.message.role,
                jump.message.timestamp,
                jump.message.fingerprint,
            ) {
                self.spotlight.jump = None;
            }
            return;
        }
        let same = match &jump.path {
            Some(path) => {
                self.pending_session.as_ref() == Some(path)
                    || self.state.session_file.as_deref().is_some_and(|p| {
                        sync::identity(path) == sync::identity(std::path::Path::new(p))
                    })
            }
            None => self.agents.active == jump.agent,
        };
        if !same {
            self.spotlight.jump = None;
            return;
        }
        // Wait for canonical full history, not an incomplete preview index.
        if self.loading_chat {
            return;
        }
        let matches: Vec<_> = self
            .state
            .messages
            .iter()
            .enumerate()
            .filter(|(_, m)| {
                m.role == jump.message.role
                    && m.timestamp == jump.message.timestamp
                    && text_fingerprint(&m.text) == jump.message.fingerprint
            })
            .map(|(index, _)| index)
            .take(2)
            .collect();
        if matches.len() == 1 {
            self.transcript.jump_to_message(matches[0]);
        } else {
            self.state.notice =
                Some("The matching passage is not on this chat's current branch.".into());
        }
        self.spotlight.jump = None;
    }
}
