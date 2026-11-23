mod watch;
pub use watch::Watch;

use crate::protocol::{content_text, string};
use serde_json::Value;
use std::{
    fs,
    io::{BufRead, BufReader, Read, Seek, SeekFrom},
    path::{Path, PathBuf},
    time::SystemTime,
};

#[derive(Clone, Debug)]
pub struct Session {
    pub path: PathBuf,
    pub cwd: PathBuf,
    pub project: PathBuf,
    pub title: String,
    pub created: Option<SystemTime>,
    pub modified: SystemTime,
}

type OriginCache =
    std::sync::Mutex<std::collections::HashMap<PathBuf, crate::remote::SessionOrigin>>;

static ORIGIN_CACHE: std::sync::LazyLock<OriginCache> =
    std::sync::LazyLock::new(|| std::sync::Mutex::new(std::collections::HashMap::new()));

pub fn origin_for(path: &Path) -> Option<crate::remote::SessionOrigin> {
    ORIGIN_CACHE
        .lock()
        .unwrap_or_else(|p| p.into_inner())
        .get(path)
        .cloned()
}

pub fn remember_origin(path: &Path, origin: crate::remote::SessionOrigin) {
    ORIGIN_CACHE
        .lock()
        .unwrap_or_else(|p| p.into_inner())
        .insert(path.to_path_buf(), origin);
}

pub fn session_root() -> Option<PathBuf> {
    if let Some(dir) = crate::runtime::managed_agent_dir() {
        return Some(dir.join("sessions"));
    }
    if let Some(path) = std::env::var_os("PI_CODING_AGENT_SESSION_DIR") {
        return Some(path.into());
    }
    let agent = std::env::var_os("PI_CODING_AGENT_DIR")
        .map(PathBuf::from)
        .or_else(|| dirs::home_dir().map(|p| p.join(".pi/agent")))?;
    Some(agent.join("sessions"))
}

pub fn discover(retained: &[PathBuf]) -> Vec<Session> {
    let Some(root) = session_root() else {
        return Vec::new();
    };
    discover_in(&root, retained)
}

type DiscoveryCache =
    std::sync::Mutex<std::collections::HashMap<PathBuf, (SystemTime, u64, Session)>>;

static DISCOVERY_CACHE: std::sync::LazyLock<DiscoveryCache> =
    std::sync::LazyLock::new(|| std::sync::Mutex::new(std::collections::HashMap::new()));

fn discover_in(root: &Path, retained: &[PathBuf]) -> Vec<Session> {
    let mut files = Vec::new();
    collect(root, &mut files);
    if let Ok(dirs) = fs::read_dir(root) {
        for dir in dirs
            .flatten()
            .filter(|d| d.file_type().is_ok_and(|t| t.is_dir()))
        {
            collect(&dir.path(), &mut files);
        }
    }
    files.sort_by_key(|(_, time, _)| std::cmp::Reverse(*time));
    // Pinned/hidden chats must remain reachable even after leaving the recent 150.
    files.truncate(150);
    let mut seen: std::collections::HashSet<_> =
        files.iter().map(|(path, _, _)| path.clone()).collect();
    for path in retained {
        if seen.insert(path.clone()) {
            if let Ok(meta) = fs::metadata(path) {
                if meta.is_file() {
                    files.push((
                        path.clone(),
                        meta.modified().unwrap_or(SystemTime::UNIX_EPOCH),
                        meta.len(),
                    ));
                }
            }
        }
    }
    let mut projects = std::collections::HashMap::new();
    let mut cache = DISCOVERY_CACHE
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    if cache.len() > 512 {
        cache.retain(|path, _| seen.contains(path));
    }
    files
        .into_iter()
        .filter_map(|(path, modified, len)| {
            let mut session = match cache.get(&path) {
                Some((cached_modified, cached_len, cached))
                    if *cached_modified == modified && *cached_len == len =>
                {
                    cached.clone()
                }
                _ => {
                    let parsed = read_session(&path, modified)?;
                    cache.insert(path, (modified, len, parsed.clone()));
                    parsed
                }
            };
            session.project = projects
                .entry(session.cwd.clone())
                .or_insert_with(|| repository_folder(&session.cwd))
                .clone();
            Some(session)
        })
        .collect()
}

