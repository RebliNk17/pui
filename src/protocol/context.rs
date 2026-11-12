//! Adapter estimates describe the active model context, not the displayed transcript.
use super::State;
use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct ContextBreakdown {
    pub system: f64,
    pub tools: f64,
    pub mcp_tools: f64,
    pub user: f64,
    pub assistant: f64,
    pub tool_results: f64,
    pub other: f64,
}

impl ContextBreakdown {
    pub fn values(&self) -> [f64; 7] {
        [
            self.system,
            self.tools,
            self.mcp_tools,
            self.user,
            self.assistant,
            self.tool_results,
            self.other,
        ]
    }

    pub fn total(&self) -> f64 {
        self.values().iter().sum()
    }

    /// Attribute Pi's total proportionally; never imply a tokenizer measured each part.
    pub fn scaled(&self, total: f64) -> Self {
        let scale = if self.total() > 0.0 {
            total / self.total()
        } else {
            0.0
        };
        Self {
            system: self.system * scale,
            tools: self.tools * scale,
            mcp_tools: self.mcp_tools * scale,
            user: self.user * scale,
            assistant: self.assistant * scale,
            tool_results: self.tool_results * scale,
            other: self.other * scale,
        }
    }
}

impl State {
    pub(super) fn apply_context(&mut self, value: &Value) {
        if value["version"] != 1
            || (self.session_file.is_some()
                && value["sessionFile"].as_str() != self.session_file.as_deref())
        {
            return;
        }
        self.context_breakdown = serde_json::from_value::<ContextBreakdown>(value["parts"].clone())
            .ok()
            .filter(|parts| {
                parts.values().iter().all(|v| v.is_finite() && *v >= 0.0)
                    && parts.total().is_finite()
                    && parts.total() > 0.0
            });
        if let Some(window) = value["contextWindow"].as_f64().filter(|v| *v > 0.0) {
            self.context_window = window;
            self.context_tokens = value["tokens"].as_f64().filter(|v| *v >= 0.0);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn status(parts: Value, tokens: Value) -> Value {
        json!({"type":"extension_ui_request", "method":"setStatus", "statusKey":"pui-context",
            "statusText":json!({"version":1,"sessionFile":"current","parts":parts,"tokens":tokens,"contextWindow":10000}).to_string()})
    }

    fn parts() -> Value {
        json!({"system":100,"tools":200,"mcp_tools":300,"user":400,"assistant":500,"tool_results":600,"other":0})
    }

    #[test]
    fn estimates_scale_to_reported_usage_and_survive_roundtrip() {
        let mut state = State {
            session_file: Some("current".into()),
            ..Default::default()
        };
        state.apply(status(parts(), json!(4200)));
        let restored: State =
            serde_json::from_value(serde_json::to_value(&state).unwrap()).unwrap();
        let scaled = restored
            .context_breakdown
            .unwrap()
            .scaled(restored.context_tokens.unwrap());
        assert_eq!(scaled.total(), 4200.0);
        assert_eq!(scaled.tool_results, 1200.0);
    }

    #[test]
    fn invalid_and_unavailable_parts_clear_old_data_but_other_sessions_cannot() {
        let mut state = State {
            session_file: Some("current".into()),
            ..Default::default()
        };
        state.apply(status(parts(), json!(4200)));
        state.apply_context(&json!({"version":1,"sessionFile":"old","parts":null}));
        assert!(state.context_breakdown.is_some());
        let mut invalid = parts();
        invalid["user"] = json!(-1);
        state.apply(status(invalid, json!(4200)));
        assert!(state.context_breakdown.is_none());
        state.apply(status(parts(), json!(4200)));
        state.apply(status(Value::Null, Value::Null));
        assert!(state.context_breakdown.is_none());
    }

    #[test]
    fn session_changes_clear_categories_but_model_ack_keeps_the_fresh_adapter_update() {
        let mut state = State {
            session_file: Some("current".into()),
            ..Default::default()
        };
        state.apply(status(parts(), json!(4200)));
        // Pi emits model_select status before acknowledging set_model.
        state.apply(
            json!({"type":"response", "command":"set_model", "success":true,
            "data":{"id":"new-model","provider":"fixture"}}),
        );
        assert!(state.context_breakdown.is_some());
        state.apply(
            json!({"type":"response", "command":"get_state", "success":true,
            "data":{"sessionFile":"different"}}),
        );
        assert!(state.context_breakdown.is_none());
        state.apply(status(parts(), json!(4200)));
        assert!(
            state.context_breakdown.is_none(),
            "previous session's status must be ignored"
        );
    }

    #[test]
    fn compaction_drops_old_split_and_allows_new_estimate_without_reported_total() {
        let mut state = State {
            session_file: Some("current".into()),
            ..Default::default()
        };
        state.apply(status(parts(), json!(4200)));
        state.apply(json!({"type":"compaction_start"}));
        assert!(state.context_breakdown.is_none());
        state.apply(status(parts(), Value::Null));
        assert!(state.context_tokens.is_none());
        assert_eq!(state.context_breakdown.unwrap().total(), 2100.0);
    }
}
