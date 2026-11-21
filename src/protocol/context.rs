//! Adapter estimates describe the active model context, not the displayed transcript.
use super::State;
use serde::{Deserialize, Serialize};
use serde_json::Value;

/// One named entry of an expandable list: a tool, skill, memory file or agent.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(from = "(String, f64)", into = "(String, f64)")]
pub struct ContextItem {
    pub name: String,
    pub tokens: f64,
}

impl From<(String, f64)> for ContextItem {
    fn from((name, tokens): (String, f64)) -> Self {
        Self { name, tokens }
    }
}

impl From<ContextItem> for (String, f64) {
    fn from(item: ContextItem) -> Self {
        (item.name, item.tokens)
    }
}

/// Per-item sizes and tools registered but not sent to the model ("deferred").
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct ContextDetail {
    pub mcp_tools: Vec<ContextItem>,
    pub system_tools: Vec<ContextItem>,
    pub skills: Vec<ContextItem>,
    pub memory: Vec<ContextItem>,
    pub agents: Vec<ContextItem>,
    pub mcp_tools_count: usize,
    pub deferred_mcp: f64,
    pub deferred_mcp_count: usize,
    pub deferred_system: f64,
    pub deferred_system_count: usize,
}

impl ContextDetail {
    fn scaled(&self, scale: f64) -> Self {
        let items = |list: &[ContextItem]| {
            list.iter()
                .map(|i| ContextItem {
                    name: i.name.clone(),
                    tokens: i.tokens * scale,
                })
                .collect()
        };
        Self {
            mcp_tools: items(&self.mcp_tools),
            system_tools: items(&self.system_tools),
            skills: items(&self.skills),
            memory: items(&self.memory),
            agents: items(&self.agents),
            // Deferred tools are outside the active context, so they keep their raw estimate.
            ..self.clone()
        }
    }

    fn is_valid(&self) -> bool {
        self.mcp_tools
            .iter()
            .chain(&self.system_tools)
            .chain(&self.skills)
            .chain(&self.memory)
            .chain(&self.agents)
            .map(|i| i.tokens)
            .chain([self.deferred_mcp, self.deferred_system])
            .all(|v| v.is_finite() && v >= 0.0)
    }
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct ContextBreakdown {
    pub system: f64,
    pub tools: f64,
    pub mcp_tools: f64,
    pub user: f64,
    pub assistant: f64,
    pub tool_results: f64,
    pub other: f64,
    #[serde(default)]
    pub skills: f64,
    #[serde(default)]
    pub memory: f64,
    #[serde(default)]
    pub mcp_instructions: f64,
    #[serde(default)]
    pub agents: f64,
    #[serde(default)]
    pub detail: ContextDetail,
}

impl ContextBreakdown {
    pub fn values(&self) -> [f64; 11] {
        [
            self.system,
            self.tools,
            self.mcp_tools,
            self.user,
            self.assistant,
            self.tool_results,
            self.other,
            self.skills,
            self.memory,
            self.mcp_instructions,
            self.agents,
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
            skills: self.skills * scale,
            memory: self.memory * scale,
            mcp_instructions: self.mcp_instructions * scale,
            agents: self.agents * scale,
            detail: self.detail.scaled(scale),
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
        let mut parts = value["parts"].clone();
        if let Some(object) = parts.as_object_mut().filter(|_| value["detail"].is_object()) {
            object.insert("detail".into(), value["detail"].clone());
        }
        self.context_breakdown = serde_json::from_value::<ContextBreakdown>(parts)
            .ok()
            .filter(|parts| {
                parts.detail.is_valid()
                    && parts.values().iter().all(|v| v.is_finite() && *v >= 0.0)
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
    fn compaction_threshold_is_adapter_reported_and_survives_shared_snapshots() {
        let mut state = State::default();
        assert_eq!(state.compact_percent, None);
        for (percent, expected) in [(85, Some(85)), (50, Some(50)), (95, Some(95)), (100, None)] {
            state.apply(json!({"type":"extension_ui_request","method":"setStatus","statusKey":"pui-compaction",
                "statusText":json!({"percent":percent,"working":false}).to_string()}));
            let restored: State =
                serde_json::from_value(serde_json::to_value(&state).unwrap()).unwrap();
            assert_eq!(restored.compact_percent, expected);
        }
        let mut old = serde_json::to_value(&state).unwrap();
        old.as_object_mut().unwrap().remove("compact_percent");
        assert_eq!(
            serde_json::from_value::<State>(old)
                .unwrap()
                .compact_percent,
            None
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

    #[test]
    fn detail_and_new_categories_parse_scale_and_stay_backward_compatible() {
        let mut state = State {
            session_file: Some("current".into()),
            ..Default::default()
        };
        let text = json!({"version":1,"sessionFile":"current","tokens":1000,"contextWindow":10000,
            "parts":{"system":100,"tools":100,"mcp_tools":100,"user":100,"assistant":100,"tool_results":100,"other":0,
                     "skills":100,"memory":100,"mcp_instructions":100,"agents":100},
            "detail":{"mcp_tools":[["mcp__a__b",100]],"memory":[["~/x.md",100]],"mcp_tools_count":1,"deferred_mcp":50}}).to_string();
        state.apply(json!({"type":"extension_ui_request","method":"setStatus","statusKey":"pui-context","statusText":text}));
        let full = state.context_breakdown.clone().unwrap();
        assert_eq!(full.total(), 1000.0);
        let scaled = full.scaled(2000.0);
        assert!((scaled.skills - 200.0).abs() < 1e-9);
        assert_eq!(scaled.detail.mcp_tools[0].name, "mcp__a__b");
        assert!((scaled.detail.memory[0].tokens - 200.0).abs() < 1e-9);
        assert_eq!(scaled.detail.deferred_mcp, 50.0, "deferred is not rescaled");
        // Older adapters omit the new fields entirely.
        state.apply(status(parts(), json!(4200)));
        let legacy = state.context_breakdown.clone().unwrap();
        assert_eq!(legacy.skills, 0.0);
        assert!(legacy.detail.mcp_tools.is_empty());
        // Negative detail sizes invalidate the whole estimate.
        let bad = json!({"version":1,"sessionFile":"current","parts":parts(),"detail":{"skills":[["x",-1]]},"contextWindow":10000}).to_string();
        state.apply(json!({"type":"extension_ui_request","method":"setStatus","statusKey":"pui-context","statusText":bad}));
        assert!(state.context_breakdown.is_none());
    }
}