fn collect(root: &Path, files: &mut Vec<(PathBuf, SystemTime, u64)>) {
    if let Ok(entries) = fs::read_dir(root) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().is_some_and(|e| e == "jsonl") {
                if let Ok(meta) = entry.metadata() {
                    files.push((
                        path,
                        meta.modified().unwrap_or(SystemTime::UNIX_EPOCH),
                        meta.len(),
                    ));
                }
            }
        }
    }
}

fn read_session(path: &Path, modified: SystemTime) -> Option<Session> {
    let file = fs::File::open(path).ok()?;
    // Finish JSONL records instead of cutting off (and discarding) a large first
    // prompt. Stop after the usual prefix once a prompt is found; keep a hard
    // bound for image-heavy/malformed sessions. Discovery runs on a worker.
    let reader = BufReader::new(file.take(96 * 1024 * 1024));
    let mut prefix_bytes = 0;
    let mut reached_eof = true;
    let mut cwd = None;
    let mut created = None;
    let mut prompt = None;
    let mut name = None;
    let mut initial_title = None;
    let mut origin = None;
    for line in reader.lines().map_while(Result::ok) {
        prefix_bytes += line.len() + 1;
        let Ok(entry) = serde_json::from_str::<Value>(&line) else {
            continue;
        };
        if string(&entry, "type") == "session" {
            cwd = entry["cwd"].as_str().map(PathBuf::from);
            created = entry["timestamp"]
                .as_str()
                .and_then(|s| chrono::DateTime::parse_from_rfc3339(s).ok())
                .map(SystemTime::from);
        }
        if string(&entry, "type") == "message"
            && string(&entry["message"], "role") == "user"
            && prompt.is_none()
        {
            let content = &entry["message"]["content"];
            prompt = prompt_title(&content_text(content)).or_else(|| {
                content.as_array().and_then(|blocks| {
                    blocks
                        .iter()
                        .any(|b| b["type"] == "image")
                        .then(|| "Image attachment".into())
                })
            });
        }
        if initial_title.is_none() {
            initial_title = stored_initial_title(&entry);
        }
        if origin.is_none() {
            origin = crate::remote::SessionOrigin::from_jsonl_entry(&entry);
        }
        if string(&entry, "type") == "session_info" {
            name = entry["name"]
                .as_str()
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .map(str::to_owned)
                .or(name);
        }
        if prefix_bytes >= 96 * 1024 && prompt.is_some() {
            reached_eof = false;
            break;
        }
    }
    if !reached_eof {
        let mut tail = fs::File::open(path).ok()?;
        let len = tail.metadata().ok()?.len();
        tail.seek(SeekFrom::Start(len.saturating_sub(32 * 1024)))
            .ok()?;
        let mut bytes = Vec::new();
        tail.read_to_end(&mut bytes).ok()?;
        for line in bytes.split(|b| *b == b'\n') {
            if let Ok(entry) = serde_json::from_slice::<Value>(line) {
                if string(&entry, "type") == "session_info" {
                    name = entry["name"]
                        .as_str()
                        .map(str::trim)
                        .filter(|s| !s.is_empty())
                        .map(str::to_owned)
                        .or(name);
                }
            }
        }
    }
    if let Some(origin) = origin {
        remember_origin(path, origin);
    }
    Some(Session {
        path: path.to_path_buf(),
        cwd: cwd.clone()?,
        project: cwd?,
        // Explicit Pui renames live in preferences. Mutable harness names must
        // not replace the durable first-request marker.
        title: initial_title
            .or(name)
            .or(prompt)
            .unwrap_or_else(|| "New conversation".into()),
        created,
        modified,
    })
}

pub const AUTO_TITLE_CHARS: usize = 48;

fn stored_initial_title(entry: &Value) -> Option<String> {
    (entry["type"] == "custom"
        && entry["customType"] == "pui.initial-title.v1"
        && entry["data"]["version"] == 1)
        .then(|| entry["data"]["title"].as_str())
        .flatten()
        .filter(|title| {
            valid_initial_title(title)
                && entry["data"]["sessionId"].as_str().is_some_and(|id| {
                    !id.is_empty()
                        && id.len() <= 128
                        && id.is_ascii()
                        && !id.chars().any(char::is_control)
                })
                && entry["data"]["initialRequest"]
                    .as_str()
                    .is_some_and(|digest| {
                        digest.len() == 64
                            && digest
                                .bytes()
                                .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
                    })
        })
        .map(str::to_owned)
}

