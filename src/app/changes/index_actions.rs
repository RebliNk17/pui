//! Explicit inspected and confirmed single-file Git index actions.
use super::*;
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};
use workspace::index_action::{Action, Snapshot};
#[cfg(test)]
mod tests;
#[derive(Default)]
pub(super) struct IndexActions {
    pub(super) controls: Vec<egui::Id>,
    pending: Option<workspace::read_only::Request<Result<Snapshot, String>>>,
    inspected: Option<Snapshot>,
    applying: Option<workspace::read_only::Request<Result<(), String>>>,
    owner: Option<(PathBuf, String)>,
    pub(super) notice: Option<String>,
    success_notice: Option<String>,
}
impl Changes {
    pub(super) fn index_row(
        &mut self,
        ui: &mut egui::Ui,
        workspace: &Workspace,
        file: &workspace::FileChange,
        loading: bool,
    ) {
        ui.horizontal_wrapped(|ui| {
            ui.label(theme::small(ui, format!("Index/worktree: {}", file.status)));
            for action in [Action::Stage, Action::Unstage, Action::Discard] {
                if !workspace::index_action::available(&file.status, action) {
                    continue;
                }
                let enabled = self.index_writable
                    && !loading
                    && workspace.error.is_none()
                    && workspace.is_git
                    && self.index_actions.pending.is_none()
                    && self.index_actions.applying.is_none()
                    && self.index_actions.inspected.is_none()
                    && !egui::Popup::is_any_open(ui.ctx())
                    && ui.memory(|m| m.top_modal_layer().is_none());
                let response = ui
                    .push_id(
                        (
                            "git-index-action",
                            &self.identity,
                            &workspace.root,
                            &file.path,
                            &file.status,
                            action.label(),
                        ),
                        |ui| {
                            ui.add_enabled(
                                enabled,
                                egui::Button::new(theme::small(ui, action.label())).frame(false),
                            )
                        },
                    )
                    .inner;
                self.index_actions.controls.push(response.id);
                #[cfg(test)]
                ui.ctx().data_mut(|d| {
                    d.insert_temp(
                        egui::Id::new(("index-action", action == Action::Stage, &file.path)),
                        (response.id, response.rect, response.enabled()),
                    )
                });
                if response.clicked() {
                    self.prepare_index(ui.ctx(), workspace, file, action);
                }
            }
        });
    }
    fn prepare_index(
        &mut self,
        ctx: &egui::Context,
        workspace: &Workspace,
        file: &workspace::FileChange,
        action: Action,
    ) {
        let Some(permit) = branches::permit(ctx) else {
            self.index_actions.notice =
                Some("Another Git inspection is finishing. Retry explicitly.".into());
            return;
        };
        self.index_actions.owner = Some(self.identity.clone());
        self.index_actions.notice =
            Some("Inspecting exact file and index before confirmation…".into());
        let root = workspace.root.clone();
        let path = file.path.clone();
        let status = file.status.clone();
        let (tx, rx) = mpsc::channel();
        let cancel = Arc::new(AtomicBool::new(false));
        self.index_actions.pending = Some(workspace::read_only::Request::new(rx, cancel.clone()));
        let ctx = ctx.clone();
        if std::thread::Builder::new()
            .name("pui-index-inspect".into())
            .spawn(move || {
                let result =
                    workspace::index_action::prepare(&root, &path, &status, action, &cancel);
                drop(permit);
                if !cancel.load(Ordering::Acquire) {
                    let _ = tx.send(result);
                    ctx.request_repaint();
                }
            })
            .is_err()
        {
            self.index_actions.pending = None;
            self.index_actions.notice = Some("Cannot start file inspection.".into());
        }
    }
    fn apply_index(&mut self, ctx: &egui::Context, snapshot: Snapshot) {
        let Some(permit) = branches::permit(ctx) else {
            self.index_actions.notice =
                Some("Git inspection busy. Inspect again explicitly.".into());
            return;
        };
        let (tx, rx) = mpsc::channel();
        let cancel = Arc::new(AtomicBool::new(false));
        self.index_actions.success_notice = Some(if snapshot.action == Action::Discard {
            format!(
                "Working file restored from index. Staged changes and HEAD unchanged. Recovery copy: {}",
                snapshot.recovery_path().display()
            )
        } else {
            "Index updated. Working files and HEAD unchanged.".into()
        });
        self.index_actions.applying = Some(workspace::read_only::Request::new(rx, cancel.clone()));
        let ctx = ctx.clone();
        if std::thread::Builder::new()
            .name("pui-index-apply".into())
            .spawn(move || {
                let result = workspace::index_action::apply(snapshot, &cancel);
                drop(permit);
                let _ = tx.send(result);
                ctx.request_repaint();
            })
            .is_err()
        {
            self.index_actions.applying = None;
            self.index_actions.notice = Some("Cannot start index action.".into());
        }
    }
    pub(in crate::app) fn cancel_index_intent(&mut self) {
        self.commit = Default::default();
        self.push = Default::default();
        self.index_actions.pending = None;
        self.index_actions.inspected = None;
        self.index_actions.applying = None;
        self.index_actions.controls.clear();
    }
    pub(super) fn index_inputs(&mut self, ctx: &egui::Context, visible: bool) {
        let state = &mut self.index_actions;
        if state
            .owner
            .as_ref()
            .is_some_and(|owner| owner != &self.identity)
            || !visible
            || !self.index_writable
        {
            state.pending = None;
            state.inspected = None;
            if state.owner.as_ref() != Some(&self.identity) || !self.index_writable {
                state.applying = None;
            }
            state.controls.clear();
        }
        if let Some(result) = state.pending.as_ref().and_then(|r| match r.try_recv() {
            Ok(r) => Some(r),
            Err(mpsc::TryRecvError::Empty) => None,
            Err(_) => Some(Err("File inspection ended.".into())),
        }) {
            state.pending = None;
            match result {
                Ok(snapshot) => {
                    state.inspected = Some(snapshot);
                    state.notice = None;
                }
                Err(e) => state.notice = Some(e),
            }
        }
        if let Some(result) = state.applying.as_ref().and_then(|r| match r.try_recv() {
            Ok(r) => Some(r),
            Err(mpsc::TryRecvError::Empty) => None,
            Err(_) => Some(Err(
                "Index action ended; inspect status before retrying.".into()
            )),
        }) {
            state.applying = None;
            state.notice = Some(match result {
                Ok(()) => state
                    .success_notice
                    .take()
                    .unwrap_or_else(|| "Git action finished. Inspect refreshed status.".into()),
                Err(e) => e,
            });
            self.refresh = true;
        }
        let own = egui::LayerId::new(egui::Order::Foreground, egui::Id::new("git-index-confirm"));
        if ctx.memory(|m| m.top_modal_layer().is_some_and(|l| l != own))
            || egui::Popup::is_any_open(ctx)
        {
            state.inspected = None;
            return;
        }
        let focused = ctx.memory(|m| m.focused());
        let pointer = ctx.input(|i| {
            (i.pointer.primary_pressed() || i.pointer.primary_released())
                .then(|| i.pointer.interact_pos())
                .flatten()
        });
        let owns_pointer = state.controls.iter().any(|id| {
            ctx.read_response(*id).is_some_and(|r| {
                r.enabled()
                    && pointer.is_some_and(|p| {
                        r.interact_rect.contains(p) && ctx.layer_id_at(p) == Some(r.layer_id)
                    })
            })
        });
        if state.controls.iter().any(|id| {
            focused == Some(*id)
                || focused.is_none()
                    && ctx.input(|i| i.key_pressed(egui::Key::Escape))
                    && ctx.memory(|m| m.focus_lost_by_escape(*id))
                || ctx.read_response(*id).is_some_and(|r| {
                    r.enabled()
                        && pointer.is_some_and(|p| {
                            r.interact_rect.contains(p) && ctx.layer_id_at(p) == Some(r.layer_id)
                        })
                })
        }) || state.inspected.is_some()
        {
            if ctx.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::Escape)) {
                state.inspected = None;
                if let Some(id) = focused {
                    ctx.memory_mut(|m| m.surrender_focus(id));
                }
            }
            let confirming = state.inspected.is_some();
            ctx.input_mut(|i| { i.events.retain(|e|{
                let text=matches!(e,egui::Event::Text(_)|egui::Event::Paste(_));
                let chord=matches!(e,egui::Event::Key{key:egui::Key::Enter|egui::Key::Backspace,modifiers,..}if modifiers.command||modifiers.ctrl);
                let activation=owns_pointer&&!confirming&&matches!(e,egui::Event::Key{key:egui::Key::Enter|egui::Key::Space,..});
                !(text||chord||activation)
            }); });
        }
    }
    pub(super) fn index_dialog(&mut self, ctx: &egui::Context) {
        let Some(snapshot) = &mut self.index_actions.inspected else {
            return;
        };
        if egui::Popup::is_any_open(ctx) || prompt_ui::modal_owns_current_pass(ctx) {
            self.index_actions.inspected = None;
            return;
        }
        let own = egui::LayerId::new(egui::Order::Foreground, egui::Id::new("git-index-confirm"));
        if ctx.memory(|m| m.top_modal_layer().is_some_and(|layer| layer != own)) {
            self.index_actions.inspected = None;
            return;
        }
        let title = if snapshot.hunk.0.is_some() {
            if snapshot.action == Action::Stage {
                "Stage selected hunks"
            } else {
                "Unstage selected hunks"
            }
        } else {
            snapshot.action.label()
        };
        let destructive = snapshot.action == Action::Discard;
        let description = if destructive {
            "Replaces this working file with its index version, discarding only unstaged content. Staged changes and HEAD stay unchanged. A recovery copy is retained at the path below. No commit or push."
        } else {
            "Updates only this file’s Git index entry. Working files and HEAD are unchanged. The index, HEAD, content and frozen preview are rechecked before publication. No commit or push."
        };
        let result =
            confirmations::Dialog::new("git-index-confirm", title, description, "Cancel", title)
                .destructive(destructive)
                .enabled(
                    snapshot
                        .hunk
                        .valid(workspace::index_action::hunks::count(snapshot)),
                )
                .show(ctx, |ui| {
                    ui.add(
                        egui::Label::new(theme::small(
                            ui,
                            format!(
                                "{}\n{} · {}",
                                snapshot.root.display(),
                                snapshot.path,
                                snapshot.status
                            ),
                        ))
                        .wrap(),
                    );
                    if destructive {
                        ui.add(
                            egui::Label::new(theme::small(
                                ui,
                                format!("Recovery copy: {}", snapshot.recovery_path().display()),
                            ))
                            .wrap(),
                        );
                    }
                    ui.label(theme::small(
                        ui,
                        if destructive {
                            "Unstaged changes to discard · index → working tree"
                        } else if snapshot.status == "??" {
                            "Entire new file to stage · literal untrusted content"
                        } else if snapshot.action == Action::Stage {
                            "Changes to stage · index → working tree"
                        } else {
                            "Staged changes to remove from index · HEAD → index"
                        },
                    ));
                    let count = workspace::index_action::hunks::count(snapshot);
                    if count > 0 {
                        ui.horizontal_wrapped(|ui| {
                            if ui
                                .selectable_label(snapshot.hunk.0.is_none(), "Whole file")
                                .clicked()
                            {
                                snapshot.hunk = Default::default();
                            }
                            for n in 0..count {
                                let response = ui.selectable_label(
                                    snapshot.hunk.contains(n),
                                    format!("Hunk {}", n + 1),
                                );
                                if response.clicked() {
                                    snapshot.hunk.toggle(n);
                                }
                                #[cfg(test)]
                                ui.ctx().data_mut(|d| {
                                    d.insert_temp(
                                        egui::Id::new(("stage-hunk", n)),
                                        (response.id, response.rect),
                                    )
                                });
                                let _ = response;
                            }
                        });
                    }
                    let selected = if snapshot.hunk.0.is_some() {
                        workspace::index_action::hunks::extract_selection(snapshot)
                    } else {
                        None
                    };
                    let preview = if snapshot.hunk.0 == Some(0) {
                        "(select at least one hunk or Whole file)"
                    } else {
                        selected.as_deref().unwrap_or(&snapshot.preview)
                    };
                    if preview.is_empty() {
                        ui.label(theme::small(
                            ui,
                            "(empty content change · file mode or empty-file change)",
                        ));
                    } else {
                        egui::ScrollArea::both()
                            .id_salt("index-frozen-preview")
                            .max_height(240.0)
                            .show_rows(ui, 16.0, preview.lines().count(), |ui, range| {
                                for line in preview.lines().skip(range.start).take(range.len()) {
                                    ui.add(
                                        egui::Label::new(
                                            RichText::new(line).monospace().size(10.5),
                                        )
                                        .extend()
                                        .selectable(false),
                                    );
                                }
                            });
                    }
                    #[cfg(test)]
                    ui.ctx().data_mut(|d| {
                        d.insert_temp(egui::Id::new("index-preview-shown"), snapshot.preview.len())
                    });
                });
        if let Some(choice) = result {
            let snapshot = self.index_actions.inspected.take().unwrap();
            if choice == confirmations::Choice::Confirm {
                self.apply_index(ctx, snapshot);
            }
        }
        ctx.input_mut(|i| {
            i.events.retain(|e| {
                !matches!(
                    e,
                    egui::Event::Key { .. } | egui::Event::Text(_) | egui::Event::Paste(_)
                )
            })
        });
    }
}
