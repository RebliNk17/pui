//! Bounded, read-only message search. Never indexes tools, reasoning or images.
use super::*;
use std::{
    collections::{HashMap, HashSet},
    fs::File,
    io::{Read, Seek, SeekFrom},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
};

const FILE_BYTES: u64 = 8 * 1024 * 1024;
const TOTAL_BYTES: u64 = 64 * 1024 * 1024;
const TEXT_CHARS: usize = 64 * 1024;
const MAX_ENTRIES: usize = 32_000;
pub(super) const MAX_HITS: usize = 60;

#[derive(Clone, Debug)]
pub(super) struct Text {
    pub role: String,
    pub timestamp: i64,
    pub text: String,
}
impl Text {
    pub fn from_message(message: &Message) -> Option<Self> {
        if !matches!(message.role.as_str(), "user" | "assistant") {
            return None;
        }
        let text = if message.role == "user" {
            crate::transcript::user_display_text(message)
        } else {
            &message.text
        };
        Some(Self {
            role: message.role.clone(),
            timestamp: message.timestamp,
            text: text.chars().take(TEXT_CHARS).collect(),
        })
    }
}

pub(super) struct Source {
    pub candidate: Candidate,
    // Loaded live history is authoritative, even when empty. Do not read a
    // different saved leaf for a connected chat or replay its abandoned branch.
    pub live: Option<Vec<Text>>,
    pub limited: bool,
}
#[derive(Default)]
pub(super) struct Found {
    pub hits: Vec<Hit>,
    pub limited: bool,
    pub unreadable: usize,
}

pub(super) fn matches(text: &str, query: &str) -> bool {
    let text = text.to_lowercase();
    query.split_whitespace().all(|word| text.contains(word))
}

pub(super) fn excerpt(text: &str, query: &str) -> String {
    let chars: Vec<_> = text.chars().collect();
    // Work in characters: lowercasing may change UTF-8 length (e.g. İ), so a
    // byte offset in a lowercase string is not a safe original-string offset.
    let word = query.split_whitespace().next().unwrap_or_default();
    let mut lower = String::new();
    let mut offsets = Vec::new();
    for (index, ch) in chars.iter().enumerate() {
        for folded in ch.to_lowercase() {
            offsets.extend(std::iter::repeat_n(index, folded.len_utf8()));
            lower.push(folded);
        }
    }
    let first = lower
        .find(word)
        .and_then(|byte| offsets.get(byte).copied())
        .unwrap_or(0);
    let start = first.saturating_sub(40);
    let end = (start + 190).min(chars.len());
    let body: String = chars[start..end].iter().collect();
    format!(
        "{}{}{}",
        if start > 0 { "…" } else { "" },
        body.split_whitespace().collect::<Vec<_>>().join(" "),
        if end < chars.len() { "…" } else { "" }
    )
}

pub(super) fn search(sources: Vec<Source>, query: &str, cancel: &Arc<AtomicBool>) -> Found {
    let mut found = Found::default();
    let mut remaining = TOTAL_BYTES;
    for source in sources {
        if cancel.load(Ordering::Relaxed) {
            break;
        }
        found.limited |= source.limited;
        let messages = match source.live {
            Some(messages) => messages,
            None => {
                let Some(path) = &source.candidate.path else {
                    continue;
                };
                if remaining == 0 {
                    found.limited = true;
                    break;
                }
                match read(path, &mut remaining, cancel) {
                    Ok((messages, partial)) => {
                        found.limited |= partial;
                        messages
                    }
                    Err(_) => {
                        found.unreadable += 1;
                        continue;
                    }
                }
            }
        };
        for message in messages.into_iter().rev() {
            if cancel.load(Ordering::Relaxed) {
                return found;
            }
            if !message.text.trim().is_empty() && matches(&message.text, query) {
                found.hits.push(Hit {
                    candidate: source.candidate.clone(),
                    detail: excerpt(&message.text, query),
                    kind: Kind::Message,
                    message: Some(Locator {
                        role: message.role,
                        timestamp: message.timestamp,
                        fingerprint: text_fingerprint(&message.text),
                    }),
                });
                if found.hits.len() >= MAX_HITS {
                    found.limited = true;
                    return found;
                }
            }
        }
    }
    found
}

