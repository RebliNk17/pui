//! Prompt cache usage excludes generated tokens and is weighted by input tokens.
use super::{Message, Value};
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct PromptUsage {
    pub input: f64,
    pub cache_read: f64,
    pub cache_write: f64,
}

impl PromptUsage {
    pub(super) fn from_json(usage: &Value) -> Self {
        Self {
            input: usage["input"].as_f64().unwrap_or_default(),
            cache_read: usage["cacheRead"].as_f64().unwrap_or_default(),
            cache_write: usage["cacheWrite"].as_f64().unwrap_or_default(),
        }
    }

    pub fn from_messages<'a>(messages: impl IntoIterator<Item = &'a Message>) -> Self {
        let mut total = Self::default();
        for message in messages {
            total.input += message.prompt_usage.input;
            total.cache_read += message.prompt_usage.cache_read;
            total.cache_write += message.prompt_usage.cache_write;
        }
        total
    }

    pub fn last_response(messages: &[Message]) -> Self {
        Self::from_messages(
            messages
                .iter()
                .rev()
                .skip_while(|m| m.role != "assistant")
                .take_while(|m| m.role == "assistant"),
        )
    }

    pub fn total(&self) -> f64 {
        self.input + self.cache_read + self.cache_write
    }

    pub fn cached_pct(&self) -> Option<f64> {
        (self.total() > 0.0).then(|| 100.0 * self.cache_read / self.total())
    }

    pub fn cache_label(&self) -> Option<String> {
        let pct = self.cached_pct()?;
        // Rounding must not claim an entirely cached (or uncached) prompt.
        let percent = if self.cache_read == self.total() {
            "100%".into()
        } else if pct >= 99.95 {
            ">99.9%".into()
        } else if pct > 0.0 && pct < 0.05 {
            "<0.1%".into()
        } else if pct.fract() == 0.0 {
            format!("{pct:.0}%")
        } else {
            format!("{pct:.1}%")
        };
        Some(format!("{percent} cached"))
    }

    pub fn cache_tooltip(&self) -> String {
        format!(
            "Prompt cache reads across this response's model calls.\n{:.0} cached / {:.0} total input tokens\n{:.0} uncached · {:.0} cache writes\nGenerated output tokens are not included.",
            self.cache_read,
            self.total(),
            self.input,
            self.cache_write,
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn message(usage: Value) -> Message {
        Message::from_json(&json!({"role":"assistant", "usage":usage}))
    }

    #[test]
    fn cache_rate_uses_normalized_input_including_cache_writes_but_not_output() {
        for (usage, expected) in [
            (json!({"input":100, "output":900}), Some(0.0)),
            (json!({"cacheWrite":100}), Some(0.0)),
            (
                json!({"input":10, "cacheRead":80, "cacheWrite":10, "output":900}),
                Some(80.0),
            ),
            (json!({"input":20, "cacheRead":80}), Some(80.0)),
            (json!({"cacheRead":100}), Some(100.0)),
            (json!({"input":0, "cacheRead":0, "cacheWrite":0}), None),
            (json!({"output":100}), None),
            (Value::Null, None),
        ] {
            assert_eq!(message(usage).prompt_usage.cached_pct(), expected);
        }
    }

    #[test]
    fn response_cache_rate_is_token_weighted_not_the_last_call_or_average_percent() {
        let messages = [
            message(json!({"input":900})),
            message(json!({"cacheRead":100})),
        ];
        assert_eq!(
            PromptUsage::from_messages(&messages).cached_pct(),
            Some(10.0)
        );
    }

    #[test]
    fn screenshot_turn_is_99_point_4_percent_not_100() {
        let messages = [
            message(json!({"input":15, "cacheRead":1187349, "cacheWrite":8446})),
            message(json!({"input":3, "cacheRead":243947, "cacheWrite":205})),
        ];
        let usage = PromptUsage::from_messages(&messages);
        assert_eq!(usage.total(), 1439965.0);
        assert_eq!(usage.cache_label().as_deref(), Some("99.4% cached"));
        assert_eq!(
            messages[1].prompt_usage.cache_label().as_deref(),
            Some("99.9% cached")
        );
        assert!(
            usage
                .cache_tooltip()
                .contains("1431296 cached / 1439965 total input tokens")
        );
    }

    #[test]
    fn partial_cache_hits_never_round_to_all_or_nothing() {
        for (read, input, label) in [
            (0.0, 100.0, "0% cached"),
            (1.0, 9999.0, "<0.1% cached"),
            (994.0, 6.0, "99.4% cached"),
            (999.0, 1.0, "99.9% cached"),
            (9999.0, 1.0, ">99.9% cached"),
            (100.0, 0.0, "100% cached"),
        ] {
            let usage = message(json!({"input":input, "cacheRead":read})).prompt_usage;
            assert_eq!(usage.cache_label().as_deref(), Some(label));
        }
        assert!(PromptUsage::default().cache_label().is_none());
    }

    #[test]
    fn last_response_matches_transcript_group_boundaries_without_stale_usage() {
        let mut messages = vec![
            message(json!({"cacheRead":10000})),
            Message {
                role: "user".into(),
                ..Default::default()
            },
            message(json!({"input":900})),
            message(json!({"cacheRead":100})),
            Message {
                role: "user".into(),
                ..Default::default()
            },
        ];
        assert_eq!(
            PromptUsage::last_response(&messages).cached_pct(),
            Some(10.0)
        );
        messages.push(Message {
            role: "compaction".into(),
            ..Default::default()
        });
        messages.push(message(Value::Null));
        assert!(PromptUsage::last_response(&messages).cached_pct().is_none());
        assert!(PromptUsage::last_response(&[]).cached_pct().is_none());
    }

    #[test]
    fn live_completion_and_history_reload_preserve_the_same_usage() {
        let calls = [
            json!({"role":"assistant", "content":[], "usage":{"input":900}}),
            json!({"role":"assistant", "content":[], "usage":{"cacheRead":100}}),
        ];
        let mut live = super::super::State::default();
        for call in &calls {
            live.apply(
                json!({"type":"message_start", "message":{"role":"assistant", "content":[]}}),
            );
            live.apply(json!({"type":"message_end", "message":call}));
        }
        let mut history = super::super::State::default();
        history.apply(json!({"type":"response", "command":"get_messages", "success":true, "data":{"messages":calls}}));
        for state in [live, history] {
            let restored: super::super::State =
                serde_json::from_value(serde_json::to_value(&state).unwrap()).unwrap();
            assert_eq!(
                PromptUsage::last_response(&restored.messages)
                    .cache_label()
                    .as_deref(),
                Some("10% cached")
            );
        }
    }

    #[test]
    fn older_snapshots_without_usage_do_not_invent_measurements() {
        let old = Message {
            role: "assistant".into(),
            ..Default::default()
        };
        let mut json = serde_json::to_value(old).unwrap();
        json.as_object_mut().unwrap().remove("prompt_usage");
        json["cached_pct"] = json!(100.0);
        let decoded: Message = serde_json::from_value(json).unwrap();
        assert!(decoded.prompt_usage.cache_label().is_none());
    }
}
