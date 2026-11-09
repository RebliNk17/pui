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

pub fn discover() -> Vec<Session> {
    let Some(root) = session_root() else {
        return Vec::new();
    };
    let mut files = Vec::new();
    collect(&root, &mut files);
    if let Ok(dirs) = fs::read_dir(&root) {
        for dir in dirs
            .flatten()
            .filter(|d| d.file_type().is_ok_and(|t| t.is_dir()))
        {
            collect(&dir.path(), &mut files);
        }
    }
    files.sort_by_key(|(_, time)| std::cmp::Reverse(*time));
    let mut projects = std::collections::HashMap::new();
    files
        .into_iter()
        .take(150)
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
    // Read a bounded prefix for the first prompt, and a bounded suffix for a later name.
    let reader = BufReader::new(file.take(96 * 1024));
    let mut cwd = None;
    let mut title = None;
    for line in reader.lines().map_while(Result::ok) {
        let Ok(entry) = serde_json::from_str::<Value>(&line) else {
            continue;
        };
        if string(&entry, "type") == "session" {
            cwd = entry["cwd"].as_str().map(PathBuf::from);
        }
        if string(&entry, "type") == "message"
            && string(&entry["message"], "role") == "user"
            && title.is_none()
        {
            title = Some(
                content_text(&entry["message"]["content"])
                    .split_whitespace()
                    .collect::<Vec<_>>()
                    .join(" ")
                    .chars()
                    .take(100)
                    .collect(),
            );
        }
        if string(&entry, "type") == "session_info" {
            title = entry["name"].as_str().map(str::to_owned).or(title);
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
                title = entry["name"].as_str().map(str::to_owned).or(title);
            }
        }
    }
    Some(Session {
        path: path.to_path_buf(),
        cwd: cwd.clone()?,
        project: cwd?,
        title: title
            .filter(|s| !s.is_empty())
            .unwrap_or_else(|| "Untitled session".into()),
        modified,
    })
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
