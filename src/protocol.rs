//! Pi's JSONL protocol, kept separate from the renderer and process transport.
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

mod answers;
mod stream;
pub use answers::QuestionAnswers;

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct Message {
    pub role: String,
    pub text: String,
    pub thinking: String,
    pub model: String,
    pub error: Option<String>,
    pub tools: Vec<Tool>,
    pub images: Vec<crate::media::Attachment>,
    pub blocks: Vec<Block>,
    pub timestamp: i64,
    pub cost: f64,
    pub cached_pct: Option<f64>,
    pub effort: String,
    pub revision: u64,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum Block {
    Text(String),
    Thinking(String),
    Tool(String),
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct Tool {
    pub id: String,
    pub name: String,
    pub args: String,
    pub output: String,
    pub failed: bool,
    pub finished: bool,
    pub path: String,
    pub command: String,
    pub diff: String,
    pub added: usize,
    pub removed: usize,
    pub duration_ms: Option<u64>,
    pub code: String,
    pub children: Vec<Tool>,
    #[serde(default)]
    pub answers: Option<QuestionAnswers>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Model {
    pub id: String,
    pub name: String,
    pub provider: String,
    pub context_window: u64,
    pub input_cost: f64,
    pub output_cost: f64,
}

impl Model {
    fn from_json(value: &Value) -> Self {
        Self {
            id: string(value, "id").into(),
            name: string(value, "name").into(),
            provider: string(value, "provider").into(),
            context_window: value["contextWindow"].as_u64().unwrap_or_default(),
            input_cost: value["cost"]["input"].as_f64().unwrap_or_default(),
            output_cost: value["cost"]["output"].as_f64().unwrap_or_default(),
        }
    }
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct HistoryPoint {
    #[serde(default)]
    pub role: String,
    #[serde(rename = "entryId")]
    pub entry_id: String,
    pub text: String,
    pub timestamp: i64,
    #[serde(rename = "imageCount")]
    pub image_count: usize,
    #[serde(rename = "codeReason")]
    pub code_reason: Option<String>,
}
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct SlashCommand {
    pub name: String,
    #[serde(default)]
    pub description: Option<String>,
}

#[derive(Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct State {
    pub messages: Vec<Message>,
    pub history_points: Vec<HistoryPoint>,
    pub models: Vec<Model>,
    pub commands: Vec<SlashCommand>,
    pub model: String,
    pub model_name: String,
    pub provider: String,
    pub thinking: String,
    pub levels: Vec<String>,
    pub session_file: Option<String>,
    pub session_name: String,
    pub busy: bool,
    pub(crate) run_observed: bool,
    #[serde(skip)]
    pub(crate) stream: Option<stream::Stream>,
    pub compacting: bool,
    pub permission_classifier: Option<String>,
    pub permission_mode: Option<String>,
    pub permission_cost: f64,
    pub compaction_settings_error: Option<String>,
    pub pending: bool,
    pub cost: f64,
    pub context_tokens: Option<f64>,
    pub context_window: f64,
    pub tokens: Value,
    pub transcript_revision: u64,
    pub prose_revision: u64,
    pub outputs_revision: u64,
    pub error: Option<String>,
    pub notice: Option<String>,
    pub diagnostics: Vec<String>,
    pub dialogs: std::collections::VecDeque<Value>,
}

pub fn string<'a>(value: &'a Value, key: &str) -> &'a str {
    value[key].as_str().unwrap_or("")
}

pub fn content_text(content: &Value) -> String {
    if let Some(text) = content.as_str() {
        return text.to_owned();
    }
    content
        .as_array()
        .map(|blocks| {
            blocks
                .iter()
                .filter_map(|b| match string(b, "type") {
                    "text" => b["text"].as_str().map(str::to_owned),
                    "image" => None,
                    _ => None,
                })
                .collect::<Vec<_>>()
                .join("\n\n")
        })
        .unwrap_or_default()
}

impl Message {
    pub fn from_json(value: &Value) -> Self {
        let mut message = Self {
            role: string(value, "role").to_owned(),
            text: content_text(&value["content"]),
            images: crate::media::attachments(&value["content"]),
            model: string(value, "model").to_owned(),
            error: value["errorMessage"].as_str().map(str::to_owned),
            timestamp: value["timestamp"].as_i64().unwrap_or_default(),
            cost: value["usage"]["cost"]["total"].as_f64().unwrap_or_default(),
            effort: string(value, "thinkingLevel").to_owned(),
            ..Default::default()
        };
        if message.role == "user" {
            if let Some((_, files)) =
                crate::media::file_manifest(crate::transcript::user_display_text(&message))
            {
                message
                    .images
                    .extend(files.into_iter().map(crate::media::Attachment::from_file));
            }
        }
        if let Some(blocks) = value["content"].as_array() {
            for block in blocks {
                match string(block, "type") {
                    "text" => message
                        .blocks
                        .push(Block::Text(string(block, "text").into())),
                    "thinking" => {
                        message.thinking.push_str(string(block, "thinking"));
                        message
                            .blocks
                            .push(Block::Thinking(string(block, "thinking").into()));
                    }
                    "toolCall" => {
                        let mut tool = Tool {
                            id: string(block, "id").into(),
                            name: string(block, "name").into(),
                            ..Default::default()
                        };
                        tool.set_args(&block["arguments"]);
                        message.blocks.push(Block::Tool(tool.id.clone()));
                        message.tools.push(tool);
                    }
                    _ => {}
                }
            }
        }
        let input = value["usage"]["input"].as_f64().unwrap_or_default();
        let read = value["usage"]["cacheRead"].as_f64().unwrap_or_default();
        let write = value["usage"]["cacheWrite"].as_f64().unwrap_or_default();
        if input + read + write > 0.0 {
            message.cached_pct = Some(100.0 * read / (input + read + write));
        }
        if message.role == "compactionSummary" || message.role == "branchSummary" {
            message.text = string(value, "summary").to_owned();
        }
        if message.role == "bashExecution" {
            message.text = format!(
                "```sh\n{}\n```\n```text\n{}\n```",
                string(value, "command"),
                string(value, "output")
            );
        }
        message
    }
}

impl State {
    pub fn add_diagnostic(&mut self, text: String) {
        if !text.is_empty() && !self.diagnostics.contains(&text) {
            if self.diagnostics.len() >= 20 {
                self.diagnostics.remove(0);
            }
            self.diagnostics.push(text);
        }
    }
    pub fn ready(&self) -> bool {
        !self.busy && !self.pending && !self.compacting
    }

    pub fn tool_mut(&mut self, id: &str) -> Option<&mut Tool> {
        for message in self.messages.iter_mut().rev() {
            for tool in &mut message.tools {
                if tool.id == id {
                    return Some(tool);
                }
                if let Some(child) = tool.children.iter_mut().find(|t| t.id == id) {
                    return Some(child);
                }
            }
        }
        None
    }

    pub fn apply(&mut self, event: Value) {
        if event["type"] == "message_end" || event["command"] == "get_messages" {
            self.prose_revision = self.prose_revision.wrapping_add(1);
        }
        if event["type"] == "tool_execution_end" || event["command"] == "get_messages" {
            self.outputs_revision = self.outputs_revision.wrapping_add(1);
        }
        if matches!(
            string(&event, "type"),
            "message_start"
                | "message_update"
                | "message_end"
                | "tool_execution_start"
                | "tool_execution_update"
                | "tool_execution_end"
        ) || (event["command"] == "get_messages")
        {
            self.transcript_revision = self.transcript_revision.wrapping_add(1);
        }
        if event["type"] == "extension_ui_request" && event["method"] == "setStatus" {
            if let Ok(value) = serde_json::from_str::<Value>(string(&event, "statusText")) {
                match string(&event, "statusKey") {
                    "pui-questions" => {
                        if let Some(closed) = value["closed"].as_str() {
                            self.dialogs.retain(|d| {
                                if d["method"] != "input" || d["title"] != "Pui questions" {
                                    return true;
                                }
                                serde_json::from_str::<Value>(string(d, "placeholder"))
                                    .map(|p| p["request_id"] != closed)
                                    .unwrap_or(true)
                            });
                        }
                        return;
                    }
                    "pui-history" => {
                        if self.session_file.is_none()
                            || value["sessionFile"].as_str() == self.session_file.as_deref()
                        {
                            self.history_points =
                                serde_json::from_value(value["points"].clone()).unwrap_or_default();
                        }
                        return;
                    }
                    "pui-permissions" => {
                        let mode = value["mode"].as_str().map(str::to_owned);
                        if self.permission_mode.is_some() && self.permission_mode != mode {
                            self.dialogs.retain(|d| {
                                !((d["method"] == "confirm"
                                    && string(d, "title").starts_with("Allow ")
                                    && string(d, "message")
                                        .ends_with("\n\nApprove this call only."))
                                    || (d["method"] == "select"
                                        && string(d, "title").starts_with("Allow ")
                                        && string(d, "title")
                                            .ends_with("\n\nApprove this exact action.")
                                        && d["options"]
                                            == json!([
                                                "Allow once",
                                                "Always allow this exact action (this chat)",
                                                "Deny"
                                            ])))
                            });
                        }
                        self.permission_mode = mode;
                        self.permission_classifier =
                            value["classifier"].as_str().map(str::to_owned);
                        self.permission_cost = value["classifierCost"].as_f64().unwrap_or(0.0);
                        return;
                    }
                    "pui-compaction" => {
                        self.compacting = value["working"] == true;
                        self.compaction_settings_error = value["error"].as_str().map(str::to_owned);
                        return;
                    }
                    _ => {}
                }
            }
        }
        match string(&event, "type") {
            "response" => {
                if event["success"] == false {
                    self.error = Some(string(&event, "error").to_owned());
                    if string(&event, "command") == "prompt" {
                        self.pending = false;
                    }
                    return;
                }
                let data = &event["data"];
                match string(&event, "command") {
                    "get_state" => {
                        self.model = string(&data["model"], "id").to_owned();
                        self.model_name = string(&data["model"], "name").into();
                        self.provider = string(&data["model"], "provider").into();
                        self.thinking = string(data, "thinkingLevel").to_owned();
                        if !self.pending {
                            self.busy = self.run_observed || data["isStreaming"] == true;
                        }
                        self.compacting = data["isCompacting"] == true;
                        self.session_name = string(data, "sessionName").to_owned();
                        self.session_file = data["sessionFile"].as_str().map(str::to_owned);
                    }
                    "get_messages" => {
                        self.stream = None;
                        self.messages = parse_history(
                            data["messages"]
                                .as_array()
                                .map(Vec::as_slice)
                                .unwrap_or_default(),
                        );
                    }
                    "get_commands" => {
                        self.commands = data["commands"]
                            .as_array()
                            .into_iter()
                            .flatten()
                            .filter_map(|v| serde_json::from_value(v.clone()).ok())
                            .collect();
                    }
                    "get_available_models" => {
                        self.models = data["models"]
                            .as_array()
                            .map(|models| models.iter().map(Model::from_json).collect())
                            .unwrap_or_default();
                    }
                    "get_available_thinking_levels" => {
                        self.levels = data["levels"]
                            .as_array()
                            .map(|l| {
                                l.iter()
                                    .filter_map(|v| v.as_str().map(str::to_owned))
                                    .collect()
                            })
                            .unwrap_or_default();
                    }
                    "get_session_stats" => {
                        self.cost = data["cost"].as_f64().unwrap_or(0.0);
                        self.tokens = data["tokens"].clone();
                        self.context_tokens = data["contextUsage"]["tokens"].as_f64();
                        self.context_window = data["contextUsage"]["contextWindow"]
                            .as_f64()
                            .unwrap_or(0.0);
                    }
                    "prompt" => {
                        // A fast run may already have settled before command acceptance arrives.
                        if self.pending && string(data, "disposition") == "started" {
                            self.busy = true;
                        }
                        self.pending = false;
                    }
                    "new_session" | "switch_session" if data["cancelled"] == true => {
                        self.notice = Some("Session change cancelled by an extension.".into())
                    }
                    _ => {}
                }
            }
            "agent_start" => {
                self.run_observed = true;
                self.busy = true;
                self.pending = false;
            }
            // agent_end is NOT idle: retries and compaction can still follow.
            "agent_settled" => {
                self.stream = None;
                self.dialogs
                    .retain(|d| d["method"] != "input" || d["title"] != "Pui questions");
                self.run_observed = false;
                self.busy = false;
                self.pending = false;
            }
            "compaction_start" => self.compacting = true,
            "compaction_end" => {
                self.compacting = false;
                if let Some(error) = event["errorMessage"].as_str() {
                    self.error = Some(error.into());
                }
            }
            "message_start" => {
                let raw = &event["message"];
                let role = string(raw, "role");
                if role == "assistant" {
                    self.stream = Some(stream::Stream::new(raw.clone()));
                }
                if matches!(role, "user" | "assistant")
                    || (role == "custom" && raw["display"] != false)
                {
                    self.messages.push(Message::from_json(raw));
                }
            }
            "message_update" => {
                let update = &event["assistantMessageEvent"];
                if let Some(message) = self.messages.last_mut().filter(|m| m.role == "assistant") {
                    message.revision = message.revision.wrapping_add(1);
                    // Pi snapshots already include this event's delta. A shallow
                    // message_start snapshot can even contain the first token.
                    // Replace from the authoritative snapshot, never append it twice.
                    if event["message"]["role"] == "assistant"
                        && event["message"]["content"].is_array()
                    {
                        if let Some(stream) = self.stream.as_mut() {
                            stream.snapshot(&event["message"]);
                        }
                        stream::replace(message, Message::from_json(&event["message"]));
                    } else if update["contentIndex"].as_u64().is_some() {
                        if let Some(snapshot) = self
                            .stream
                            .as_mut()
                            .and_then(|s| s.apply(update, &event["usage"]))
                        {
                            stream::replace(message, snapshot);
                        }
                    } else {
                        match string(update, "type") {
                            "text_delta" => {
                                message.text.push_str(string(update, "delta"));
                                match message.blocks.last_mut() {
                                    Some(Block::Text(text)) => {
                                        text.push_str(string(update, "delta"))
                                    }
                                    _ => message
                                        .blocks
                                        .push(Block::Text(string(update, "delta").into())),
                                }
                            }
                            "thinking_delta" => {
                                message.thinking.push_str(string(update, "delta"));
                                match message.blocks.last_mut() {
                                    Some(Block::Thinking(text)) => {
                                        text.push_str(string(update, "delta"))
                                    }
                                    _ => message
                                        .blocks
                                        .push(Block::Thinking(string(update, "delta").into())),
                                }
                            }
                            _ => {}
                        }
                    }
                }
            }
            "message_end" => {
                let raw = &event["message"];
                let role = string(raw, "role");
                if role == "assistant" {
                    self.stream = None;
                }
                if matches!(role, "user" | "assistant")
                    || (role == "custom" && raw["display"] != false)
                {
                    let final_message = Message::from_json(raw);
                    if let Some(last) = self.messages.last_mut().filter(|m| m.role == role) {
                        let mut final_message = final_message;
                        if role == "assistant" {
                            for tool in &mut final_message.tools {
                                if let Some(live) = last.tools.iter().find(|t| {
                                    t.id == tool.id && (t.finished || !t.children.is_empty())
                                }) {
                                    *tool = live.clone();
                                }
                            }
                        }
                        let revision = last.revision.wrapping_add(1);
                        *last = final_message;
                        last.revision = revision;
                    }
                }
            }
            "tool_execution_start" => {
                let id = string(&event, "toolCallId");
                if self.tool_mut(id).is_none() {
                    if let Some(message) = self.messages.last_mut() {
                        let tool = Tool {
                            id: id.into(),
                            name: string(&event, "toolName").into(),
                            ..Default::default()
                        };
                        let parent = string(&event, "parentToolCallId");
                        if let Some(wrapper) = message.tools.iter_mut().find(|t| t.id == parent) {
                            wrapper.children.push(tool);
                        } else {
                            message.blocks.push(Block::Tool(id.into()));
                            message.tools.push(tool);
                        }
                    }
                }
                if let Some(tool) = self.tool_mut(id) {
                    tool.added = 0;
                    tool.removed = 0;
                    tool.set_args(&event["args"]);
                }
            }
            "tool_execution_update" | "tool_execution_end" => {
                if event["type"] == "tool_execution_end" {
                    let id = string(&event, "toolCallId");
                    if let Some((owner, tool)) =
                        self.messages.iter().enumerate().rev().find_map(|(i, m)| {
                            m.tools
                                .iter()
                                .position(|t| t.id == id || t.children.iter().any(|c| c.id == id))
                                .map(|t| (i, t))
                        })
                    {
                        let wrapper = &self.messages[owner].tools[tool];
                        let call = wrapper
                            .children
                            .iter()
                            .find(|t| t.id == id)
                            .unwrap_or(wrapper);
                        let path = (call.name == "read" && event["isError"] != true)
                            .then(|| call.path.clone());
                        place_tool_images(
                            &mut self.messages,
                            owner,
                            path.as_deref(),
                            crate::media::attachments(&event["result"]["content"]),
                        );
                    }
                }
                if let Some(tool) = self.tool_mut(string(&event, "toolCallId")) {
                    let finished = string(&event, "type") == "tool_execution_end";
                    tool.output = content_text(
                        &event[if finished { "result" } else { "partialResult" }]["content"],
                    );
                    if tool.name == "codemode" {
                        tool.set_nested(&event[if finished { "result" } else { "partialResult" }]);
                    }
                    if finished {
                        tool.set_answers(&event["result"]);
                    }
                    tool.finished = finished;
                    tool.failed = event["isError"] == true;
                    tool.duration_ms = event["durationMs"].as_u64();
                }
            }
            "session_info_changed" => self.session_name = string(&event, "name").into(),
            "thinking_level_changed" => self.thinking = string(&event, "level").into(),
            "extension_error" => self.add_diagnostic(string(&event, "error").into()),
            "auto_retry_start" => {
                self.notice = Some(format!("Retrying: {}", string(&event, "errorMessage")))
            }
            "auto_retry_end" if event["success"] == false => {
                self.error = Some(string(&event, "finalError").into())
            }
            "extension_ui_request" => match string(&event, "method") {
                "confirm" | "select" | "input" | "editor" => self.dialogs.push_back(event),
                "notify" => self.add_diagnostic(string(&event, "message").into()),
                _ => {}
            },
            _ => {}
        }
    }
}

/// Reconstruct a finalized history on a worker, with O(1) tool-result lookup.
pub fn parse_history(raw: &[Value]) -> Vec<Message> {
    let mut messages: Vec<Message> = Vec::new();
    let mut calls: std::collections::HashMap<String, (usize, usize)> =
        std::collections::HashMap::new();
    for raw in raw {
        if string(raw, "role") == "toolResult" {
            if let Some(&(message_index, tool_index)) = calls.get(string(raw, "toolCallId")) {
                let message: &mut Message = &mut messages[message_index];
                let tool = &mut message.tools[tool_index];
                tool.output = content_text(&raw["content"]);
                tool.finished = true;
                tool.failed = raw["isError"] == true;
                tool.duration_ms = raw["durationMs"].as_u64();
                tool.set_answers(raw);
                if tool.name == "codemode" {
                    tool.set_nested(raw);
                }
                let images = crate::media::attachments(&raw["content"]);
                let path = (tool.name == "read" && !tool.failed).then(|| tool.path.clone());
                // Wrapper images lack per-image provenance. Recover a screenshot
                // only when exactly one successful, referenced read is unambiguous.
                let reads: Vec<_> = tool
                    .children
                    .iter()
                    .filter(|c| c.name == "read" && c.finished && !c.failed)
                    .map(|c| c.path.clone())
                    .collect();
                let path = path.or_else(|| {
                    if images.len() != 1 {
                        return None;
                    }
                    let user = messages[..message_index]
                        .iter()
                        .rev()
                        .find(|m| m.role == "user")?;
                    let matches: Vec<_> = reads
                        .into_iter()
                        .filter(|p| referenced_image_path(&user.text, p).is_some())
                        .collect();
                    (matches.len() == 1).then(|| matches[0].clone())
                });
                place_tool_images(&mut messages, message_index, path.as_deref(), images);
            }
        } else if string(raw, "role") != "system"
            && !(string(raw, "role") == "custom" && raw["display"] == false)
        {
            let message = Message::from_json(raw);
            let index = messages.len();
            for (tool_index, tool) in message.tools.iter().enumerate() {
                calls.insert(tool.id.clone(), (index, tool_index));
            }
            messages.push(message);
        }
    }
    messages
}

/// Display-only recovery matching Harbor: a successful read of an exact path
/// mentioned by the preceding user belongs inside that user's bubble. Reuse
/// only bytes Pi already returned; never open a historical path on disk.
fn place_tool_images(
    messages: &mut [Message],
    owner: usize,
    path: Option<&str>,
    images: Vec<crate::media::Attachment>,
) {
    if images.is_empty() {
        return;
    }
    let user = messages[..owner].iter().rposition(|m| m.role == "user");
    let matched = user.and_then(|i| {
        path.and_then(|p| referenced_image_path(&messages[i].text, p))
            .map(|p| (i, p))
    });
    for image in images {
        if let Some((user, _)) = &matched {
            for message in &mut messages[user + 1..] {
                message.images.retain(|im| im.key != image.key);
            }
            if !messages[*user].images.iter().any(|im| im.key == image.key) {
                messages[*user].images.push(image);
            }
            messages[*user].revision = messages[*user].revision.wrapping_add(1);
        } else if !messages[user.unwrap_or(owner)..]
            .iter()
            .any(|m| m.images.iter().any(|im| im.key == image.key))
        {
            messages[owner].images.push(image);
        }
    }
    if let Some((user, path)) = matched {
        messages[user].text = remove_image_path(&messages[user].text, &path);
    }
}
fn referenced_image_path(text: &str, path: &str) -> Option<String> {
    if path.is_empty() {
        return None;
    }
    let alias = if let Some(p) = path.strip_prefix("/private/var/") {
        format!("/var/{p}")
    } else if path.starts_with("/var/") {
        format!("/private{path}")
    } else {
        String::new()
    };
    [path, alias.as_str()]
        .into_iter()
        .find(|p| !p.is_empty() && remove_image_path(text, p) != text)
        .map(str::to_owned)
}
fn remove_image_path(text: &str, path: &str) -> String {
    let mut result = String::new();
    let mut start = 0;
    for (index, _) in text.match_indices(path) {
        let before = text[..index].chars().next_back();
        let end = index + path.len();
        let after = text[end..].chars().next();
        if before.is_none_or(|c| c.is_whitespace() || "\"'`([{".contains(c))
            && after.is_none_or(|c| c.is_whitespace() || "\"'`])},;:!?".contains(c))
        {
            result.push_str(&text[start..index]);
            start = end;
        }
    }
    if start == 0 {
        text.to_owned()
    } else {
        result.push_str(&text[start..]);
        result.trim().to_owned()
    }
}

pub fn command(kind: &str) -> Value {
    json!({"type": kind})
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn mcp_notifications_are_deduplicated_diagnostics_not_composer_notices() {
        let mut state = State::default();
        for _ in 0..3 {
            state.apply(json!({"type":"extension_ui_request","method":"notify","message":"MCP servers need attention: HTTP 503"}));
        }
        assert_eq!(state.diagnostics.len(), 1);
        assert!(state.notice.is_none());
        assert!(state.error.is_none());
    }
    #[test]
    fn codemode_nested_metadata_rehydrates_actual_actions() {
        let messages = parse_history(&[
            json!({"role":"assistant","content":[{"type":"toolCall","id":"c","name":"codemode","arguments":{"code":"await tools.read({path: \"src/main.rs\"})"}}]}),
            json!({"role":"toolResult","toolCallId":"c","content":[],"nestedCalls":{"complete":true,"calls":[{"id":"c/1","name":"read","arguments":{"path":"src/main.rs"},"status":"ok","durationMs":3}]}}),
        ]);
        let tool = &messages[0].tools[0];
        assert_eq!(tool.children.len(), 1);
        assert!(tool.label().contains("src/main.rs"));
        assert!(tool.children[0].finished);
        assert_eq!(tool.children[0].duration_ms, Some(3));
    }
    #[test]
    fn path_screenshot_is_recovered_inside_user_bubble_without_reading_disk() {
        let raw = vec![
            json!({"role":"user","content":"/var/folders/screenshot.png look here"}),
            json!({"role":"assistant","content":[{"type":"toolCall","id":"read-1","name":"read","arguments":{"path":"/private/var/folders/screenshot.png"}}]}),
            json!({"role":"toolResult","toolCallId":"read-1","content":[{"type":"image","data":"aGVsbG8=","mimeType":"image/png"}]}),
        ];
        let messages = parse_history(&raw);
        assert_eq!(messages[0].text, "look here");
        assert_eq!(messages[0].images.len(), 1);
        assert!(messages[1].images.is_empty());
        let mut state = State::default();
        state.apply(json!({"type":"message_start","message":raw[0]}));
        state.apply(json!({"type":"message_start","message":raw[1]}));
        state.apply(json!({"type":"tool_execution_end","toolCallId":"read-1","result":{"content":raw[2]["content"]},"isError":false}));
        assert_eq!(state.messages[0].text, "look here");
        assert_eq!(state.messages[0].images.len(), 1);
        assert!(state.messages[1].images.is_empty());
    }
    #[test]
    fn screenshot_path_matching_requires_exact_boundaries_and_successful_bytes() {
        assert!(referenced_image_path("/a/image.png.bak", "/a/image.png").is_none());
        assert!(referenced_image_path("/other/image.png", "/a/image.png").is_none());
        let mut messages = vec![
            Message {
                role: "user".into(),
                text: "/a/image.png".into(),
                ..Default::default()
            },
            Message {
                role: "assistant".into(),
                ..Default::default()
            },
        ];
        place_tool_images(&mut messages, 1, Some("/a/image.png"), Vec::new());
        assert_eq!(messages[0].text, "/a/image.png");
    }
    #[test]
    fn images_are_preserved_not_replaced_with_placeholder() {
        let message = Message::from_json(
            &json!({"role":"user","content":[{"type":"image","data":"aGVsbG8=","mimeType":"image/png"},{"type":"text","text":"See this"}]}),
        );
        assert_eq!(message.text, "See this");
        assert_eq!(message.images.len(), 1);
        assert_eq!(message.images[0].data.as_ref(), "aGVsbG8=");
    }
    #[test]
    fn model_metadata_comes_from_pi() {
        let mut state = State::default();
        state.apply(json!({"type":"response","command":"get_state","success":true,"data":{"model":{"id":"id","name":"Friendly","provider":"local"},"thinkingLevel":"high"}}));
        state.apply(json!({"type":"response","command":"get_available_models","success":true,"data":{"models":[{"id":"id","name":"Friendly","provider":"local","contextWindow":200000,"cost":{"input":2,"output":10}}]}}));
        assert_eq!(state.provider, "local");
        assert_eq!(state.model_name, "Friendly");
        assert_eq!(state.models[0].context_window, 200000);
    }
    #[test]
    fn text_and_activity_preserve_original_order() {
        let message = Message::from_json(&json!({"role":"assistant","content":[
            {"type":"text","text":"Before"},{"type":"toolCall","id":"t","name":"edit","arguments":{"path":"a.rs","oldText":"old","newText":"new"}},
            {"type":"text","text":"After"}
        ]}));
        assert!(
            matches!(&message.blocks[..],[Block::Text(a),Block::Tool(_),Block::Text(b)] if a=="Before" && b=="After")
        );
        assert_eq!(message.tools[0].path, "a.rs");
        assert!(message.tools[0].diff.contains("+new"));
    }
    #[test]
    fn lifecycle_waits_for_settled() {
        let mut state = State::default();
        state.apply(json!({"type":"agent_start"}));
        state.apply(json!({"type":"agent_end"}));
        assert!(state.busy);
        state.apply(json!({"type":"agent_settled"}));
        assert!(state.ready());
    }
    #[test]
    fn streaming_snapshot_does_not_repeat_first_word_before_final_message() {
        let mut state = State::default();
        state.apply(json!({"type":"message_start","message":{"role":"assistant","content":[{"type":"text","text":"You"}]}}));
        for (delta, text) in [
            ("You", "You"),
            ("’re right", "You’re right"),
            ("—fixed", "You’re right—fixed"),
        ] {
            state.apply(json!({"type":"message_update","assistantMessageEvent":{"type":"text_delta","delta":delta},"message":{"role":"assistant","content":[{"type":"text","text":text}]}}));
            assert_eq!(state.messages[0].text, text);
            assert!(matches!(&state.messages[0].blocks[..],[Block::Text(t)] if t==text));
        }
        // Applying the same snapshot is idempotent; genuine repetition is retained.
        let raw = json!({"role":"assistant","content":[{"type":"thinking","thinking":"Think think"},{"type":"text","text":"very very good"}]});
        for _ in 0..2 {
            state.apply(json!({"type":"message_update","message":raw,"assistantMessageEvent":{"type":"text_delta","delta":"very"}}));
        }
        assert_eq!(state.messages[0].text, "very very good");
        assert_eq!(state.messages[0].thinking, "Think think");
    }
    #[test]
    fn final_message_replaces_stream_without_duplication() {
        let mut s = State::default();
        s.apply(json!({"type":"message_start","message":{"role":"assistant","content":[]}}));
        s.apply(json!({"type":"message_update","assistantMessageEvent":{"type":"text_delta","delta":"partial"}}));
        s.apply(json!({"type":"message_end","message":{"role":"assistant","content":[{"type":"text","text":"final"}]}}));
        assert_eq!(s.messages.len(), 1);
        assert_eq!(s.messages[0].text, "final");
    }
    #[test]
    fn late_state_snapshot_cannot_mark_an_active_run_idle() {
        let mut state = State::default();
        state.apply(json!({"type":"agent_start"}));
        state.apply(json!({"type":"response","command":"get_state","success":true,"data":{"isStreaming":false}}));
        assert!(state.busy);
        state.apply(json!({"type":"agent_settled"}));
        assert!(state.ready());
    }
    #[test]
    fn explicit_agent_messages_are_visible_without_becoming_user_turns() {
        let mut state = State::default();
        let raw = json!({"role":"custom","customType":"pui-agent-message","content":"Worker report","display":true});
        state.apply(json!({"type":"message_start","message":raw}));
        state.apply(json!({"type":"message_end","message":raw}));
        assert_eq!(state.messages.len(), 1);
        assert_eq!(state.messages[0].role, "custom");
        assert_eq!(state.messages[0].text, "Worker report");
    }
    #[test]
    fn rejected_prompt_releases_pending() {
        let mut s = State {
            pending: true,
            ..Default::default()
        };
        s.apply(json!({"type":"response","command":"prompt","success":false,"error":"no model"}));
        assert!(s.ready());
        assert_eq!(s.error.as_deref(), Some("no model"));
    }
    #[test]
    fn late_acceptance_does_not_restart_a_settled_run() {
        let mut s = State {
            pending: true,
            ..Default::default()
        };
        s.apply(json!({"type":"agent_start"}));
        s.apply(json!({"type":"agent_settled"}));
        s.apply(json!({"type":"response","command":"prompt","success":true,"data":{"disposition":"started"}}));
        assert!(s.ready());
    }

    #[test]
    fn handled_prompt_never_waits_for_a_run() {
        let mut s = State {
            pending: true,
            ..Default::default()
        };
        s.apply(json!({"type":"response","command":"prompt","success":true,"data":{"disposition":"handled"}}));
        assert!(s.ready());
    }

    #[test]
    fn tools_rehydrate_from_history() {
        let mut s = State::default();
        s.apply(json!({"type":"response","command":"get_messages","success":true,"data":{"messages":[
            {"role":"assistant","content":[{"type":"toolCall","id":"t1","name":"read","arguments":{"path":"a.rs"}}]},
            {"role":"toolResult","toolCallId":"t1","content":[{"type":"text","text":"hello"}],"isError":false}
        ]}}));
        let tool = &s.messages[0].tools[0];
        assert!(tool.finished);
        assert_eq!(tool.output, "hello");
    }
}
