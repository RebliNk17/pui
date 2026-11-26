//! Application inactivity is separate from per-agent process retirement.
use super::super::*;
use std::time::Instant;

const TIMEOUT: Duration = Duration::from_secs(10 * 60);

pub(in crate::app) struct Idle {
    last: Instant,
}
impl Default for Idle {
    fn default() -> Self {
        Self {
            last: Instant::now(),
        }
    }
}
impl Idle {
    pub(in crate::app) fn touch(&mut self) {
        self.last = Instant::now();
    }
    pub(in crate::app) fn observe(&mut self, ctx: &egui::Context) {
        let input = ctx.input(|input| {
            input
                .events
                .iter()
                .any(|event| !matches!(event, egui::Event::Screenshot { .. }))
                || !input.raw.dropped_files.is_empty()
                || !input.raw.hovered_files.is_empty()
                || input.pointer.any_down()
        });
        if input {
            self.touch();
        }
    }
    fn remaining(&self, now: Instant) -> Duration {
        TIMEOUT.saturating_sub(now.saturating_duration_since(self.last))
    }
}

impl Pui {
    fn update_restart_blocked(&self, automatic: bool) -> bool {
        self.demo
            || self.screenshot.is_some()
            || self.agents.exiting
            || !self.sync.enabled()
            || !self.sync.dock_single_window()
            || self.sync.has_unsent()
            || !self.composer_recovery_ready()
            || (automatic && !self.agents_sleeping_for_update())
            || (!automatic && !self.agents_quiet_for_update())
            || rpc::retiring()
            || self.tasks.update_restart_blocked()
            || self.terminals.running()
            || self.background_tasks.update_restart_blocked()
            || self.automation.update_restart_blocked()
            || self.remote.is_running()
            || self.deletion.in_progress()
            || self.deletion.confirming()
            || self.restoring_code()
            || self.history_pending.is_some()
            || self.history_view.is_open()
            || self.resource_reload.pending()
            || self.resource_reload.connector_pending()
            || self.resource_reload.confirming()
            || self.pending_dialog.is_some()
            || self.loading_chat
            || self.pending_session.is_some()
            || self.interrupt.is_some()
            || !self.deliveries.is_empty()
            || self.waiting.values().any(|queue| !queue.is_empty())
            || !self.state.ready()
            || !self.state.dialogs.is_empty()
            || self.state.stream.is_some()
            || self.state.retrying
            || self.state.pending_messages.iter().any(|m| {
                !matches!(
                    m.status,
                    protocol::DeliveryStatus::Handled | protocol::DeliveryStatus::Cancelled
                )
            })
            || self.dictation.active()
            || self.image_picker_rx.is_some()
            || self.chat_folder_rx.is_some()
            || self.bypass_pending.is_some()
            || self.worker_upgrade_pending.is_some()
            || self.settings_view.providers.any_modal_open()
            || self.changes.update_restart_blocked(&self.ctx)
    }

    pub(in crate::app) fn maybe_install_update(
        &mut self,
        ctx: &egui::Context,
        frame: &mut eframe::Frame,
    ) {
        let manual = std::mem::take(&mut self.update_install_requested);
        self.prefs.settings.normalize_update_preferences();
        let enabled =
            self.prefs.settings.auto_download_updates && self.prefs.settings.auto_install_updates;
        if !manual && (!enabled || !self.updates.automatic_install_ready()) {
            return;
        }
        if manual && !self.updates.ready_to_install() {
            return;
        }
        if manual && (!self.updates.open || egui::Popup::is_any_open(ctx)) {
            return;
        }
        if !manual {
            let remaining = self.update_idle.remaining(Instant::now());
            if !remaining.is_zero() {
                ctx.request_repaint_after(remaining);
                return;
            }
            // Any open modal/form or utility/browser pane may own unsaved input.
            if self.updates.open
                || self.settings_open
                || self.search_open
                || self.menu.about
                || self.changes.review.is_some()
                || self.files.is_open()
                || self.tasks.open
                || self.automation.open
                || self.kanban.open
                || self.background_tasks.open
                || self.pr_pane.open
                || egui::Popup::is_any_open(ctx)
                || ctx.memory(|m| m.top_modal_layer().is_some())
            {
                return;
            }
        }
        if self.update_restart_blocked(!manual) {
            if manual {
                self.updates.fail_install("Restart postponed. Wait for agents to finish, close other Pui windows, and finish active operations, terminals and attachment imports. Drafts are kept.".into());
            }
            return;
        }
        let Some(path) = restart::Recovery::location() else {
            self.updates
                .fail_install("No recovery directory; Pui was kept open.".into());
            return;
        };
        let Some(storage) = frame.storage_mut() else {
            self.updates
                .fail_install("Persistent storage is unavailable; Pui was kept open.".into());
            return;
        };
        if let Err(error) = self.save_before_update(storage, &path) {
            self.updates.fail_install(error);
            return;
        }
        // The automatic path has no clients. Explicit restarts also drain late
        // transport activity; never discard a question or an editor update that
        // arrived while saving. Do not dispatch follow-up work at this boundary.
        if manual && self.drain_update_rpc() {
            self.updates.fail_install("New chat activity arrived while saving. Pui stayed open; review it before retrying installation.".into());
            return;
        }
        if self.update_restart_blocked(!manual) {
            return;
        }
        if !cfg!(test) && self.updates.attempt_install(ctx) {
            self.agents.exiting = true;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn inactivity_is_ten_minutes_monotonic_and_foreground_does_not_block() {
        let now = Instant::now();
        let idle = Idle { last: now };
        assert_eq!(
            idle.remaining(now + Duration::from_secs(599)),
            Duration::from_secs(1)
        );
        assert!(idle.remaining(now + TIMEOUT).is_zero());
        assert_eq!(idle.remaining(now - Duration::from_secs(1)), TIMEOUT);
    }
    #[test]
    fn automatic_restart_is_fail_closed_for_unknown_window_presence_and_pending_drafts() {
        let ctx = egui::Context::default();
        let mut app = Pui::new(
            &eframe::CreationContext::_new_kittest(ctx),
            Options {
                demo: true,
                ..Default::default()
            },
        );
        app.demo = false;
        app.state = State::default();
        app.draft = "Ready draft can be saved".into();
        assert!(
            app.update_restart_blocked(true),
            "disabled coordination is not proof of being alone"
        );
        assert!(app.update_restart_blocked(false));
        app.prefs.settings.auto_download_updates = false;
        app.prefs.settings.auto_install_updates = true;
        let mut frame = eframe::Frame::_new_kittest();
        app.maybe_install_update(&app.ctx.clone(), &mut frame);
        assert!(!app.prefs.settings.auto_install_updates);
        assert!(!app.agents.exiting);
        assert_eq!(app.draft, "Ready draft can be saved");
    }

    #[test]
    fn screenshot_events_dont_fake_use_but_real_input_resets_idle() {
        let ctx = egui::Context::default();
        let mut idle = Idle {
            last: Instant::now() - TIMEOUT,
        };
        let _ = ctx.run(
            egui::RawInput {
                events: vec![egui::Event::Text("typed".into())],
                ..Default::default()
            },
            |ctx| idle.observe(ctx),
        );
        assert!(idle.remaining(Instant::now()) > Duration::from_secs(590));
    }
}