/// Unicode General_Category=Cf plus line/paragraph separators. Keep consistent
/// with the extension's property escape; no invisible/bidi control in auto names.
pub fn title_format_character(c: char) -> bool {
    matches!(c, '\u{ad}' | '\u{600}'..='\u{605}' | '\u{61c}' | '\u{6dd}' | '\u{70f}'
        | '\u{890}'..='\u{891}' | '\u{8e2}' | '\u{180e}' | '\u{200b}'..='\u{200f}'
        | '\u{2028}'..='\u{202e}' | '\u{2060}'..='\u{2064}' | '\u{2066}'..='\u{206f}'
        | '\u{feff}' | '\u{fff9}'..='\u{fffb}' | '\u{110bd}' | '\u{110cd}'
        | '\u{13430}'..='\u{1343f}' | '\u{1bca0}'..='\u{1bca3}' | '\u{1d173}'..='\u{1d17a}'
        | '\u{e0001}' | '\u{e0020}'..='\u{e007f}')
}

pub fn valid_initial_title(title: &str) -> bool {
    !crate::protocol::title_placeholder(title)
        && title.chars().count() <= AUTO_TITLE_CHARS
        && !title
            .chars()
            .any(|c| c.is_control() || title_format_character(c))
}

/// New automatic titles use only the initial request's first meaningful line,
/// never context bodies, tool output, summary prose or subsequent user turns.
/// Legacy prompt fallbacks below keep their old cap; explicit names are uncapped.
pub fn initial_title(text: &str) -> Option<String> {
    if text.chars().any(title_format_character) {
        return None;
    }
    let mut context = false;
    let line = text.lines().map(str::trim).find(|line| {
        if line.starts_with("[Context:") {
            context = true;
            return false;
        }
        if *line == "[/Context]" {
            context = false;
            return false;
        }
        !context && !line.is_empty()
    })?;
    if line.starts_with('<')
        || line.starts_with("```")
        || line.starts_with("[Attached files]")
        || line.starts_with("Parent agent ")
        || line.starts_with("Agent message from ")
        || line.starts_with("The conversation so far")
        || line.starts_with("This conversation is being continued")
    {
        return None;
    }
    // Keep linked subjects useful without spending the whole sidebar on URL
    // schemes/query strings. This is extractive, not a speculative summary.
    let mut paragraph = line.to_owned();
    // Soft-wrapped request lines belong together; context/code/blank boundaries
    // do not. The result remains a single line and is capped below.
    for following in text
        .lines()
        .skip_while(|candidate| candidate.trim() != line)
        .skip(1)
        .map(str::trim)
    {
        if following.is_empty()
            || following.starts_with("[Context:")
            || following.starts_with("```")
            || following.starts_with('<')
        {
            break;
        }
        paragraph.push(' ');
        paragraph.push_str(following);
        if paragraph.chars().count() > 1024 {
            break;
        }
    }
    let normalized = paragraph
        .split_whitespace()
        .map(|word| {
            url::Url::parse(word)
                .ok()
                .filter(|url| matches!(url.scheme(), "http" | "https"))
                .and_then(|url| {
                    if matches!(url.host_str(), Some("github.com" | "www.github.com")) {
                        let path: Vec<_> = url
                            .path_segments()?
                            .filter(|s| !s.is_empty())
                            .take(2)
                            .collect();
                        (!path.is_empty()).then(|| path.join("/"))
                    } else {
                        url.host_str()
                            .map(|host| host.trim_start_matches("www.").to_owned())
                    }
                })
                .unwrap_or_else(|| word.to_owned())
        })
        .collect::<Vec<_>>()
        .join(" ");
    let normalized = normalized.trim_start_matches(['#', ' ', '*']);
    if crate::protocol::title_placeholder(normalized)
        || matches!(
            normalized.to_lowercase().as_str(),
            "hello" | "hi" | "help" | "continue" | "ok" | "thanks"
        )
    {
        return None;
    }
    let mut title: String = normalized
        .chars()
        .filter(|c| !c.is_control())
        .take(AUTO_TITLE_CHARS)
        .collect();
    if normalized.chars().count() > AUTO_TITLE_CHARS {
        // Prefer a word boundary, but never let a long URL/CJK/token bypass cap.
        if let Some((byte, _)) = title.char_indices().rev().find(|(_, c)| c.is_whitespace())
            && title[..byte].chars().count() >= AUTO_TITLE_CHARS / 2
        {
            title.truncate(byte);
        }
    }
    title.truncate(title.trim_end().len());
    valid_initial_title(&title).then_some(title)
}

