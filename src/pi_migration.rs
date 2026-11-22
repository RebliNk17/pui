//! Move Pi's configuration and chat history into Pui's own folder.
//!
//! Non-destructive: configuration is copied, and the sessions folder is moved
//! with a symlink left at its old location, so every saved reference (Pi CLI,
//! older task/automation records) keeps resolving to the same files.
use serde_json::{Value, json};
use std::fs;
use std::path::{Path, PathBuf};

/// Not copied: Pi's self-managed binaries/installs (Pui bundles its own Pi).
const SKIP: &[&str] = &["sessions", "bin", "install"];

#[derive(Clone, Debug, PartialEq)]
pub struct Candidate {
    pub dir: PathBuf,
    pub logins: usize,
    pub providers: usize,
    pub sessions: usize,
    pub resources: usize,
}

impl Candidate {
    pub fn summary(&self) -> String {
        let plural = |n: usize, one: &str, many: &str| format!("{n} {}", if n == 1 { one } else { many });
        [
            plural(self.logins, "login", "logins"),
            plural(self.providers, "custom provider", "custom providers"),
            plural(self.sessions, "chat", "chats"),
            plural(self.resources, "skill or extension", "skills and extensions"),
        ]
        .join(" · ")
    }
}

fn read_object(path: &Path) -> Value {
    fs::read_to_string(path)
        .ok()
        .and_then(|text| serde_json::from_str(&text).ok())
        .filter(Value::is_object)
        .unwrap_or_else(|| json!({}))
}

fn count_sessions(dir: &Path) -> usize {
    let Ok(projects) = fs::read_dir(dir) else { return 0 };
    projects
        .flatten()
        .filter_map(|project| fs::read_dir(project.path()).ok())
        .flat_map(|files| files.flatten())
        .filter(|file| file.path().extension().is_some_and(|e| e == "jsonl"))
        .count()
}

/// Inspect one Pi folder; `None` when it holds nothing worth moving.
pub fn inspect(dir: &Path) -> Option<Candidate> {
    let has = |name: &str| dir.join(name).exists();
    if !dir.is_dir() || !["auth.json", "models.json", "settings.json", "sessions"].iter().any(|n| has(n)) {
        return None;
    }
    let count_entries = |name: &str| fs::read_dir(dir.join(name)).map(|e| e.count()).unwrap_or(0);
    Some(Candidate {
        dir: dir.to_path_buf(),
        logins: read_object(&dir.join("auth.json")).as_object().map_or(0, |o| o.len()),
        providers: read_object(&dir.join("models.json"))["providers"]
            .as_object()
            .map_or(0, |o| o.len()),
        sessions: count_sessions(&dir.join("sessions")),
        resources: ["skills", "extensions", "prompts", "themes"]
            .iter()
            .map(|name| count_entries(name))
            .sum(),
    })
}

/// Default places a Pi installation keeps its folder.
pub fn scan(target: &Path) -> Vec<Candidate> {
    let mut dirs: Vec<PathBuf> = Vec::new();
    if let Some(dir) = std::env::var_os("PI_CODING_AGENT_DIR") {
        dirs.push(dir.into());
    }
    if let Some(home) = dirs::home_dir() {
        dirs.push(home.join(".pi/agent"));
    }
    let target = fs::canonicalize(target).unwrap_or_else(|_| target.to_path_buf());
    let mut seen = std::collections::BTreeSet::new();
    dirs.into_iter()
        .filter(|dir| {
            let canonical = fs::canonicalize(dir).unwrap_or_else(|_| dir.clone());
            canonical != target && seen.insert(canonical)
        })
        .filter_map(|dir| inspect(&dir))
        .collect()
}

fn write_marker(target: &Path, source: Option<&Path>) -> Result<(), String> {
    let marker = json!({
        "version": 1,
        "source": source.map(|p| p.display().to_string()),
        "at": std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or_default(),
    });
    fs::write(target.join(crate::runtime::MANAGED_MARKER), marker.to_string())
        .map_err(|_| "Could not write Pui's Pi folder marker.".to_owned())
}

/// A fresh install: start Pui's own empty Pi folder.
pub fn create_empty(target: &Path) -> Result<(), String> {
    fs::create_dir_all(target.join("sessions"))
        .map_err(|_| "Could not create Pui's Pi folder.".to_owned())?;
    private(target);
    write_marker(target, None)
}

fn private(path: &Path) {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = fs::set_permissions(path, fs::Permissions::from_mode(0o700));
    }
}

