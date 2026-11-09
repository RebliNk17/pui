//! Fast, read-only tail preview while Pi initializes its tools and MCP connections.
//! This is presentation only; Pi RPC remains authoritative for model context.
use crate::protocol::{Message, parse_history};
use serde_json::Value;
use std::{
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
        if entry["type"] == "message" {
            chain.push(entry["message"].clone());
        }
        leaf = entry["parentId"].as_str().map(str::to_owned);
    }
    chain.reverse();
    let mut messages = parse_history(&chain);
    // Avoid starting a preview with a fragment of the preceding assistant turn.
    if partial {
        if let Some(user) = messages.iter().position(|m| m.role == "user") {
            messages.drain(..user);
        }
    }
    Ok(Preview { messages, partial })
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
    fn incomplete_tail_is_marked_partial() {
        let preview=parse(b"{\"type\":\"message\",\"id\":\"a\",\"parentId\":\"missing\",\"message\":{\"role\":\"user\",\"content\":\"tail\"}}",true).unwrap();
        assert!(preview.partial);
        assert_eq!(preview.messages.len(), 1);
    }
}