pub fn prompt_title(text: &str) -> Option<String> {
    // Bound the output without collecting every word of a potentially huge prompt.
    let title: String = text
        .split_whitespace()
        .flat_map(|word| word.chars().chain(std::iter::once(' ')))
        .take(100)
        .collect();
    let title = title.trim_end();
    (!title.is_empty()).then(|| title.to_owned())
}

/// Deletion accepts only ordinary Pi JSONL session files, never directories or symlinks.
pub fn validate_delete(path: &Path) -> Result<(), String> {
    if path.extension().is_none_or(|e| e != "jsonl") {
        return Err("Only Pi .jsonl session files can be deleted.".into());
    }
    match fs::symlink_metadata(path) {
        Ok(meta) if meta.is_file() && !meta.file_type().is_symlink() => {
            let file = fs::File::open(path).map_err(|e| e.to_string())?;
            let mut header = String::new();
            BufReader::new(file.take(64 * 1024))
                .read_line(&mut header)
                .map_err(|e| e.to_string())?;
            let header: Value =
                serde_json::from_str(&header).map_err(|_| "Not a Pi session file")?;
            if header["type"] != "session" {
                return Err("Not a Pi session file".into());
            }
            Ok(())
        }
        Ok(_) => Err("Refusing to delete a symlink or non-file session path.".into()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error.to_string()),
    }
}
pub fn delete(path: &Path) -> Result<(), String> {
    validate_delete(path)?;
    match fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(format!("Cannot delete session: {error}")),
    }
}

/// For abandoned new chats only, after the writer has exited and ownership is
/// locked. Never infer emptiness from a title or the currently selected branch.
pub fn delete_if_empty(path: &Path) -> Result<(), String> {
    validate_delete(path)?;
    let file = match fs::File::open(path) {
        Ok(file) => file,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(error) => return Err(error.to_string()),
    };
    const LIMIT: u64 = 64 * 1024;
    if file.metadata().map_err(|e| e.to_string())?.len() > LIMIT {
        return Err("New chat has saved data; it was kept.".into());
    }
    let mut bytes = Vec::new();
    file.take(LIMIT + 1)
        .read_to_end(&mut bytes)
        .map_err(|e| e.to_string())?;
    if bytes.len() as u64 > LIMIT {
        return Err("New chat has saved data; it was kept.".into());
    }
    for line in bytes.split(|b| *b == b'\n').filter(|line| !line.is_empty()) {
        let entry: Value = serde_json::from_slice(line)
            .map_err(|_| "New chat contains unrecognized data; it was kept.")?;
        if !matches!(
            string(&entry, "type"),
            "session" | "model_change" | "thinking_level_change"
        ) {
            return Err("New chat has saved data; it was kept.".into());
        }
    }
    delete(path)
}

pub fn export_markdown(path: &Path, destination: &Path) -> Result<(), String> {
    let file = fs::File::open(path).map_err(|e| format!("Cannot open session: {e}"))?;
    // Follow the active leaf rather than exporting abandoned branches as conversation.
    let mut entries = std::collections::HashMap::new();
    let mut leaf = None;
    for line in BufReader::new(file).lines().map_while(Result::ok) {
        if let Ok(entry) = serde_json::from_str::<Value>(&line) {
            if let Some(id) = entry["id"].as_str().filter(|_| entry["type"] != "session") {
                leaf = Some(id.to_owned());
                entries.insert(id.to_owned(), entry);
            }
        }
    }
    let mut chain = Vec::new();
    let mut seen = std::collections::HashSet::new();
    while let Some(id) = leaf {
        if !seen.insert(id.clone()) {
            break;
        }
        let Some(entry) = entries.get(&id) else {
            break;
        };
        chain.push(entry);
        leaf = entry["parentId"].as_str().map(str::to_owned);
    }
    chain.reverse();
    let mut output = String::from("# Pi session\n\n");
    for entry in chain {
        if entry["type"] == "message" {
            let message = &entry["message"];
            let role = string(message, "role");
            if matches!(role, "user" | "assistant") {
                output.push_str(&format!(
                    "## {}\n\n{}\n\n",
                    if role == "user" { "You" } else { "Pi" },
                    content_text(&message["content"])
                ));
                if !crate::media::attachments(&message["content"]).is_empty() {
                    output.push_str("[Image attachment omitted from Markdown export]\n\n");
                }
            }
        }
    }
    fs::write(destination, output).map_err(|e| format!("Cannot write export: {e}"))
}

