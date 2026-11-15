//! Expanded Changes rows share the embedded review's bounded navigation and renderer.
//! No bodies/digests are retained here, and no work is started for closed/capacity rows.
use super::*;

const MAX_OPEN_ROWS: usize = 32;
const MAX_INLINE_PATH: usize = 4096;
type Key = (bool, String); // true = Current Git, false = Recorded edits

#[derive(Default)]
struct Row {
    navigation: Option<diff_viewer::Navigation>,
}
#[derive(Default)]
pub(super) struct Inline {
    rows: BTreeMap<Key, Row>,
    revision: Option<u64>,
    git_bound: bool,
    generation: u64,
    header_owner: Option<egui::Id>,
    keyboard_pass: Option<u64>,
    source_pass: Option<u64>,
}
fn keyboard_event(event: &egui::Event) -> bool {
    matches!(
        event,
        egui::Event::Key { .. }
            | egui::Event::Text(_)
            | egui::Event::Paste(_)
            | egui::Event::Copy
            | egui::Event::Cut
            | egui::Event::Ime(_)
    )
}
fn available(ctx: &egui::Context) -> bool {
    !egui::Popup::is_any_open(ctx) && ctx.memory(|memory| memory.top_modal_layer().is_none())
}
fn pointer_owns(ctx: &egui::Context, id: egui::Id) -> bool {
    ctx.read_response(id).is_some_and(|response| {
        let (pressed, released, pos) = ctx.input(|input| {
            (
                input.pointer.button_pressed(egui::PointerButton::Primary),
                input.pointer.button_released(egui::PointerButton::Primary),
                input.pointer.interact_pos(),
            )
        });
        response.enabled()
            && (response.is_pointer_button_down_on()
                || (pressed || released)
                    && pos.is_some_and(|pos| {
                        response.interact_rect.contains(pos)
                            && ctx.layer_id_at(pos) == Some(response.layer_id)
                    }))
    })
}
impl Inline {
    pub(super) fn clear(&mut self) {
        self.rows.clear();
        self.generation = self.generation.wrapping_add(1);
        self.git_bound = false;
    }
    pub(super) fn close(&mut self, key: &Key) {
        self.rows.remove(key);
    }
    pub(super) fn clear_selection(ctx: &egui::Context) {
        clear_selection(ctx);
    }
    pub(super) fn reset_git(&mut self, path: &str) {
        self.rows.remove(&(true, path.to_owned()));
    }
    pub(super) fn owns_keyboard(&self, ctx: &egui::Context) -> bool {
        self.keyboard_pass == Some(ctx.cumulative_pass_nr())
            || self.source_pass == Some(ctx.cumulative_pass_nr())
    }
}
impl Changes {
    pub(in crate::app) fn demo_inline(&mut self, cwd: &std::path::Path, owner: &str) {
        self.bind(cwd, owner);
        self.open_rows
            .insert((false, "src/synthetic-inline.rs".into()));
    }
    pub(super) fn inline_capacity(&self, ui: &mut egui::Ui) {
        if self.open_rows.len() >= MAX_OPEN_ROWS {
            egui::Frame::new().inner_margin(12).show(ui, |ui| {
                ui.add(
                    egui::Label::new(theme::small(ui, "Close another expanded file (32 max)."))
                        .wrap(),
                );
            });
        }
    }
    pub(super) fn inline_header_input(&mut self, ctx: &egui::Context, response: &egui::Response) {
        if available(ctx)
            && response.enabled()
            && (response.has_focus() || pointer_owns(ctx, response.id))
        {
            self.inline.keyboard_pass = Some(ctx.cumulative_pass_nr());
            self.inline.header_owner = response.has_focus().then_some(response.id);
            if response.has_focus() {
                ctx.memory_mut(|memory| {
                    memory.set_focus_lock_filter_current(
                        response.id,
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
    }
    pub(super) fn can_expand_inline(&self, path: &str) -> bool {
        self.open_rows.len() < MAX_OPEN_ROWS && path.len() <= MAX_INLINE_PATH
    }
    pub(super) fn inline_header_id(&self, git: bool, path: &str) -> egui::Id {
        egui::Id::new(("inline-change-row", &self.identity, git, path))
    }
    pub(in crate::app) fn bind_inline_owner(
        &mut self,
        ctx: &egui::Context,
        cwd: &std::path::Path,
        session: &str,
    ) {
        if self.identity.0 != cwd || self.identity.1 != session {
            self.close_branches(ctx);
            clear_selection(ctx);
            self.bind(cwd, session);
        }
    }
    pub(super) fn bind_inline_git(&mut self, workspace: &Workspace) {
        if self.inline.git_bound {
            return;
        }
        self.inline.git_bound = true;
        self.open_rows
            .retain(|(git, path)| !*git || workspace.files.iter().any(|file| file.path == *path));
    }
    pub(super) fn bind_inline_revision(
        &mut self,
        ctx: &egui::Context,
        outputs: &[RecordedFile],
        revision: u64,
    ) {
        if self.inline.revision == Some(revision) {
            return;
        }
        self.inline.revision = Some(revision);
        let had_recorded = self.inline.rows.keys().any(|(git, _)| !git);
        self.inline.rows.retain(|(git, _), _| *git);
        self.open_rows
            .retain(|(git, path)| *git || outputs.iter().any(|file| file.path == *path));
        if had_recorded {
            clear_selection(ctx);
        }
    }
    /// Before globals: only the visible, exact row's actual native owner may claim input.
    pub(super) fn inline_inputs(
        &mut self,
        ctx: &egui::Context,
        outputs: &[RecordedFile],
        revision: u64,
        workspace: &Workspace,
    ) {
        self.bind_inline_revision(ctx, outputs, revision);
        self.bind_inline_git(workspace);
        if self.review.is_some() || !available(ctx) {
            return;
        }
        let mut close = None;
        let git = self.source == Source::Git;
        let query = self.query.trim().to_lowercase();
        // Closed row triggers are native controls too. Only inspect the bounded
        // response owners; no body read/hash or full file-list pass while idle.
        let relevant_input = ctx.input(|input| {
            input.pointer.any_pressed()
                || input.pointer.any_released()
                || input
                    .events
                    .iter()
                    .any(|event| matches!(event, egui::Event::Key { .. }))
        });
        let paths: Box<dyn Iterator<Item = &str> + '_> = if !relevant_input {
            Box::new(std::iter::empty())
        } else if git {
            Box::new(workspace.files.iter().map(|file| file.path.as_str()))
        } else {
            Box::new(outputs.iter().map(|file| file.path.as_str()))
        };
        for path in paths.filter(|path| path.to_lowercase().contains(&query)) {
            let id = self.inline_header_id(git, path);
            if (self.open_rows.contains(&(git, path.to_owned())) || self.can_expand_inline(path))
                && ctx
                    .read_response(id)
                    .is_some_and(|response| response.enabled())
                && (ctx.memory(|memory| memory.has_focus(id) || memory.focus_lost_by_escape(id))
                    || pointer_owns(ctx, id))
            {
                guard_activation(ctx);
                if !self.open_rows.contains(&(git, path.to_owned())) {
                    let escaped = ctx.input_mut(|input| {
                        input.consume_key(egui::Modifiers::NONE, egui::Key::Escape)
                    });
                    if escaped {
                        self.inline.keyboard_pass = Some(ctx.cumulative_pass_nr());
                    }
                }
            }
        }
        for key in self
            .open_rows
            .iter()
            .filter(|(source, path)| *source == git && path.to_lowercase().contains(&query))
        {
            let header = self.inline_header_id(git, &key.1);
            let nav = self
                .inline
                .rows
                .get(key)
                .and_then(|row| row.navigation.as_ref());
            let controls: Vec<_> = std::iter::once(header)
                .chain(nav.into_iter().flat_map(|nav| nav.control_ids()))
                .chain(
                    nav.and_then(|nav| nav.selection_response(ctx))
                        .map(|response| response.id),
                )
                .chain(nav.map(|nav| nav.focus_id()))
                .collect();
            let escape = ctx.input(|input| input.key_pressed(egui::Key::Escape));
            let focused = ctx.memory(|memory| memory.focused());
            let deferred = focused.is_none() && escape;
            let owns = if deferred {
                controls
                    .iter()
                    .any(|id| ctx.memory(|memory| memory.focus_lost_by_escape(*id)))
            } else {
                nav.is_some_and(|nav| nav.owns_focus(ctx))
                    || controls.iter().any(|id| {
                        ctx.memory(|memory| {
                            memory.has_focus(*id)
                                || focused.is_none() && escape && memory.focus_lost_by_escape(*id)
                        })
                    })
            };
            let pointer = controls.iter().any(|id| pointer_owns(ctx, *id))
                || nav
                    .and_then(|nav| nav.selection_response(ctx))
                    .is_some_and(|response| pointer_owns(ctx, response.id))
                || nav.is_some_and(|nav| pointer_owns(ctx, nav.focus_id()));
            if pointer
                || controls.iter().any(|id| {
                    ctx.read_response(*id)
                        .is_some_and(|response| response.enabled() && response.has_focus())
                })
            {
                self.inline.keyboard_pass = Some(ctx.cumulative_pass_nr());
            }
            if !owns && !pointer {
                continue;
            }
            // Leave unmodified activation and Tab to native widgets. Strip modified
            // activation on both key phases so it cannot synthesize a button click.
            let mut escape_pressed = false;
            guard_activation(ctx);
            ctx.input_mut(|input| {
                input.events.retain(|event| {
                    if let egui::Event::Key {
                        key: egui::Key::Escape,
                        pressed,
                        ..
                    } = event
                        && owns
                    {
                        escape_pressed |= *pressed;
                        return false;
                    }
                    true
                })
            });
            if escape_pressed {
                close = Some((key.clone(), header));
                break;
            }
            if owns
                && let Some(nav) = self
                    .inline
                    .rows
                    .get_mut(key)
                    .and_then(|row| row.navigation.as_mut())
            {
                nav.inputs(
                    ctx,
                    inline_source(outputs, &self.diffs, key).unwrap_or_default(),
                );
            }
        }
        if let Some((key, header)) = close {
            self.open_rows.remove(&key);
            self.inline.close(&key);
            if key.0 {
                self.diffs.remove(&key.1);
            }
            clear_selection(ctx);
            ctx.memory_mut(|memory| {
                memory.request_focus_with_filter(
                    header,
                    egui::EventFilter {
                        escape: true,
                        vertical_arrows: true,
                        horizontal_arrows: true,
                        ..Default::default()
                    },
                );
            });
            self.inline.keyboard_pass = Some(ctx.cumulative_pass_nr());
        }
    }
    /// Shared renderer/chrome only: no duplicate diff body or inline comment feature.
    pub(super) fn inline_diff(
        ui: &mut egui::Ui,
        inline: &mut Inline,
        identity: &(PathBuf, String),
        path: &str,
        source: &str,
        recorded: bool,
    ) {
        if source.trim().is_empty() {
            ui.label(theme::small(ui, "No changes to display."));
            return;
        }
        let key = (!recorded, path.to_owned());
        let owner = egui::Id::new((
            "inline-diff-owner",
            identity,
            &key,
            inline.generation,
            recorded.then_some(inline.revision),
        ));
        let row = inline.rows.entry(key).or_default();
        let navigation = row
            .navigation
            .get_or_insert_with(|| diff_viewer::Navigation::inline(source, owner));
        if available(ui.ctx()) {
            navigation.restore_label_focus(ui.ctx());
        }
        ui.push_id(owner, |ui| {
            ui.spacing_mut().item_spacing.y = 2.0;
            ui.horizontal_wrapped(|ui| {
                ui.spacing_mut().interact_size.y = 24.0;
                ui.add_enabled_ui(available(ui.ctx()), |ui| navigation.show(ui, source));
            });
            navigation.layout_width(ui.available_width());
            let id = numbered_diff_target(
                ui,
                path,
                source,
                recorded,
                navigation.take_target(),
                navigation.wrap(),
            );
            navigation.register(5, id);
            if let Some(response) = ui.ctx().read_response(id) {
                let focus = ui.interact(response.rect, owner, Sense::focusable_noninteractive());
                if pointer_owns(ui.ctx(), id)
                    && available(ui.ctx())
                    && prompt_ui::keyboard_available(ui)
                {
                    focus.ctx.memory_mut(|memory| {
                        memory.request_focus_with_filter(
                            focus.id,
                            egui::EventFilter {
                                escape: true,
                                horizontal_arrows: true,
                                vertical_arrows: true,
                                ..Default::default()
                            },
                        )
                    });
                }
            }
        });
        navigation.finish_focus(ui.ctx());
    }
    /// After native Tab/button processing and before agent questions. Never strip a
    /// code label's copy/selection input; quarantine only actual controls/activation.
    pub(super) fn finish_inline_inputs(&mut self, ctx: &egui::Context) {
        if self.review.is_some() || !available(ctx) {
            return;
        }
        let git = self.source == Source::Git;
        let query = self.query.trim().to_lowercase();
        let mut controls = Vec::new();
        for key in self
            .open_rows
            .iter()
            .filter(|(source, path)| *source == git && path.to_lowercase().contains(&query))
        {
            controls.push(self.inline_header_id(git, &key.1));
            if let Some(nav) = self
                .inline
                .rows
                .get(key)
                .and_then(|row| row.navigation.as_ref())
            {
                controls.extend(nav.control_ids());
                if nav.owns_focus(ctx) {
                    self.inline.source_pass = Some(ctx.cumulative_pass_nr());
                    if let Some(id) = ctx.memory(|memory| memory.focused()) {
                        ctx.memory_mut(|memory| {
                            memory.set_focus_lock_filter_current(
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
            }
        }
        if let Some(header) = self.inline.header_owner.take()
            && ctx
                .read_response(header)
                .is_some_and(|response| response.enabled() && response.has_focus())
        {
            controls.push(header);
        }
        let mut seen = std::collections::HashSet::new();
        controls.retain(|id| {
            seen.insert(*id)
                && ctx
                    .read_response(*id)
                    .is_some_and(|response| response.enabled() && response.sense.is_focusable())
        });
        let focused = ctx.memory(|memory| memory.focused());
        let owned = self.inline.keyboard_pass == Some(ctx.cumulative_pass_nr())
            || self.inline.source_pass == Some(ctx.cumulative_pass_nr())
            || controls.iter().any(|id| {
                ctx.read_response(*id).is_some_and(|response| {
                    response.enabled() && (response.has_focus() || pointer_owns(ctx, *id))
                })
            });
        if owned {
            self.inline.keyboard_pass = Some(ctx.cumulative_pass_nr());
            if let Some(id) = focused.filter(|id| controls.contains(id)) {
                ctx.memory_mut(|memory| {
                    memory.set_focus_lock_filter_current(
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
            ctx.input_mut(|input| input.events.retain(|event| !keyboard_event(event)));
        }
    }
}
fn clear_selection(ctx: &egui::Context) {
    context_capture::clear_source(ctx);
    let mut selection = egui::text_selection::LabelSelectionState::load(ctx);
    selection.clear_selection();
    selection.store(ctx);
}
fn guard_activation(ctx: &egui::Context) {
    ctx.input_mut(|input| {
        input.events.retain(|event| {
            !matches!(event,
                egui::Event::Key { key: egui::Key::Enter, modifiers, .. }
                    if *modifiers != egui::Modifiers::NONE
            ) && !matches!(event,
                egui::Event::Key { key: egui::Key::Backspace, modifiers, .. }
                    if modifiers.command || modifiers.ctrl
            )
        })
    });
}
fn inline_source<'a>(
    outputs: &'a [RecordedFile],
    diffs: &'a BTreeMap<String, Diff>,
    key: &Key,
) -> Option<&'a str> {
    if key.0 {
        diffs
            .get(&key.1)?
            .content
            .as_ref()?
            .as_ref()
            .ok()
            .map(String::as_str)
    } else {
        outputs
            .iter()
            .find(|file| file.path == key.1)
            .map(|file| file.diff.as_str())
    }
}

#[cfg(test)]
mod input_regressions;
#[cfg(test)]
mod selection_tests;
#[cfg(test)]
mod tests;
#[cfg(test)]
mod wrap_preference_tests;
