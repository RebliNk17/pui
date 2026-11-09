//! Pi RPC omits partial snapshots on message_update. Rebuild indexed blocks from
//! their own deltas, not from the possibly already-mutated message_start seed.
use super::*;
use std::collections::HashMap;

#[cfg(test)]
mod tests;

#[derive(Clone, Default)]
pub(crate) struct Stream {
    raw: Value,
    deltas: HashMap<usize, String>,
}
impl Stream {
    pub(super) fn new(raw: Value) -> Self {
        Self {
            raw,
            ..Default::default()
        }
    }
    pub(super) fn snapshot(&mut self, raw: &Value) {
        self.raw = raw.clone();
        self.deltas.clear();
        if let Some(blocks) = raw["content"].as_array() {
            for (index, block) in blocks.iter().enumerate() {
                let text = match string(block, "type") {
                    "text" => string(block, "text").to_owned(),
                    "thinking" => string(block, "thinking").to_owned(),
                    "toolCall" => block["arguments"].to_string(),
                    _ => continue,
                };
                self.deltas.insert(index, text);
            }
        }
    }
    pub(super) fn apply(&mut self, update: &Value, usage: &Value) -> Option<Message> {
        let index = usize::try_from(update["contentIndex"].as_u64()?).ok()?;
        if index > 4096 {
            return None;
        }
        let kind = string(update, "type");
        if !matches!(
            kind,
            "text_start"
                | "text_delta"
                | "text_end"
                | "thinking_start"
                | "thinking_delta"
                | "thinking_end"
                | "toolcall_start"
                | "toolcall_delta"
                | "toolcall_end"
        ) {
            return None;
        }
        if !self.raw["content"].is_array() {
            self.raw["content"] = json!([]);
        }
        let blocks = self.raw["content"].as_array_mut()?;
        blocks.resize(blocks.len().max(index + 1), Value::Null);
        match kind {
            "text_start" | "thinking_start" => {
                self.deltas.insert(index, String::new());
                blocks[index] = if kind == "text_start" {
                    json!({"type":"text","text":""})
                } else {
                    json!({"type":"thinking","thinking":""})
                };
            }
            "text_delta" | "thinking_delta" | "text_end" | "thinking_end" => {
                // The first delta replaces any shallow seed; subsequent deltas
                // append, even if their text is identical. End is authoritative.
                let text = self.deltas.entry(index).or_default();
                if kind.ends_with("_end") {
                    *text = update["content"].as_str()?.into();
                } else {
                    text.push_str(string(update, "delta"));
                }
                blocks[index] = if kind.starts_with("text_") {
                    json!({"type":"text","text":text})
                } else {
                    json!({"type":"thinking","thinking":text})
                };
            }
            "toolcall_start" => {
                self.deltas.insert(index, String::new());
                blocks[index] = json!({"type":"toolCall","id":update["id"],"name":update["toolName"],"arguments":{}});
            }
            "toolcall_delta" => {
                let args = self.deltas.entry(index).or_default();
                args.push_str(string(update, "delta"));
                if let Ok(parsed) = serde_json::from_str::<Value>(args) {
                    if blocks[index].is_object() {
                        blocks[index]["arguments"] = parsed;
                    }
                }
            }
            "toolcall_end" => {
                blocks[index] = update["toolCall"].clone();
                self.deltas.remove(&index);
            }
            _ => unreachable!(),
        }
        if usage.is_object() {
            self.raw["usage"] = usage.clone();
        }
        Some(Message::from_json(&self.raw))
    }
}

pub(super) fn replace(message: &mut Message, mut snapshot: Message) {
    snapshot.revision = message.revision;
    for tool in &mut snapshot.tools {
        if let Some(live) = message
            .tools
            .iter()
            .find(|t| t.id == tool.id && (t.finished || !t.children.is_empty()))
        {
            *tool = live.clone();
        }
    }
    *message = snapshot;
}