pub fn age(time: SystemTime) -> String {
    let secs = time.elapsed().unwrap_or_default().as_secs();
    match secs {
        0..60 => "just now".into(),
        60..3600 => format!("{}m ago", secs / 60),
        3600..86400 => format!("{}h ago", secs / 3600),
        _ => format!("{}d ago", secs / 86400),
    }
}

fn repository_folder(cwd: &Path) -> PathBuf {
    // Group subfolders and worktrees with the main repository, without spawning Git per session.
    for ancestor in cwd.ancestors() {
        let git = ancestor.join(".git");
        if git.is_dir() {
            return ancestor.to_path_buf();
        }
        if git.is_file() {
            if let Ok(text) = fs::read_to_string(&git) {
                if let Some(path) = text.trim().strip_prefix("gitdir: ") {
                    let dir = ancestor.join(path);
                    if let Ok(common) = fs::read_to_string(dir.join("commondir")) {
                        let common = dir.join(common.trim());
                        if let Ok(common) = common.canonicalize() {
                            if let Some(root) = common.parent() {
                                return root.to_path_buf();
                            }
                        }
                    }
                }
            }
            return ancestor.to_path_buf();
        }
    }
    cwd.to_path_buf()
}

pub fn project_name(path: &Path) -> String {
    path.file_name()
        .unwrap_or(path.as_os_str())
        .to_string_lossy()
        .into_owned()
}

