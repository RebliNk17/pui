//! Reconnect one existing MCP connection through Pi's supported extension command.
//! Never restart the agent, submit a model prompt, or replay an uncertain request.
use super::*;

impl Pui {
    pub(in crate::app) fn connector_reconnect_blocked(&self) -> Option<&'static str> {
        if self.demo || self.is_mirror() || self.agents.background {
            return Some("Open this chat in its owning Pui window to reconnect a connector.");
        }
        if !self.connected || self.client.is_none() || self.loading_chat {
            return Some("Connect to this chat before reconnecting a connector.");
        }
        if self.resource_reload.pending() || self.resource_reload.confirming {
            return Some("Finish reloading chat resources first.");
        }
        if self.resource_reload.connector_pending() {
            return Some("A connector reconnect is already in progress for this chat.");
        }
        if self.history_pending.is_some() || self.restoring_code() || self.agents.exiting {
            return Some("Finish the current chat operation first.");
        }
        if !self.state.commands.iter().any(|c| c.name == "mcp") {
            return Some(
                "This runtime does not provide /mcp. Reload chat resources when idle to load MCP support.",
            );
        }
        if self
            .state
            .session_file
            .as_ref()
            .is_none_or(|path| self.sync.enabled() && !self.sync.owns(std::path::Path::new(path)))
        {
            return Some("Reconnect from the window that owns this saved chat.");
        }
        None
    }

    pub(in crate::app) fn request_connector_reconnect(&mut self, name: &str) {
        if self.connector_reconnect_blocked().is_some() || !valid_name(name) {
            return;
        }
        self.ensure_agent();
        // Pi closes the selected transport, rejecting any in-flight calls on it.
        // Busy is not a prohibition, but the user must acknowledge that risk.
        if !self.state.ready() {
            self.resource_reload.connector_confirm =
                Some((name.into(), self.state.session_file.clone().unwrap()));
        } else {
            self.begin_connector_reconnect(name);
        }
    }

    pub(super) fn connector_reconnect_dialog(&mut self, ctx: &egui::Context) {
        let Some((name, session)) = self.resource_reload.connector_confirm.clone() else {
            return;
        };
        if self.state.session_file.as_deref() != Some(session.as_str()) {
            self.resource_reload.connector_confirm = None;
            return;
        }
        let blocked = self.connector_reconnect_blocked();
        let title = format!("Reconnect {name}?");
        let choice = confirmations::Dialog::new(
            "reconnect-mcp-connector",
            &title,
            "This chat is active. Reconnecting closes this connector's connection; any requests already using it can fail and are not automatically retried.",
            "Cancel",
            "Reconnect connector",
        )
        .destructive(false)
        .enabled(blocked.is_none())
        .width(440.0)
        .show(ctx, |ui| {
            confirmations::note(ui, "The chat keeps running. Other connectors, history, drafts and temporary approvals are unchanged. This does not reload configuration or extensions.");
            if let Some(reason) = blocked {
                confirmations::note(ui, reason);
            }
        });
        match choice {
            Some(confirmations::Choice::Cancel) => self.resource_reload.connector_confirm = None,
            Some(confirmations::Choice::Confirm) => {
                self.resource_reload.connector_confirm = None;
                self.begin_connector_reconnect(&name);
            }
            None => {}
        }
    }

    fn begin_connector_reconnect(&mut self, name: &str) {
        if self.connector_reconnect_blocked().is_some() || !valid_name(name) {
            return;
        }
        // Verify command presence above: an unsupported slash command must never
        // fall through to a real model prompt. Do not touch delivery/busy state.
        let command = json!({"type":"prompt", "message":format!("/mcp reconnect {name}")});
        self.clear_connector_error();
        match self.client.as_mut().unwrap().enqueue(command) {
            Ok(id) => {
                self.resource_reload.reconnected.remove(name);
                self.resource_reload.connector_pending = Some(ConnectorRequest {
                    id,
                    name: name.into(),
                    started: std::time::Instant::now(),
                    confirmed: false,
                    timed_out: false,
                });
            }
            Err(error) => self.state.error = Some(error),
        }
    }

    /// Consume only this control command's receipt, never another prompt's.
    pub(in crate::app) fn connector_reconnect_record(&mut self, record: &Value) -> bool {
        if record["type"] == "extension_ui_request"
            && record["method"] == "notify"
            && matches!(protocol::string(record, "notifyType"), "error" | "warning")
        {
            let message = protocol::string(record, "message").to_ascii_lowercase();
            self.resource_reload
                .reconnected
                .retain(|name| !message.contains(&name.to_ascii_lowercase()));
        }
        let Some(request) = self.resource_reload.connector_pending.as_mut() else {
            return false;
        };
        if record["type"] == "extension_ui_request" && record["method"] == "notify" {
            // Pi's command acceptance does NOT mean the connection succeeded.
            let prefix = format!("Reconnected to MCP server \"{}\" (", request.name);
            if record["notifyType"] == "info"
                && protocol::string(record, "message").starts_with(&prefix)
            {
                request.confirmed = true;
                return true;
            }
        }
        if record["type"] != "response"
            || record["command"] != "prompt"
            || record["id"] != request.id
        {
            return false;
        }
        let request = self.resource_reload.connector_pending.take().unwrap();
        if record["success"] == true
            && record["data"]["disposition"] == "handled"
            && request.confirmed
        {
            self.clear_connector_error();
            self.resource_reload
                .reconnected
                .insert(request.name.clone());
            self.state.notice = Some(format!(
                "Reconnected {}. The chat was not restarted.",
                request.name
            ));
        } else {
            self.set_connector_error(format!(
                "Reconnect for {} was not confirmed. Check Diagnostics for connection or sign-in errors. The chat was not restarted; nothing is retried automatically.",
                request.name,
            ));
        }
        true
    }

    fn clear_connector_error(&mut self) {
        if let Some(error) = self.resource_reload.connector_error.take() {
            if self.state.error.as_ref() == Some(&error) {
                self.state.error = None;
            }
        }
    }

    fn set_connector_error(&mut self, error: String) {
        self.clear_connector_error();
        if self.state.error.is_none() {
            self.state.error = Some(error.clone());
            self.resource_reload.connector_error = Some(error);
        }
    }

    pub(super) fn poll_connector_reconnect(&mut self) {
        let Some(request) = self.resource_reload.connector_pending.as_mut() else {
            return;
        };
        if !self.connected || self.client.is_none() {
            let request = self.resource_reload.connector_pending.take().unwrap();
            self.set_connector_error(format!(
                "Pi disconnected while reconnecting {}. Outcome unknown; nothing was retried.",
                request.name
            ));
            return;
        }
        let remaining = Duration::from_secs(120).saturating_sub(request.started.elapsed());
        if remaining.is_zero() {
            if !request.timed_out {
                request.timed_out = true;
                let name = request.name.clone();
                self.set_connector_error(format!(
                    "Reconnect for {name} has not completed. The chat can continue. No automatic retry; wait for Pi's result or reopen the chat."
                ));
            }
        } else {
            self.ctx.request_repaint_after(remaining);
        }
    }
}

fn valid_name(name: &str) -> bool {
    // /mcp splits arguments on whitespace; never allow another argument/command.
    !name.is_empty()
        && name.len() <= 256
        && name
            .bytes()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, b'_' | b'-'))
}

#[cfg(test)]
mod tests;
