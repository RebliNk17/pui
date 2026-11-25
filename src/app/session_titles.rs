//! Assign display titles on explicit metadata/history events, never idle frames.
use super::*;
use sha2::{Digest, Sha256};
use std::{collections::HashMap, path::Path};

pub(super) fn title_event(record: &Value) -> bool {
    match protocol::string(record, "type") {
        "session_info_changed" => true,
        "extension_ui_request" => record["statusKey"] == "pui-initial-title",
        "message_start" | "message_end" => record["message"]["role"] == "user",
        "response" => {
            record["success"] != false
                && matches!(
                    protocol::string(record, "command"),
                    "get_state" | "get_entries" | "get_messages"
                )
        }
        _ => false,
    }
}

fn reconcile(state: &mut State, session: Option<&mut Session>, saved: &mut sidebar::ChatPrefs) {
    #[cfg(test)]
    RECONCILIATIONS.with(|count| count.set(count.get() + 1));
    // Preferences pin observed legacy names conservatively, not an invitation to
    // regenerate unknown sessions from whatever survives the context projection.
    if let Some(title) = &saved.initial_title {
        if state.assigned_title() != Some(title.as_str()) {
            state.title = state
                .session_file
                .clone()
                .map(|session_file| protocol::ChatTitle {
                    session_file,
                    title: title.clone(),
                    initial_request: None,
                });
        }
    } else {
        if state.assigned_title().is_none() && state.title_request.is_none() {
            let title = (!protocol::title_placeholder(&state.session_name))
                .then(|| state.session_name.clone())
                .or_else(|| {
                    session
                        .as_ref()
                        .map(|s| s.title.clone())
                        .filter(|t| !protocol::title_placeholder(t))
                });
            if let Some(title) = title {
                state.preserve_title(&title);
            }
        }
        saved.initial_title = state.assigned_title().map(str::to_owned);
    }
    if let Some(session) = session
        && let Some(title) = state.assigned_title()
    {
        session.title = title.into();
    }
}

impl Pui {
    /// Called only by the direct interactive composer submit path, not by queue
    /// replay, automation, child messages or model/tool follow-ups. The command
    /// prepares inert metadata; the extension verifies the exact next raw input.
    pub(super) fn prepare_initial_title(&mut self, message: &run_status::WaitingMessage) {
        if !self.new_chat
            || !self.ultracode_ready
            || self.is_mirror()
            || self.readonly_worker()
            || self.state.assigned_title().is_some()
            || !self.state.messages.is_empty()
            || !self.state.pending_messages.is_empty()
            || !self.deliveries.is_empty()
            || !protocol::title_placeholder(&self.state.session_name)
        {
            return;
        }
        let (Some(path), Some(session_id)) = (
            self.state.session_file.clone(),
            self.state.session_id.clone(),
        ) else {
            return;
        };
        if self
            .prefs
            .chats
            .get(Path::new(&path))
            .is_some_and(|meta| meta.title.is_some() || meta.initial_title.is_some())
        {
            return;
        }
        let Some(title) = sessions::initial_title(&message.text)
            .or_else(|| (!message.images.is_empty()).then(|| "Image attachment".into()))
        else {
            return;
        };
        let record = message.rpc_record("prompt");
        let images = record["images"].as_array().map_or(0, Vec::len);
        let mut digest = Sha256::new();
        digest.update(protocol::string(&record, "message").as_bytes());
        digest.update(format!("\0{images}").as_bytes());
        let request = crate::hex::lower(digest.finalize());
        self.state.title_request = Some(request.clone());
        let data = json!({"version":1,"sessionFile":path,"sessionId":session_id,"initialRequest":request,"title":title});
        self.send(json!({"type":"prompt","message":format!("/pui-initial-title {data}")}));
    }

    pub(super) fn update_session_title(&mut self) {
        let Some(path) = self.state.session_file.clone() else {
            return;
        };
        let session = self
            .sessions
            .iter_mut()
            .find(|s| s.path == Path::new(&path));
        let path = PathBuf::from(path);
        let mut unsaved = sidebar::ChatPrefs::default();
        let prefs = self.prefs.chats.get_mut(&path).unwrap_or(&mut unsaved);
        let before = self.state.title.clone();
        reconcile(&mut self.state, session, prefs);
        let changed = before != self.state.title;
        if unsaved.initial_title.is_some() {
            self.prefs.chats.insert(path.clone(), unsaved);
        }
        if changed {
            self.invalidate_title_publication(&path);
        }
    }

    pub(super) fn refresh_session_titles(&mut self) {
        let mut indices = HashMap::with_capacity(self.sessions.len());
        for (index, session) in self.sessions.iter_mut().enumerate() {
            indices.entry(session.path.clone()).or_insert(index);
            if let Some(title) = self
                .prefs
                .chats
                .get(&session.path)
                .and_then(|p| p.initial_title.as_ref())
            {
                session.title.clone_from(title);
            }
        }
        let mut changed_paths = Vec::new();
        let mut unconfirmed = std::collections::HashSet::new();
        for state in std::iter::once(&mut self.state)
            .chain(self.agents.parked.values_mut().map(|chat| &mut chat.state))
        {
            let Some(path) = state.session_file.as_deref() else {
                continue;
            };
            let session = indices
                .get(Path::new(path))
                .map(|index| &mut self.sessions[*index]);
            let path = PathBuf::from(path);
            if state.title_request.is_some() && state.assigned_title().is_none() {
                unconfirmed.insert(path.clone());
            }
            let mut unsaved = sidebar::ChatPrefs::default();
            let prefs = self.prefs.chats.get_mut(&path).unwrap_or(&mut unsaved);
            let before = state.title.clone();
            reconcile(state, session, prefs);
            if before != state.title {
                changed_paths.push(path.clone());
            }
            if unsaved.initial_title.is_some() {
                self.prefs.chats.insert(path, unsaved);
            }
        }
        for path in changed_paths {
            self.invalidate_title_publication(&path);
        }
        for session in &mut self.sessions {
            if protocol::title_placeholder(&session.title) || unconfirmed.contains(&session.path) {
                continue;
            }
            let prefs = self.prefs.chats.entry(session.path.clone()).or_default();
            if let Some(title) = &prefs.initial_title {
                session.title.clone_from(title);
            } else if !protocol::title_placeholder(&session.title) {
                prefs.initial_title = Some(session.title.clone());
            }
        }
    }
}

#[cfg(test)]
thread_local! {
    static RECONCILIATIONS: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

#[cfg(test)]
mod lifecycle_tests;
#[cfg(test)]
mod tests;