fn copy_tree(source: &Path, target: &Path, skip_top: &[&str]) -> Result<(), String> {
    fs::create_dir_all(target).map_err(|e| format!("Could not create {}: {e}", target.display()))?;
    for entry in fs::read_dir(source).map_err(|e| format!("Could not read {}: {e}", source.display()))? {
        let entry = entry.map_err(|e| e.to_string())?;
        let name = entry.file_name();
        if skip_top.iter().any(|skip| name == *skip) {
            continue;
        }
        let from = entry.path();
        let to = target.join(&name);
        let kind = entry.file_type().map_err(|e| e.to_string())?;
        if kind.is_symlink() {
            if fs::symlink_metadata(&to).is_ok_and(|m| m.file_type().is_symlink()) {
                let _ = fs::remove_file(&to); // replace a link, never a real file
            }
            #[cfg(unix)]
            std::os::unix::fs::symlink(fs::read_link(&from).map_err(|e| e.to_string())?, &to)
                .map_err(|e| e.to_string())?;
        } else if kind.is_dir() {
            copy_tree(&from, &to, &[])?;
        } else {
            // fs::copy keeps permissions (auth.json stays private).
            fs::copy(&from, &to).map_err(|e| format!("Could not copy {}: {e}", from.display()))?;
        }
    }
    Ok(())
}

/// Copy Pi's settings again into an existing Pui folder (chats untouched).
pub fn import_settings(source: &Path, target: &Path) -> Result<(), String> {
    copy_tree(source, target, SKIP)
}

#[derive(Debug, PartialEq)]
pub struct Moved {
    pub old_sessions: PathBuf,
    pub new_sessions: PathBuf,
}

/// Copy configuration, move sessions (leaving a symlink), then mark the folder
/// as Pui's. Safe to retry: nothing is deleted and the marker is written last.
pub fn migrate(source: &Path, target: &Path) -> Result<Option<Moved>, String> {
    if target.join(crate::runtime::MANAGED_MARKER).is_file() {
        return Ok(None);
    }
    let parent = target.parent().ok_or("Invalid Pui folder.")?;
    fs::create_dir_all(parent).map_err(|_| "Could not create Pui's data folder.")?;
    if target.exists() {
        // An earlier attempt stopped before finishing; keep it aside, never delete.
        let aside = parent.join(format!("pi-agent.incomplete-{}", uuid::Uuid::new_v4()));
        fs::rename(target, &aside).map_err(|_| "Could not set aside an incomplete Pui Pi folder.")?;
        if let Ok(sessions) = fs::read_dir(aside.join("sessions")) {
            // Sessions already moved by that attempt stay in use.
            let _ = sessions;
            fs::create_dir_all(target).map_err(|e| e.to_string())?;
            fs::rename(aside.join("sessions"), target.join("sessions")).map_err(|e| e.to_string())?;
        }
    }
    let staging = parent.join(format!(".pi-agent.migrating-{}", uuid::Uuid::new_v4()));
    copy_tree(source, &staging, SKIP)?;
    if target.exists() {
        copy_tree(&staging, target, &[])?;
        let _ = fs::remove_dir_all(&staging); // our own temporary copy only
    } else {
        fs::rename(&staging, target).map_err(|_| "Could not finish copying Pi's settings.")?;
    }
    private(target);
    let old_sessions = source.join("sessions");
    let new_sessions = target.join("sessions");
    let moved = match fs::symlink_metadata(&old_sessions) {
        Ok(meta) if meta.is_dir() && !new_sessions.exists() => {
            if fs::rename(&old_sessions, &new_sessions).is_err() {
                // Different volume: copy, then keep the original aside.
                copy_tree(&old_sessions, &new_sessions, &[])?;
                let aside = source.join(format!("sessions.moved-to-pui-{}", uuid::Uuid::new_v4()));
                fs::rename(&old_sessions, aside).map_err(|e| e.to_string())?;
            }
            #[cfg(unix)]
            std::os::unix::fs::symlink(&new_sessions, &old_sessions)
                .map_err(|_| "Moved chats, but could not leave a link at the old location.")?;
            Some(Moved { old_sessions, new_sessions })
        }
        _ => {
            fs::create_dir_all(&new_sessions).map_err(|e| e.to_string())?;
            None
        }
    };
    write_marker(target, Some(source))?;
    Ok(moved)
}

/// Rewrite saved paths under `from` to `to` in strings and object keys.
pub fn rewrite_paths(value: &mut Value, from: &Path, to: &Path) -> bool {
    let (from, to) = (from.to_string_lossy().into_owned(), to.to_string_lossy().into_owned());
    let map = |text: &str| -> Option<String> {
        let rest = text.strip_prefix(&from)?;
        (rest.is_empty() || rest.starts_with('/')).then(|| format!("{to}{rest}"))
    };
    fn walk(value: &mut Value, map: &dyn Fn(&str) -> Option<String>) -> bool {
        match value {
            Value::String(text) => match map(text) {
                Some(new) => {
                    *text = new;
                    true
                }
                None => false,
            },
            Value::Array(items) => items.iter_mut().fold(false, |c, v| walk(v, map) | c),
            Value::Object(object) => {
                let mut changed = false;
                let entries: Vec<_> = std::mem::take(object).into_iter().collect();
                for (key, mut item) in entries {
                    changed |= walk(&mut item, map);
                    let key = map(&key).inspect(|_| changed = true).unwrap_or(key);
                    object.insert(key, item);
                }
                changed
            }
            _ => false,
        }
    }
    walk(value, &map)
}

