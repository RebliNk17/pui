//! Fast, read-only tail preview while Pi initializes its tools and MCP connections.
//! This is presentation only; full display history comes from Pi's raw active
//! branch (`get_entries`), never its compacted model context (`get_messages`).
use crate::protocol::{Message, parse_history_iter};
use serde_json::Value;
use std::{
    borrow::Cow,
    collections::{HashMap, HashSet},
    fs::File,
    io::{Read, Seek, SeekFrom},
    path::Path,
};

const PREVIEW_BYTES: u64 = 2 * 1024 * 1024;
#[derive(Default)]
pub struct Preview {
    pub messages: Vec<Message>,
    pub partial: bool,
}

pub fn load(path: &Path) -> Result<Preview, String> {
    let mut file = File::open(path).map_err(|_| "Could not read saved chat.")?;
    let size = file
        .metadata()
        .map_err(|_| "Could not inspect saved chat.")?
        .len();
    let offset = size.saturating_sub(PREVIEW_BYTES);
    file.seek(SeekFrom::Start(offset))
        .map_err(|_| "Could not read saved chat.")?;
    let mut bytes = Vec::new();
    file.read_to_end(&mut bytes)
        .map_err(|_| "Could not read saved chat.")?;
    let bytes = if offset > 0 {
        bytes.splitn(2, |b| *b == b'\n').nth(1).unwrap_or_default()
    } else {
        &bytes
    };
    parse(bytes, offset > 0)
}
fn parse(bytes: &[u8], mut partial: bool) -> Result<Preview, String> {
    let mut entries = HashMap::new();
    let mut leaf = None;
    for line in bytes.split(|b| *b == b'\n') {
        if let Ok(entry) = serde_json::from_slice::<Value>(line) {
            if entry["type"] == "session" {
                continue;
            }
            if let Some(id) = entry["id"].as_str() {
                leaf = Some(id.to_owned());
                entries.insert(id.to_owned(), entry);
            }
        }
    }
    let mut chain = Vec::new();
    let mut seen = HashSet::new();
    while let Some(id) = leaf {
        if !seen.insert(id.clone()) {
            partial = true;
            break;
        }
        let Some(entry) = entries.get(&id) else {
            partial = true;
            break;
        };
        if let Some(message) = entry_message(entry) {
            chain.push(message);
        }
        leaf = entry["parentId"].as_str().map(str::to_owned);
    }
    chain.reverse();
    let mut messages = parse_history_iter(chain.iter().map(Cow::as_ref));
    // Avoid starting a preview with a fragment of the preceding assistant turn.
    if partial {
        if let Some(user) = messages.iter().position(|m| m.role == "user") {
            messages.drain(..user);
        }
    }
    Ok(Preview { messages, partial })
}
/// Reconstruct only the branch selected by Pi. Context edits and compactions
/// change model input, not the saved messages shown to the user. Do not infer the
/// leaf from file order: navigation can select an earlier entry or another root.
pub fn from_entries(entries: &[Value], leaf: Option<&str>) -> Vec<Message> {
    let by_id: HashMap<_, _> = entries
        .iter()
        .filter(|entry| entry["type"] != "session")
        .filter_map(|entry| entry["id"].as_str().map(|id| (id, entry)))
        .collect();
    let mut chain = Vec::new();
    let mut seen = HashSet::new();
    let mut next = leaf;
    while let Some(id) = next {
        if !seen.insert(id) {
            break;
        }
        let Some(entry) = by_id.get(id) else {
            break;
        };
        if let Some(message) = entry_message(entry) {
            chain.push(message);
        }
        next = entry["parentId"].as_str();
    }
    chain.reverse();
    parse_history_iter(chain.iter().map(Cow::as_ref))
}

