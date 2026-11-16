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

pub fn session_root() -> Option<PathBuf> {
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
    files.sort_by_key(|(_, time)| std::cmp::Reverse(*time));
    // Pinned/hidden chats must remain reachable even after leaving the recent 150.
    files.truncate(150);
    let mut seen: std::collections::HashSet<_> =
        files.iter().map(|(path, _)| path.clone()).collect();
    for path in retained {
        if seen.insert(path.clone()) {
            if let Ok(meta) = fs::metadata(path) {
                if meta.is_file() {
                    files.push((
                        path.clone(),
                        meta.modified().unwrap_or(SystemTime::UNIX_EPOCH),
                    ));
                }
            }
        }
    }
    let mut projects = std::collections::HashMap::new();
    files
        .into_iter()
        .filter_map(|(path, modified)| {
            let mut session = read_session(&path, modified)?;
            session.project = projects
                .entry(session.cwd.clone())
                .or_insert_with(|| repository_folder(&session.cwd))
                .clone();
            Some(session)
        })
        .collect()
}

fn collect(root: &Path, files: &mut Vec<(PathBuf, SystemTime)>) {
    if let Ok(entries) = fs::read_dir(root) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().is_some_and(|e| e == "jsonl") {
                if let Ok(meta) = entry.metadata() {
                    files.push((path, meta.modified().unwrap_or(SystemTime::UNIX_EPOCH)));
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
    let mut cwd = None;
    let mut created = None;
    let mut prompt = None;
    let mut name = None;
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
        if string(&entry, "type") == "session_info" {
            name = entry["name"]
                .as_str()
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .map(str::to_owned)
                .or(name);
        }
        if prefix_bytes >= 96 * 1024 && prompt.is_some() {
            break;
        }
    }
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
    Some(Session {
        path: path.to_path_buf(),
        cwd: cwd.clone()?,
        project: cwd?,
        title: name.or(prompt).unwrap_or_else(|| "New conversation".into()),
        created,
        modified,
    })
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