/// Rewrite session paths in Pui's own JSON stores (tasks, automation).
pub fn rewrite_stores(moved: &Moved) {
    let Some(data) = dirs::data_local_dir().map(|dir| dir.join("pui")) else { return };
    let mut files = vec![data.join("automation-v1/jobs.json")];
    if let Ok(tasks) = fs::read_dir(data.join("tasks-v1")) {
        files.extend(
            tasks
                .flatten()
                .map(|e| e.path())
                .filter(|p| p.extension().is_some_and(|e| e == "json")),
        );
    }
    for file in files {
        let Ok(text) = fs::read_to_string(&file) else { continue };
        let Ok(mut value) = serde_json::from_str::<Value>(&text) else { continue };
        if rewrite_paths(&mut value, &moved.old_sessions, &moved.new_sessions) {
            let temp = file.with_extension(format!("json.{}", uuid::Uuid::new_v4()));
            if fs::write(&temp, value.to_string()).is_ok() && fs::rename(&temp, &file).is_err() {
                let _ = fs::remove_file(&temp);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("pui-migrate-{name}-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn migration_copies_config_moves_sessions_and_leaves_a_working_link() {
        let root = temp("root");
        let source = root.join("pi");
        fs::create_dir_all(source.join("sessions/--proj--")).unwrap();
        fs::create_dir_all(source.join("skills/review")).unwrap();
        fs::create_dir_all(source.join("bin")).unwrap();
        fs::write(source.join("auth.json"), r#"{"anthropic":{},"antigravity":{}}"#).unwrap();
        fs::write(source.join("models.json"), r#"{"providers":{"Nvidia":{}}}"#).unwrap();
        fs::write(source.join("settings.json"), r#"{"defaultModel":"x"}"#).unwrap();
        fs::write(source.join("sessions/--proj--/a.jsonl"), "{}\n").unwrap();
        fs::write(source.join("bin/pi"), "binary").unwrap();
        let candidate = inspect(&source).unwrap();
        assert_eq!((candidate.logins, candidate.providers, candidate.sessions, candidate.resources), (2, 1, 1, 1));
        let target = root.join("app/pi-agent");
        let moved = migrate(&source, &target).unwrap().unwrap();
        assert!(target.join(crate::runtime::MANAGED_MARKER).is_file());
        assert_eq!(fs::read_to_string(target.join("models.json")).unwrap(), r#"{"providers":{"Nvidia":{}}}"#);
        assert!(target.join("skills/review").is_dir());
        assert!(!target.join("bin").exists(), "Pi's own binaries are not copied");
        assert!(target.join("sessions/--proj--/a.jsonl").is_file(), "sessions moved");
        assert!(fs::symlink_metadata(source.join("sessions")).unwrap().file_type().is_symlink());
        assert_eq!(fs::read_to_string(source.join("sessions/--proj--/a.jsonl")).unwrap(), "{}\n", "old paths still resolve");
        assert!(source.join("auth.json").is_file(), "originals are kept");
        assert_eq!(moved.new_sessions, target.join("sessions"));
        assert_eq!(migrate(&source, &target).unwrap(), None, "idempotent");
        // A second Pi folder scan excludes Pui's own folder.
        assert!(inspect(&target).is_some());
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn saved_paths_are_rewritten_only_on_path_boundaries() {
        let mut value = json!({
            "last_session": "/h/.pi/agent/sessions/--p--/a.jsonl",
            "chats": {"/h/.pi/agent/sessions/--p--/a.jsonl": {"title": "x"}},
            "other": "/h/.pi/agent/sessions-backup/a.jsonl",
            "list": ["/h/.pi/agent/sessions"]
        });
        assert!(rewrite_paths(&mut value, Path::new("/h/.pi/agent/sessions"), Path::new("/p/pi-agent/sessions")));
        assert_eq!(value["last_session"], "/p/pi-agent/sessions/--p--/a.jsonl");
        assert!(value["chats"].get("/p/pi-agent/sessions/--p--/a.jsonl").is_some());
        assert_eq!(value["other"], "/h/.pi/agent/sessions-backup/a.jsonl");
        assert_eq!(value["list"][0], "/p/pi-agent/sessions");
    }

    #[test]
    fn fresh_install_creates_an_empty_managed_folder() {
        let root = temp("fresh");
        let target = root.join("pi-agent");
        create_empty(&target).unwrap();
        assert!(target.join("sessions").is_dir());
        assert!(target.join(crate::runtime::MANAGED_MARKER).is_file());
        fs::remove_dir_all(root).unwrap();
    }
}