fn read(
    path: &std::path::Path,
    remaining: &mut u64,
    cancel: &AtomicBool,
) -> Result<(Vec<Text>, bool), ()> {
    let metadata = std::fs::symlink_metadata(path).map_err(|_| ())?;
    if !metadata.is_file() || metadata.file_type().is_symlink() {
        return Err(());
    }
    let mut file = File::open(path).map_err(|_| ())?;
    let size = file.metadata().map_err(|_| ())?.len();
    let count = FILE_BYTES.min(*remaining);
    let offset = size.saturating_sub(count);
    file.seek(SeekFrom::Start(offset)).map_err(|_| ())?;
    let mut bytes = Vec::new();
    file.take(count.min(size))
        .read_to_end(&mut bytes)
        .map_err(|_| ())?;
    *remaining = remaining.saturating_sub(bytes.len() as u64);
    let bytes = if offset > 0 {
        bytes.splitn(2, |b| *b == b'\n').nth(1).unwrap_or_default()
    } else {
        &bytes
    };
    Ok(parse(bytes, offset > 0, cancel))
}

// Deserialize only searchable text and ancestry; unknown tool/image/reasoning
// fields are skipped by serde rather than retained as JSON object graphs.
#[derive(Deserialize)]
struct Record {
    #[serde(rename = "type")]
    kind: String,
    id: Option<String>,
    #[serde(rename = "parentId")]
    parent: Option<String>,
    message: Option<RawMessage>,
}
#[derive(Deserialize)]
struct RawMessage {
    role: String,
    #[serde(default)]
    timestamp: i64,
    #[serde(default)]
    content: Content,
}
#[derive(Default, Deserialize)]
#[serde(untagged)]
enum Content {
    Text(String),
    Blocks(Vec<TextBlock>),
    #[default]
    Empty,
}
#[derive(Deserialize)]
struct TextBlock {
    #[serde(rename = "type")]
    kind: String,
    text: Option<String>,
}
impl Content {
    fn text(self) -> String {
        match self {
            Self::Text(text) => text,
            Self::Blocks(blocks) => blocks
                .into_iter()
                .filter(|b| b.kind == "text")
                .filter_map(|b| b.text)
                .collect::<Vec<_>>()
                .join("\n\n"),
            Self::Empty => String::new(),
        }
    }
}