#[cfg(test)]
mod tests {
    use super::*;
    fn fixture(entries: &[Value]) -> Session {
        let path =
            std::env::temp_dir().join(format!("pui-session-title-{}.jsonl", uuid::Uuid::new_v4()));
        let text = std::iter::once(serde_json::json!({"type":"session", "cwd":"/fixture"}))
            .chain(entries.iter().cloned())
            .map(|entry| serde_json::to_string(&entry).unwrap())
            .collect::<Vec<_>>()
            .join("\n");
        fs::write(&path, text + "\n").unwrap();
        let session = read_session(&path, SystemTime::now()).unwrap();
        fs::remove_file(path).unwrap();
        session
    }
    #[test]
    fn abandoned_session_cleanup_requires_only_known_empty_metadata() {
        let path = std::env::temp_dir().join(format!("pui-empty-{}.jsonl", uuid::Uuid::new_v4()));
        let header = "{\"type\":\"session\",\"cwd\":\"/fixture\"}\n";
        for entry in [
            "{\"type\":\"message\",\"message\":{\"role\":\"user\",\"content\":\"keep\"}}",
            "{\"type\":\"session_info\",\"name\":\"Named empty chat\"}",
            "{\"type\":\"custom\",\"data\":\"keep\"}",
            "{\"type\":\"unknown\"}",
            "{malformed",
        ] {
            let original = format!("{header}{entry}\n");
            fs::write(&path, &original).unwrap();
            assert!(delete_if_empty(&path).is_err());
            assert_eq!(fs::read_to_string(&path).unwrap(), original);
        }
        fs::write(
            &path,
            format!(
                "{header}{{\"type\":\"model_change\"}}\n{{\"type\":\"thinking_level_change\"}}\n"
            ),
        )
        .unwrap();
        delete_if_empty(&path).unwrap();
        assert!(!path.exists());
        delete_if_empty(&path).unwrap(); // Never-persisted new chat.
    }
    #[test]
    fn pinned_and_hidden_paths_survive_the_recent_scan_limit() {
        let root = std::env::temp_dir().join(format!("pui-retained-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&root).unwrap();
        for i in 0..152 {
            fs::write(
                root.join(format!("{i}.jsonl")),
                format!(
                    "{}\n",
                    serde_json::json!({"type":"session", "cwd":"/fixture"})
                ),
            )
            .unwrap();
        }
        let recent = discover_in(&root, &[]);
        assert_eq!(recent.len(), 150);
        let excluded: Vec<_> = (0..152)
            .map(|i| root.join(format!("{i}.jsonl")))
            .filter(|p| !recent.iter().any(|s| &s.path == p))
            .collect();
        assert_eq!(excluded.len(), 2);
        let retained = discover_in(
            &root,
            &[
                excluded[0].clone(),
                excluded[1].clone(),
                excluded[0].clone(),
                root.join("missing.jsonl"),
            ],
        );
        assert_eq!(retained.len(), 152);
        assert!(
            excluded
                .iter()
                .all(|p| retained.iter().any(|s| &s.path == p))
        );
        for i in 0..152 {
            fs::remove_file(root.join(format!("{i}.jsonl"))).unwrap();
        }
        fs::remove_dir(root).unwrap();
    }
    #[test]
    fn creation_time_comes_from_session_header_not_last_activity() {
        let session = fixture(&[serde_json::json!({
            "type":"session", "cwd":"/fixture", "timestamp":"2024-01-02T03:04:05.000Z"
        })]);
        let expected = chrono::DateTime::parse_from_rfc3339("2024-01-02T03:04:05Z").unwrap();
        assert_eq!(session.created, Some(SystemTime::from(expected)));
        assert_ne!(session.created, Some(session.modified));
        assert_eq!(fixture(&[]).created, None);
        assert_eq!(
            fixture(&[
                serde_json::json!({"type":"session", "cwd":"/fixture", "timestamp":"invalid"})
            ])
            .created,
            None
        );
    }
    #[test]
    fn title_survives_first_prompt_crossing_old_prefix_limit() {
        let session = fixture(&[serde_json::json!({
            "type":"message", "message":{"role":"user", "content":[
                {"type":"text", "text":format!("Fix session titles\n{}", "context ".repeat(20000))},
                {"type":"image", "data":"a".repeat(200000)}
            ]}
        })]);
        assert!(session.title.starts_with("Fix session titles context"));
        assert_eq!(session.title.chars().count(), 100);
    }
    #[test]
    fn title_finds_prompt_after_large_metadata() {
        let session = fixture(&[
            serde_json::json!({"type":"custom", "data":"x".repeat(200000)}),
            serde_json::json!({"type":"message", "message":{"role":"user", "content":"  Actual\n prompt  "}}),
        ]);
        assert_eq!(session.title, "Actual prompt");
    }
    #[test]
    fn blank_prompts_and_names_do_not_mask_useful_titles() {
        let session = fixture(&[
            serde_json::json!({"type":"message", "message":{"role":"user", "content":"\n  "}}),
            serde_json::json!({"type":"message", "message":{"role":"user", "content":"Useful title"}}),
            serde_json::json!({"type":"session_info", "name":"  "}),
        ]);
        assert_eq!(session.title, "Useful title");
    }
    #[test]
    fn latest_explicit_name_wins_in_tail() {
        let session = fixture(&[
            serde_json::json!({"type":"session_info", "name":"Original name"}),
            serde_json::json!({"type":"message", "message":{"role":"user", "content":"Prompt title"}}),
            serde_json::json!({"type":"message", "message":{"role":"assistant", "content":"x".repeat(200000)}}),
            serde_json::json!({"type":"session_info", "name":" Renamed chat "}),
            serde_json::json!({"type":"session_info", "name":""}),
        ]);
        assert_eq!(session.title, "Renamed chat");
    }
    #[test]
    fn image_only_and_empty_sessions_have_honest_fallbacks() {
        let image = fixture(&[
            serde_json::json!({"type":"message", "message":{"role":"user", "content":[{"type":"image", "data":"a".repeat(200000)}]}}),
        ]);
        assert_eq!(image.title, "Image attachment");
        assert_eq!(fixture(&[]).title, "New conversation");
    }
    #[test]
    fn prompt_titles_are_normalized_bounded_and_unicode_safe() {
        assert_eq!(prompt_title(" \n \t "), None);
        assert_eq!(
            prompt_title(" Hello\n  world \t"),
            Some("Hello world".into())
        );
        assert_eq!(prompt_title(&"界".repeat(200)), Some("界".repeat(100)));
    }
    #[test]
    fn initial_request_titles_are_relevant_short_single_line_and_unicode_safe() {
        assert_eq!(
            initial_title("Fix Hebrew sidebar\n[Context: logs]\nwrong tool output\n[/Context]"),
            Some("Fix Hebrew sidebar".into())
        );
        assert_eq!(
            initial_title("[Context: logs]\nTool output title\n[/Context]\nRepair JSON exports"),
            Some("Repair JSON exports".into())
        );
        assert_eq!(
            initial_title(
                "https://www.onorca.dev/ https://github.com/stablyai/orca lets research this app lets take some ideas"
            ),
            Some("onorca.dev stablyai/orca lets research this app".into())
        );
        for text in [
            "continue",
            "New conversation",
            "```tool output",
            "<summary>compacted request",
            "This conversation is being continued from a summary",
            "[Context: log]\nonly tool output\n[/Context]",
        ] {
            assert_eq!(initial_title(text), None, "{text}");
        }
        for text in [
            "界".repeat(200),
            "🦀".repeat(200),
            "abcdefgh".repeat(200),
            "a\u{301}".repeat(200),
        ] {
            let title = initial_title(&text).unwrap();
            assert!(title.chars().count() <= AUTO_TITLE_CHARS);
            assert!(!title.chars().any(char::is_control));
            assert!(text.starts_with(&title));
        }
    }
    #[test]
    fn malformed_marker_never_overrides_preserved_legacy_title() {
        let valid = serde_json::json!({"version":1,"title":"Fix Hebrew sidebar","sessionId":"session-a","initialRequest":"a".repeat(64)});
        for (key, value) in [
            ("title", serde_json::json!("x".repeat(49))),
            ("title", serde_json::json!("Two\nlines")),
            ("title", serde_json::json!("Invisible\u{200b}name")),
            ("title", serde_json::json!("Bidi\u{202e}name")),
            ("title", serde_json::json!("Line\u{2028}separator")),
            ("title", serde_json::json!("New conversation")),
            ("initialRequest", serde_json::json!("not-a-digest")),
            ("sessionId", serde_json::json!("")),
        ] {
            let mut data = valid.clone();
            data[key] = value;
            let session = fixture(&[
                serde_json::json!({"type":"session_info","name":"Preserved existing title"}),
                serde_json::json!({"type":"custom","customType":"pui.initial-title.v1","data":data}),
                serde_json::json!({"type":"message","message":{"role":"user","content":"Original request"}}),
            ]);
            assert_eq!(session.title, "Preserved existing title");
        }
        for text in [
            "Fix \u{200b}sidebar",
            "Fix \u{202e}sidebar",
            "Fix\u{2028}sidebar",
        ] {
            assert_eq!(initial_title(text), None);
        }
    }
    #[test]
    fn durable_initial_title_survives_later_metadata_and_large_compacted_history() {
        let session = fixture(&[
            serde_json::json!({"type":"custom","customType":"pui.initial-title.v1","data":{"version":1,"title":"Fix Hebrew sidebar","sessionId":"first","initialRequest":"a".repeat(64)}}),
            serde_json::json!({"type":"message","message":{"role":"user","content":"Fix Hebrew sidebar\nextra context"}}),
            serde_json::json!({"type":"message","message":{"role":"assistant","content":"x".repeat(200000)}}),
            serde_json::json!({"type":"compaction","summary":"Unrelated summary"}),
            serde_json::json!({"type":"session_info","name":"Wrong generated name"}),
        ]);
        assert_eq!(session.title, "Fix Hebrew sidebar");
    }
    #[test]
    fn project_subfolders_resolve_to_repository_folder() {
        let root = std::env::temp_dir().join(format!("pui-repository-test-{}", std::process::id()));
        fs::create_dir_all(root.join(".git")).unwrap();
        fs::create_dir_all(root.join("src/nested")).unwrap();
        assert_eq!(repository_folder(&root.join("src/nested")), root);
    }
    #[test]
    fn future_timestamps_are_safe() {
        assert_eq!(
            age(SystemTime::now() + std::time::Duration::from_secs(60)),
            "just now"
        );
    }
    #[test]
    fn root_has_a_name() {
        assert!(!project_name(Path::new("/")).is_empty());
    }
}
