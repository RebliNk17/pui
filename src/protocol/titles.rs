//! Display names are assigned once; Pi's mutable session metadata remains separate.
use super::*;

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq, Hash)]
pub struct ChatTitle {
    pub session_file: String,
    pub title: String,
    /// Absent for preserved legacy titles. Never infer eligibility from absence.
    pub initial_request: Option<String>,
}

pub fn placeholder(title: &str) -> bool {
    title.trim().is_empty()
        || matches!(
            title,
            "Untitled session" | "New conversation" | "Opening chat…"
        )
}

impl State {
    pub fn assigned_title(&self) -> Option<&str> {
        self.title
            .as_ref()
            .filter(|title| Some(title.session_file.as_str()) == self.session_file.as_deref())
            .map(|title| title.title.as_str())
    }

    pub fn preserve_title(&mut self, title: &str) {
        if self.assigned_title().is_none()
            && !placeholder(title)
            && let Some(session_file) = self.session_file.clone()
        {
            self.title = Some(ChatTitle {
                session_file,
                title: title.into(),
                initial_request: None,
            });
        }
    }

    pub(super) fn title_status(&mut self, value: &Value) {
        if value["version"] != 1
            || value["sessionFile"].as_str() != self.session_file.as_deref()
            || (self.session_id.is_some()
                && value["sessionId"].as_str() != self.session_id.as_deref())
            || self.assigned_title().is_some()
        {
            return;
        }
        let title = string(value, "title");
        let request = string(value, "initialRequest");
        if value["restored"] != true
            && (self.title_request.as_deref() != Some(request)
                || value["sessionId"].as_str() != self.session_id.as_deref())
        {
            return;
        }
        if !crate::sessions::valid_initial_title(title)
            || request.len() != 64
            || !request.bytes().all(|b| b.is_ascii_hexdigit())
        {
            return;
        }
        self.title = Some(ChatTitle {
            session_file: string(value, "sessionFile").into(),
            title: title.into(),
            initial_request: Some(request.into()),
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn metadata_and_compacted_history_cannot_change_assigned_display_title() {
        let mut state = State {
            session_file: Some("/first".into()),
            ..Default::default()
        };
        state.preserve_title("Fix Hebrew sidebar");
        state.apply(json!({"type":"session_info_changed","name":"Tool output title"}));
        state.apply(json!({"type":"response","command":"get_state","data":{"sessionFile":"/first","sessionName":"Compacted summary","sessionId":"a"}}));
        state.apply(json!({"type":"response","command":"get_messages","data":{"messages":[{"role":"compactionSummary","summary":"summary"},{"role":"user","content":"Different later request"}]}}));
        assert_eq!(
            state.session_name, "Compacted summary",
            "Keep raw protocol metadata"
        );
        assert_eq!(state.chat_title().as_deref(), Some("Fix Hebrew sidebar"));
        let resumed: State = serde_json::from_value(serde_json::to_value(&state).unwrap()).unwrap();
        assert_eq!(resumed.chat_title(), state.chat_title());
        state.apply(json!({"type":"response","command":"get_state","data":{"sessionFile":"/second","sessionId":"b","sessionName":"Other chat"}}));
        assert_eq!(state.chat_title().as_deref(), Some("Other chat"));
    }
    #[test]
    fn restored_marker_requires_current_session_and_preserves_fork_identity() {
        let mut state = State {
            session_file: Some("/fork".into()),
            session_id: Some("fork-id".into()),
            ..Default::default()
        };
        let mut status = json!({"version":1,"restored":true,"sessionFile":"/fork","sessionId":"original-id","title":"Original request","initialRequest":"a".repeat(64)});
        state.title_status(&status);
        assert!(state.title.is_none());
        status["sessionId"] = "fork-id".into();
        state.title_status(&status);
        assert_eq!(state.assigned_title(), Some("Original request"));
        assert_eq!(state.session_id.as_deref(), Some("fork-id"));
    }
    #[test]
    fn initial_assignment_is_bound_to_session_and_never_replaces_an_assignment() {
        let mut state = State {
            session_file: Some("/first".into()),
            session_id: Some("a".into()),
            title_request: Some("a".repeat(64)),
            ..Default::default()
        };
        let status = json!({"version":1,"sessionFile":"/second","sessionId":"a","title":"Wrong request","initialRequest":"a".repeat(64)});
        state.title_status(&status);
        assert!(state.title.is_none());
        let mut status = status;
        status["sessionFile"] = "/first".into();
        status["sessionId"] = "old-epoch".into();
        state.title_status(&status);
        assert!(state.title.is_none());
        status["sessionId"] = "a".into();
        status["title"] = "Fix Hebrew sidebar".into();
        state.title_status(&status);
        status["title"] = "Late background result".into();
        state.title_status(&status);
        assert_eq!(state.assigned_title(), Some("Fix Hebrew sidebar"));
    }
}
