//! Full transcript actions keep their originating conversation while embedded.
use super::*;

impl Pui {
    pub(in crate::app) fn side_chat_visible(&self) -> bool {
        !self.settings_open
            && !self.tasks.open
            && !self.kanban.open
            && !self.automation.open
            && !self.terminals.is_open()
            && !self.inspector_panel
    }
    pub(in crate::app) fn side_chat_gallery_viewer(&mut self, ctx: &egui::Context) -> bool {
        let Some(target) = self.docked_side_chat_target() else {
            return false;
        };
        self.agents
            .parked
            .get_mut(&target)
            .is_some_and(|chat| chat.show_galleries(ctx))
    }

    pub(in crate::app) fn side_composer_formatting_available(&self, ctx: &egui::Context) -> bool {
        !self.settings_open
            && !self.search_open
            && !self.kanban.open
            && !self.automation.open
            && !self.deletion.confirming()
            && !self.agents.close_prompt
            && !self.history_view.is_open()
            && self.docked_side_chat_target().is_some_and(|target| {
                ctx.memory(|memory| {
                    memory.has_focus(egui::Id::new(("btw-editor", target.as_str())))
                }) && self
                    .agents
                    .parked
                    .get(&target)
                    .is_some_and(|chat| chat.state.dialogs.is_empty())
            })
    }
    pub(in crate::app) fn format_side_composer(
        &mut self,
        ctx: &egui::Context,
        format: composer_text::Format,
    ) -> bool {
        let Some(target) = self
            .docked_side_chat_target()
            .filter(|_| self.side_composer_formatting_available(ctx))
        else {
            return false;
        };
        let Some(chat) = self.agents.parked.remove(&target) else {
            return false;
        };
        let main = self.take_chat();
        self.put_chat(chat);
        let owner = composer_text::set_owner(Some(target.clone()));
        self.context_chips.clear();
        composer_text::format(ctx, &mut self.draft, format);
        composer_text::set_owner(owner);
        let chat = self.take_chat();
        self.agents.parked.insert(target, chat);
        self.put_chat(main);
        true
    }

    pub(super) fn side_chat_owns_editor(&self, ctx: &egui::Context, target: &str) -> bool {
        let id = egui::Id::new(("btw-editor", target));
        ctx.memory(|memory| memory.has_focus(id) || memory.had_focus_last_frame(id))
    }

    pub(super) fn side_transcript_action(&mut self, ctx: &egui::Context) {
        let Some(action) = self.transcript.action.take() else {
            return;
        };
        match action {
            crate::transcript::TranscriptAction::Continue(_) => {}
            crate::transcript::TranscriptAction::Open(target) => {
                if let Err(error) = self.files.open_target(ctx, &self.prefs.cwd, &target) {
                    self.state.notice = Some(error);
                } else {
                    self.inspector_panel = false;
                }
            }
            crate::transcript::TranscriptAction::ViewSource(target) => {
                let path = PathBuf::from(&target);
                let path = if path.is_absolute() {
                    path
                } else {
                    self.prefs.cwd.join(path)
                };
                if let Err(error) =
                    self.files
                        .open_workspace_source(ctx, &self.prefs.cwd, &path, None)
                {
                    self.state.notice = Some(error);
                } else {
                    self.inspector_panel = false;
                }
            }
            crate::transcript::TranscriptAction::OpenVisual { id, title, html } => {
                self.files
                    .open_visual_html(ctx, &self.prefs.cwd, &id, &title, &html);
                self.inspector_panel = false;
                self.background_tasks.open = false;
            }
            crate::transcript::TranscriptAction::OpenWorkflow(id) => {
                if self.background_tasks.workflows.iter().any(|w| w.id == id)
                    || id.starts_with("wf-")
                {
                    self.open_background_tasks_panel(None, None);
                } else {
                    self.open_task_from_board(id, None);
                }
            }
        }
        ctx.request_repaint();
    }

    /// Global attachment routing runs before either composer is rendered.
    pub(in crate::app) fn side_chat_image_inputs(&mut self, ctx: &egui::Context) -> bool {
        self.poll_image_inputs(ctx);
        let Some(target) = self.docked_side_chat_target() else {
            return false;
        };
        let side_rect =
            ctx.data(|data| data.get_temp::<egui::Rect>(egui::Id::new("side-chat-input-rect")));
        let dropping = ctx.input(|input| !input.raw.dropped_files.is_empty());
        let drop_in_side = ctx.input(|input| {
            input
                .pointer
                .latest_pos()
                .is_some_and(|pos| side_rect.is_some_and(|rect| rect.contains(pos)))
        });
        let owns = self.side_chat_visible()
            && if dropping {
                drop_in_side
            } else {
                self.side_chat_owns_keyboard(ctx)
            };
        let Some(chat) = self.agents.parked.remove(&target) else {
            return false;
        };
        let main = self.take_chat();
        let focus = self.focus_composer;
        self.focus_composer = false;
        self.put_chat(chat);
        let previous_owner = composer_text::set_owner(Some(target.clone()));
        if owns {
            self.image_inputs(ctx);
        } else {
            self.poll_image_inputs(ctx);
        }
        composer_text::set_owner(previous_owner);
        self.side_chat.focus |= self.focus_composer;
        let chat = self.take_chat();
        self.agents.parked.insert(target, chat);
        self.put_chat(main);
        self.focus_composer = focus;
        owns
    }
}
