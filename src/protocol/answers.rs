//! The question tool's structured result is the durable source for answer cards.
use super::*;

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct QuestionAnswers {
    pub cancelled: bool,
    pub entries: Vec<QuestionAnswer>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct QuestionAnswer {
    pub question: String,
    pub answer: String,
}
impl Tool {
    pub(super) fn set_answers(&mut self, raw: &Value) {
        if self.name != "pui_ask_questions" || raw["isError"] == true {
            return;
        }
        let details = &raw["details"];
        if details["cancelled"] == true {
            self.answers = Some(QuestionAnswers {
                cancelled: true,
                entries: vec![],
            });
            return;
        }
        let Some(answers) = details["answers"].as_array().filter(|a| a.len() <= 8) else {
            return;
        };
        let questions = details["questions"].as_array();
        let mut entries = Vec::new();
        for answer in answers {
            let question = answer["question"]
                .as_str()
                .or_else(|| {
                    questions?.iter().find(|q| q["id"] == answer["id"])?["question"].as_str()
                })
                .unwrap_or("Question")
                .to_owned();
            let mut parts = answer["selections"]
                .as_array()
                .map(|a| {
                    a.iter()
                        .filter_map(Value::as_str)
                        .map(str::to_owned)
                        .collect::<Vec<_>>()
                })
                .unwrap_or_default();
            if let Some(text) = answer["answer"].as_str().filter(|s| !s.trim().is_empty()) {
                parts.push(text.into());
            }
            let text = if answer["skipped"] == true {
                "Skipped".into()
            } else if parts.is_empty() {
                "Not answered".into()
            } else {
                parts.join("\n")
            };
            entries.push(QuestionAnswer {
                question,
                answer: text,
            });
        }
        self.answers = Some(QuestionAnswers {
            cancelled: false,
            entries,
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn answers_rehydrate_from_live_results_history_and_shared_snapshots() {
        let call = json!({"role":"assistant","content":[{"type":"toolCall","id":"questions","name":"pui_ask_questions","arguments":{"questions":[]}}]});
        let details = json!({"questions":[{"id":"start","question":"How to start?"},{"id":"checks","question":"Which checks?"},{"id":"optional","question":"Anything else?"}],"answers":[{"id":"start","answer":"New task"},{"id":"checks","selections":["Unit tests","Visual checks"],"answer":"Also accessibility","custom":true},{"id":"optional","skipped":true}],"cancelled":false});
        let result = json!({"role":"toolResult","toolCallId":"questions","content":[{"type":"text","text":"result"}],"details":details});
        let history = parse_history(&[call.clone(), result.clone()]);
        let mut state = State::default();
        state.apply(json!({"type":"message_start","message":call}));
        state.apply(json!({"type":"tool_execution_end","toolCallId":"questions","result":result}));
        for tool in [&history[0].tools[0], &state.messages[0].tools[0]] {
            let answers = tool.answers.as_ref().unwrap();
            assert_eq!(answers.entries[0].question, "How to start?");
            assert_eq!(answers.entries[0].answer, "New task");
            assert_eq!(
                answers.entries[1].answer,
                "Unit tests\nVisual checks\nAlso accessibility"
            );
            assert_eq!(answers.entries[2].answer, "Skipped");
        }
        let snapshot: State =
            serde_json::from_value(serde_json::to_value(&state).unwrap()).unwrap();
        assert_eq!(
            snapshot.messages[0].tools[0]
                .answers
                .as_ref()
                .unwrap()
                .entries
                .len(),
            3
        );
    }
    #[test]
    fn cancellation_is_not_an_answer_and_old_tools_need_no_new_field() {
        let mut tool: Tool = serde_json::from_value(
            serde_json::to_value(Tool {
                name: "pui_ask_questions".into(),
                ..Default::default()
            })
            .unwrap(),
        )
        .unwrap();
        tool.set_answers(&json!({"details":{"cancelled":true,"answers":[]}}));
        assert!(tool.answers.as_ref().unwrap().cancelled);
        assert!(tool.answers.as_ref().unwrap().entries.is_empty());
        let mut old = serde_json::to_value(&tool).unwrap();
        old.as_object_mut().unwrap().remove("answers");
        assert!(
            serde_json::from_value::<Tool>(old)
                .unwrap()
                .answers
                .is_none()
        );
    }
}
