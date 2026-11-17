//! Reconcile titles on metadata/history events, never on animation or idle frames.
use super::*;
use std::{collections::HashMap, path::Path};

pub(super) fn title_event(record: &Value) -> bool {
    match protocol::string(record, "type") {
        "session_info_changed" => true,
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

fn unnamed(title: &str) -> bool {
    title.trim().is_empty()
        || matches!(
            title,
            "Untitled session" | "New conversation" | "Opening chat…"
        )
}

fn reconcile(state: &mut State, session: Option<&mut Session>) {
    #[cfg(test)]
    RECONCILIATIONS.with(|count| count.set(count.get() + 1));
    if unnamed(&state.session_name) {
        state.session_name = session
            .as_ref()
            .filter(|s| !unnamed(&s.title))
            .map(|s| s.title.clone())
            .or_else(|| {
                state
                    .messages
                    .iter()
                    .filter(|m| m.role == "user")
                    .find_map(|m| {
                        sessions::prompt_title(&m.text)
                            .or_else(|| (!m.images.is_empty()).then(|| "Image attachment".into()))
                    })
            })
            .unwrap_or_default();
    }
    if let Some(session) = session
        && !state.session_name.is_empty()
    {
        session.title.clone_from(&state.session_name);
    }
}

impl Pui {
    pub(super) fn update_session_title(&mut self) {
        let Some(path) = self.state.session_file.as_deref() else {
            return;
        };
        let session = self.sessions.iter_mut().find(|s| s.path == Path::new(path));
        reconcile(&mut self.state, session);
    }

    /// A fresh directory scan can replace titles even when parked chats receive
    /// no RPC events. Index once, rather than scanning all sessions per chat.
    pub(super) fn refresh_session_titles(&mut self) {
        let mut indices = HashMap::with_capacity(self.sessions.len());
        for (index, session) in self.sessions.iter().enumerate() {
            indices.entry(session.path.clone()).or_insert(index);
        }
        for state in std::iter::once(&mut self.state)
            .chain(self.agents.parked.values_mut().map(|chat| &mut chat.state))
        {
            let Some(path) = state.session_file.as_deref() else {
                continue;
            };
            let session = indices
                .get(Path::new(path))
                .map(|index| &mut self.sessions[*index]);
            reconcile(state, session);
        }
    }
}

#[cfg(test)]
thread_local! {
    static RECONCILIATIONS: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

#[cfg(test)]
mod tests;