fn parse(bytes: &[u8], mut partial: bool, cancel: &AtomicBool) -> (Vec<Text>, bool) {
    struct Entry {
        parent: Option<String>,
        message: Option<Text>,
    }
    let mut entries = HashMap::new();
    let mut leaf = None;
    for line in bytes.split(|b| *b == b'\n') {
        if cancel.load(Ordering::Relaxed) {
            return (Vec::new(), true);
        }
        if line.iter().all(u8::is_ascii_whitespace) {
            continue;
        }
        let Ok(entry) = serde_json::from_slice::<Record>(line) else {
            // A malformed final record must not silently select a previous,
            // potentially abandoned branch. A later valid entry can restore it.
            partial = true;
            leaf = None;
            continue;
        };
        if entry.kind == "session" {
            continue;
        }
        let Some(id) = entry.id else {
            partial = true;
            leaf = None;
            continue;
        };
        if entries.len() >= MAX_ENTRIES {
            return (Vec::new(), true);
        }
        let text = entry
            .message
            .filter(|m| entry.kind == "message" && matches!(m.role.as_str(), "user" | "assistant"))
            .map(|message| {
                let text = message.content.text();
                partial |= text.chars().count() > TEXT_CHARS;
                Text {
                    role: message.role,
                    timestamp: message.timestamp,
                    text: text.chars().take(TEXT_CHARS).collect(),
                }
            });
        leaf = Some(id.clone());
        entries.insert(
            id,
            Entry {
                parent: entry.parent,
                message: text,
            },
        );
    }
    let mut messages = Vec::new();
    let mut seen = HashSet::new();
    while let Some(id) = leaf {
        if cancel.load(Ordering::Relaxed) {
            return (Vec::new(), true);
        }
        if !seen.insert(id.clone()) {
            partial = true;
            break;
        }
        let Some(entry) = entries.remove(&id) else {
            partial = true;
            break;
        };
        if let Some(message) = entry.message {
            messages.push(message);
        }
        leaf = entry.parent;
    }
    messages.reverse();
    (messages, partial)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn saved_search_follows_latest_branch_and_ignores_private_blocks() {
        let records = [
            json!({"type":"message","id":"u","parentId":null,"message":{"role":"user","content":"visible prompt"}}),
            json!({"type":"message","id":"abandoned","parentId":"u","message":{"role":"assistant","content":"wrong branch"}}),
            json!({"type":"message","id":"a","parentId":"u","message":{"role":"assistant","content":[{"type":"text","text":"visible answer"},{"type":"thinking","thinking":"private reasoning"},{"type":"toolCall","arguments":{"token":"tool secret"}}]}}),
            json!({"type":"message","id":"t","parentId":"a","message":{"role":"toolResult","content":"tool output"}}),
            json!({"type":"compaction","id":"c","parentId":"t","summary":"private summary"}),
            json!({"type":"custom_message","id":"x","parentId":"c","content":"hidden instruction","display":false}),
        ];
        let bytes = records
            .iter()
            .map(Value::to_string)
            .collect::<Vec<_>>()
            .join("\n");
        let (messages, partial) = parse(bytes.as_bytes(), false, &AtomicBool::new(false));
        assert!(!partial);
        assert_eq!(
            messages.iter().map(|m| m.text.as_str()).collect::<Vec<_>>(),
            ["visible prompt", "visible answer"]
        );
    }
    #[test]
    fn malformed_final_record_never_selects_an_abandoned_leaf() {
        let records = b"{\"type\":\"message\",\"id\":\"old\",\"parentId\":null,\"message\":{\"role\":\"user\",\"content\":\"old branch\"}}\n{\"type\":\"message\",\"id\":\"broken\"";
        let (messages, partial) = parse(records, false, &AtomicBool::new(false));
        assert!(partial && messages.is_empty());
    }
    #[test]
    fn live_empty_branch_never_falls_back_to_saved_messages() {
        let candidate = Candidate {
            title: "Loaded empty chat".into(),
            project: PathBuf::new(),
            cwd: PathBuf::new(),
            path: Some("/must/not/read.jsonl".into()),
            target: sidebar_tree::Target::Agent("live".into()),
        };
        let found = search(
            vec![Source {
                candidate,
                live: Some(Vec::new()),
                limited: false,
            }],
            "branch",
            &Arc::new(AtomicBool::new(false)),
        );
        assert!(found.hits.is_empty());
        assert_eq!(found.unreadable, 0);
    }
    #[test]
    fn unicode_matching_snippets_and_cancellation_are_safe() {
        for text in ["שלום kanban 日本語", "İstanbul navigation", "e\u{301} café"] {
            let snippet = excerpt(text, "navigation");
            assert!(!snippet.is_empty());
        }
        assert!(matches("Keyboard shortcuts", "shortcuts keyboard"));
        assert!(!matches("Keyboard", "shortcuts keyboard"));
        assert!(parse(b"{}", false, &AtomicBool::new(true)).0.is_empty());
        let bytes = br#"{"type":"message","id":"u","parentId":"missing","message":{"role":"user","content":"tail"}}"#;
        assert!(parse(bytes, true, &AtomicBool::new(false)).1);
    }
}