fn entry_message(entry: &Value) -> Option<Cow<'_, Value>> {
    match entry["type"].as_str()? {
        "message" => Some(Cow::Borrowed(&entry["message"])),
        kind @ ("compaction" | "custom_message" | "branch_summary") => {
            let timestamp = entry["timestamp"]
                .as_str()
                .and_then(|s| chrono::DateTime::parse_from_rfc3339(s).ok())
                .map(|t| t.timestamp_millis())
                .unwrap_or_default();
            Some(Cow::Owned(match kind {
                "compaction" => serde_json::json!({"role":"compaction", "timestamp":timestamp}),
                "custom_message" => serde_json::json!({
                    "role":"custom", "content":entry["content"],
                    "display":entry["display"], "timestamp":timestamp
                }),
                _ => serde_json::json!({
                    "role":"branchSummary", "summary":entry["summary"], "timestamp":timestamp
                }),
            }))
        }
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    #[ignore = "local read-only preview benchmark; set PUI_BENCH_SESSION"]
    fn local_preview_benchmark() {
        let Some(path) = std::env::var_os("PUI_BENCH_SESSION") else {
            return;
        };
        let start = std::time::Instant::now();
        let preview = load(Path::new(&path)).unwrap();
        eprintln!(
            "Saved-chat preview: {} messages, partial={}, elapsed={:?}",
            preview.messages.len(),
            preview.partial,
            start.elapsed()
        );
        assert!(!preview.messages.is_empty());
    }
    #[test]
    fn preview_follows_active_branch_not_abandoned_messages() {
        let bytes=b"{\"type\":\"message\",\"id\":\"a\",\"parentId\":null,\"message\":{\"role\":\"user\",\"content\":\"start\"}}\n{\"type\":\"message\",\"id\":\"b\",\"parentId\":\"a\",\"message\":{\"role\":\"assistant\",\"content\":\"abandoned\"}}\n{\"type\":\"message\",\"id\":\"c\",\"parentId\":\"a\",\"message\":{\"role\":\"assistant\",\"content\":\"active\"}}\n";
        let preview = parse(bytes, false).unwrap();
        assert_eq!(preview.messages.len(), 2);
        assert_eq!(preview.messages[1].text, "active");
        assert!(!preview.partial);
    }
    #[test]
    fn full_history_preserves_original_messages_across_compactions_and_context_edits() {
        use serde_json::json;
        let entries = vec![
            json!({"type":"message","id":"u","parentId":null,"message":{"role":"user","content":"original prompt","timestamp":1}}),
            json!({"type":"message","id":"a","parentId":"u","message":{"role":"assistant","timestamp":2,"content":[{"type":"text","text":"original answer"},{"type":"toolCall","id":"read-1","name":"read","arguments":{"path":"src/main.rs"}}]}}),
            json!({"type":"message","id":"r","parentId":"a","message":{"role":"toolResult","toolCallId":"read-1","content":"original tool output"}}),
            json!({"type":"context_edit","id":"e","parentId":"r","targetId":"u","replacement":null}),
            json!({"type":"compaction","id":"c1","parentId":"e","timestamp":"2026-11-10T16:10:00Z","summary":"HIDDEN SUMMARY ONE","firstKeptEntryId":"r","systemMessage":{"content":"hidden prompt"}}),
            json!({"type":"custom_message","id":"m","parentId":"c1","content":"visible agent message","display":true}),
            json!({"type":"custom_message","id":"h","parentId":"m","content":"hidden agent message","display":false}),
            json!({"type":"compaction","id":"c2","parentId":"h","summary":"HIDDEN SUMMARY TWO","firstKeptEntryId":"c2"}),
            json!({"type":"message","id":"v","parentId":"c2","message":{"role":"user","content":[{"type":"text","text":"latest prompt"},{"type":"image","mimeType":"image/png","data":"AA=="}],"timestamp":3}}),
            json!({"type":"message","id":"abandoned","parentId":"u","message":{"role":"user","content":"wrong branch"}}),
        ];
        let messages = from_entries(&entries, Some("v"));
        assert_eq!(
            messages.iter().map(|m| m.role.as_str()).collect::<Vec<_>>(),
            [
                "user",
                "assistant",
                "compaction",
                "custom",
                "compaction",
                "user"
            ]
        );
        assert_eq!(messages[0].text, "original prompt");
        assert_eq!(messages[1].text, "original answer");
        assert_eq!(messages[1].tools[0].output, "original tool output");
        assert!(messages[1].tools[0].finished);
        assert_eq!(messages[2].timestamp, 1794327000000);
        assert_eq!(messages[3].text, "visible agent message");
        assert_eq!(messages[5].images.len(), 1);
        assert_eq!(
            messages.iter().filter(|m| m.role == "compaction").count(),
            2
        );
        assert!(
            messages
                .iter()
                .filter(|m| m.role == "compaction")
                .all(|m| m.text.is_empty() && m.tools.is_empty() && m.images.is_empty())
        );
        assert_eq!(from_entries(&entries, Some("a")).len(), 2);
        assert!(from_entries(&entries, None).is_empty());
    }

    #[test]
    fn preview_and_full_history_agree_on_compaction_position_without_summary_text() {
        let bytes = b"{\"type\":\"message\",\"id\":\"u\",\"parentId\":null,\"message\":{\"role\":\"user\",\"content\":\"before\"}}\n{\"type\":\"compaction\",\"id\":\"c\",\"parentId\":\"u\",\"summary\":\"SECRET SUMMARY\"}\n{\"type\":\"message\",\"id\":\"a\",\"parentId\":\"c\",\"message\":{\"role\":\"assistant\",\"content\":\"after\"}}\n";
        let preview = parse(bytes, false).unwrap();
        let entries: Vec<Value> = bytes
            .split(|b| *b == b'\n')
            .filter_map(|line| serde_json::from_slice(line).ok())
            .collect();
        let full = from_entries(&entries, Some("a"));
        assert_eq!(
            serde_json::to_value(&preview.messages).unwrap(),
            serde_json::to_value(&full).unwrap()
        );
        assert_eq!(full[1].role, "compaction");
        assert!(full[1].text.is_empty());
    }

    #[test]
    fn selected_leaf_handles_other_roots_missing_links_and_cycles() {
        use serde_json::json;
        let entries = vec![
            json!({"type":"message","id":"a","parentId":null,"message":{"role":"user","content":"root a"}}),
            json!({"type":"message","id":"b","parentId":null,"message":{"role":"user","content":"root b"}}),
            json!({"type":"message","id":"broken","parentId":"missing","message":{"role":"user","content":"tail"}}),
            json!({"type":"message","id":"cycle","parentId":"cycle","message":{"role":"user","content":"cycle"}}),
        ];
        assert_eq!(from_entries(&entries, Some("a"))[0].text, "root a");
        assert_eq!(from_entries(&entries, Some("b"))[0].text, "root b");
        assert_eq!(from_entries(&entries, Some("broken")).len(), 1);
        assert_eq!(from_entries(&entries, Some("cycle")).len(), 1);
        assert!(from_entries(&entries, Some("missing")).is_empty());
    }

    #[test]
    fn incomplete_tail_is_marked_partial() {
        let preview=parse(b"{\"type\":\"message\",\"id\":\"a\",\"parentId\":\"missing\",\"message\":{\"role\":\"user\",\"content\":\"tail\"}}",true).unwrap();
        assert!(preview.partial);
        assert_eq!(preview.messages.len(), 1);
    }
}
