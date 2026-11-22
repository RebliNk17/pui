//! Private, bounded per-task files. A single writer thread preserves transition order.
use super::Task;
use std::{
    fs,
    io::{Read, Write},
    path::{Path, PathBuf},
};
const MAX_RECORD: u64 = 4 * 1024 * 1024;
pub const LOCK_CONFLICT: &str =
    "This task is open in another Pui window. Close it there before changing or deleting it.";
pub fn home() -> Option<PathBuf> {
    dirs::data_local_dir().map(|p| p.join("pui/tasks-v1"))
}
fn private_dir(home: &Path) -> Result<(), String> {
    if home
        .symlink_metadata()
        .is_ok_and(|m| m.file_type().is_symlink())
    {
        return Err("Task store cannot be a symlink.".into());
    }
    fs::create_dir_all(home).map_err(|e| e.to_string())?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(home, fs::Permissions::from_mode(0o700)).map_err(|e| e.to_string())?;
    }
    Ok(())
}
pub fn lock(home: &Path, id: &str) -> Result<fs::File, String> {
    use fs2::FileExt;
    uuid::Uuid::parse_str(id).map_err(|_| "Invalid task ID")?;
    private_dir(home)?;
    let path = home.join(format!("{id}.lock"));
    if path
        .symlink_metadata()
        .is_ok_and(|m| m.file_type().is_symlink())
    {
        return Err("Task lock cannot be a symlink.".into());
    }
    let mut options = fs::OpenOptions::new();
    options.create(true).read(true).write(true).truncate(false);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let file = options.open(path).map_err(|e| e.to_string())?;
    file.try_lock_exclusive().map_err(|error| {
        if error.kind() == std::io::ErrorKind::WouldBlock {
            LOCK_CONFLICT.into()
        } else {
            format!("Cannot lock task storage: {error}")
        }
    })?;
    Ok(file)
}
pub fn save(home: &Path, task: &Task) -> Result<(), String> {
    task.validate_checkpoints()?;
    task.validate_operations()?;
    uuid::Uuid::parse_str(&task.id).map_err(|_| "Invalid task ID")?;
    private_dir(home)?;
    if home.join(format!("{}.deleted", task.id)).exists() {
        return Err("This task was deleted in another window.".into());
    }
    let bytes = serde_json::to_vec(task).map_err(|e| e.to_string())?;
    if bytes.len() as u64 > MAX_RECORD {
        return Err("Task record exceeded its storage bound.".into());
    }
    let temp = home.join(format!("{}.{}.tmp", task.id, uuid::Uuid::new_v4()));
    let mut options = fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut f = options.open(&temp).map_err(|e| e.to_string())?;
    f.write_all(&bytes)
        .and_then(|_| f.sync_all())
        .map_err(|e| e.to_string())?;
    fs::rename(temp, home.join(format!("{}.json", task.id))).map_err(|e| e.to_string())?;
    #[cfg(unix)]
    fs::File::open(home)
        .and_then(|dir| dir.sync_all())
        .map_err(|e| e.to_string())?;
    Ok(())
}
pub fn delete(home: &Path, id: &str, sessions: &[PathBuf]) -> Result<(), String> {
    uuid::Uuid::parse_str(id).map_err(|_| "Invalid task ID")?;
    private_dir(home)?;
    let record = home.join(format!("{id}.json"));
    if record
        .symlink_metadata()
        .is_ok_and(|m| !m.is_file() || m.file_type().is_symlink())
    {
        return Err("Invalid task record path; nothing was deleted.".into());
    }
    // Validate every transcript before removing anything. Keep the record on failure
    // so a partially failed deletion remains visible and retryable.
    for path in sessions {
        crate::sessions::validate_delete(path)?;
    }
    for path in sessions {
        crate::sessions::delete(path)?;
    }
    match fs::remove_file(record) {
        Ok(()) => {}
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => return Err(format!("Cannot delete task: {error}")),
    }
    // Contains no user data. Prevent a stale window/queued save resurrecting the task.
    let marker = home.join(format!("{id}.deleted"));
    fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&marker)
        .or_else(|e| {
            if e.kind() == std::io::ErrorKind::AlreadyExists {
                fs::File::open(marker)
            } else {
                Err(e)
            }
        })
        .map_err(|e| e.to_string())?;
    Ok(())
}

/// Read a bounded record without recovery so relationship-only edits preserve evidence.
fn read_record(home: &Path, id: &str) -> Result<Task, String> {
    uuid::Uuid::parse_str(id).map_err(|_| "Invalid task ID")?;
    if home.join(format!("{id}.deleted")).exists() {
        return Err("This workflow was deleted; nothing was changed.".into());
    }
    let path = home.join(format!("{id}.json"));
    let meta = fs::symlink_metadata(&path).map_err(|_| "Workflow record is unavailable.")?;
    if !meta.is_file() || meta.file_type().is_symlink() || meta.len() > MAX_RECORD {
        return Err("Invalid workflow record; nothing was changed.".into());
    }
    let mut options = fs::OpenOptions::new();
    options.read(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK);
    }
    let file = options
        .open(path)
        .map_err(|_| "Workflow record cannot be opened safely.")?;
    let opened = file
        .metadata()
        .map_err(|_| "Workflow record cannot be inspected.")?;
    if !opened.is_file() || opened.len() > MAX_RECORD {
        return Err("Invalid workflow record; nothing was changed.".into());
    }
    let mut bytes = Vec::new();
    file.take(MAX_RECORD + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| "Workflow record cannot be read.")?;
    if bytes.len() as u64 > MAX_RECORD {
        return Err("Workflow record exceeds its bound.".into());
    }
    let task: Task = serde_json::from_slice(&bytes)
        .map_err(|_| "Invalid workflow record; nothing was changed.")?;
    task.validate_checkpoints()?;
    task.validate_operations()?;
    if task.id != id || task.version != 1 {
        return Err("Invalid workflow record; nothing was changed.".into());
    }
    Ok(task)
}

/// Caller must hold the task's real writer lock. Compare all fields, not the
/// second-resolution timestamp: another window can write twice in one second.
pub fn current_record(home: &Path, expected: &Task) -> Result<Task, String> {
    let task = read_record(home, &expected.id)?;
    let expected = serde_json::to_value(expected).map_err(|e| e.to_string())?;
    // An owning live writer compares the raw durable record. Observers see the
    // recovered presentation after a crash; accept that exact projection too,
    // while the caller's real writer claim still governs whether it may act.
    let raw = serde_json::to_value(&task).map_err(|e| e.to_string())?;
    let mut visible = task.clone();
    visible.recover();
    if raw != expected && serde_json::to_value(&visible).map_err(|e| e.to_string())? != expected {
        return Err(
            "This workflow changed in another window. Refresh it before trying again.".into(),
        );
    }
    Ok(task)
}

/// Relationship-only transaction. Caller holds the writer lock through save.
pub fn attach(home: &Path, expected: &Task, parent: &Path) -> Result<Task, String> {
    let mut task = current_record(home, expected)?;
    if task.phase.active() {
        return Err("Stop the active workflow before attaching it to another conversation.".into());
    }
    task.parent_session = Some(parent.into());
    task.updated = super::now();
    save(home, &task)?;
    Ok(task)
}

#[derive(Default, Debug)]
pub struct History {
    pub records: Vec<Task>,
    /// Invalid records remain untouched on disk, excluded from executable state.
    pub quarantined: Vec<String>,
    pub omitted: usize,
}
impl History {
    pub fn warning(&self) -> Option<String> {
        if self.quarantined.is_empty() && self.omitted == 0 {
            return None;
        }
        Some(format!(
            "Workflow history: {} invalid record(s) quarantined in place; {} older valid record(s) not shown. Nothing was replayed or deleted.{}",
            self.quarantined.len(),
            self.omitted,
            self.quarantined
                .first()
                .map(|id| format!(
                    " First record: {id}. Inspect private workflow storage before repairing it."
                ))
                .unwrap_or_default()
        ))
    }
}
/// Read every bounded record before selecting the newest 1,000. A damaged record
/// is a per-record diagnostic, never permission to discard the rest of history.
pub fn history(home: &Path) -> Result<History, String> {
    private_dir(home)?;
    let mut history = History::default();
    let mut entries = fs::read_dir(home)
        .map_err(|e| e.to_string())?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| e.to_string())?;
    entries.sort_by_key(|e| e.file_name());
    let mut records = std::collections::BTreeMap::new();
    for e in entries {
        if e.path().extension().is_none_or(|ext| ext != "json") {
            continue;
        }
        let Some(id) = e
            .path()
            .file_stem()
            .and_then(|s| s.to_str())
            .map(str::to_owned)
        else {
            continue;
        };
        // Do not disclose untrusted filenames in diagnostics.
        if uuid::Uuid::parse_str(&id).is_err() {
            continue;
        }
        if home.join(format!("{id}.deleted")).exists() {
            continue;
        }
        match read_record(home, &id) {
            Ok(mut task) => {
                task.recover();
                records.insert((std::cmp::Reverse(task.updated), task.id.clone()), task);
                if records.len() > 1000 {
                    records.pop_last();
                    history.omitted += 1;
                }
            }
            Err(_) => history.quarantined.push(id),
        }
    }
    history.records = records.into_values().collect();
    Ok(history)
}
#[cfg(test)]
pub fn load(home: &Path) -> Result<Vec<Task>, String> {
    history(home).map(|h| h.records)
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn malformed_oversized_and_symlink_records_are_quarantined_without_hiding_valid_history() {
        let home =
            std::env::temp_dir().join(format!("pui-history-quarantine-{}", uuid::Uuid::new_v4()));
        let valid = Task::new("/project".into(), "p".into(), "m".into(), "off".into());
        save(&home, &valid).unwrap();
        let bad = uuid::Uuid::new_v4().to_string();
        let path = home.join(format!("{bad}.json"));
        fs::write(&path, b"not json").unwrap();
        let big = uuid::Uuid::new_v4().to_string();
        fs::OpenOptions::new()
            .create_new(true)
            .write(true)
            .open(home.join(format!("{big}.json")))
            .unwrap()
            .set_len(MAX_RECORD + 1)
            .unwrap();
        #[cfg(unix)]
        {
            let id = uuid::Uuid::new_v4().to_string();
            std::os::unix::fs::symlink(&path, home.join(format!("{id}.json"))).unwrap();
        }
        let result = history(&home).unwrap();
        assert_eq!(result.records.len(), 1);
        assert_eq!(result.records[0].id, valid.id);
        assert!(result.quarantined.contains(&bad) && result.quarantined.contains(&big));
        assert!(result.warning().unwrap().contains("quarantined in place"));
        assert_eq!(fs::read(&path).unwrap(), b"not json");
        assert!(current_record(&home, &valid).is_ok());
    }
    #[test]
    fn newest_selection_and_ties_do_not_depend_on_directory_enumeration() {
        let home =
            std::env::temp_dir().join(format!("pui-history-newest-{}", uuid::Uuid::new_v4()));
        let mut expected = Vec::new();
        for i in 0..1002 {
            let mut task = Task::new("/project".into(), "p".into(), "m".into(), "off".into());
            task.updated = i / 2;
            expected.push((std::cmp::Reverse(task.updated), task.id.clone()));
            save(&home, &task).unwrap();
        }
        expected.sort();
        expected.truncate(1000);
        let result = history(&home).unwrap();
        assert_eq!(result.omitted, 2);
        assert_eq!(
            result
                .records
                .iter()
                .map(|t| (std::cmp::Reverse(t.updated), t.id.clone()))
                .collect::<Vec<_>>(),
            expected
        );
    }
    #[test]
    fn deleting_task_preserves_worktree_and_other_records_and_rejects_stale_writes() {
        let home =
            std::env::temp_dir().join(format!("pui-task-delete-store-{}", uuid::Uuid::new_v4()));
        let mut task = Task::new(
            "/project".into(),
            "provider".into(),
            "model".into(),
            "high".into(),
        );
        let other = Task::new(
            "/project".into(),
            "provider".into(),
            "model".into(),
            "high".into(),
        );
        let worktree = home.join("worktree");
        fs::create_dir_all(&worktree).unwrap();
        fs::write(worktree.join("keep.txt"), "project contents").unwrap();
        task.workspace = Some(worktree.clone());
        task.isolated = true;
        save(&home, &task).unwrap();
        save(&home, &other).unwrap();
        let transcript = home.join("run.jsonl");
        fs::write(&transcript, "{\"type\":\"session\"}\n").unwrap();
        let guard = lock(&home, &task.id).unwrap();
        assert!(lock(&home, &task.id).is_err());
        delete(&home, &task.id, &[transcript.clone()]).unwrap();
        drop(guard);
        assert!(!transcript.exists());
        assert!(worktree.join("keep.txt").exists());
        assert_eq!(load(&home).unwrap()[0].id, other.id);
        assert!(save(&home, &task).is_err());
        assert!(delete(&home, "../../outside", &[]).is_err());
        delete(&home, &task.id, &[transcript]).unwrap();
    }
    #[test]
    fn invalid_transcript_keeps_task_record_for_retry() {
        let home =
            std::env::temp_dir().join(format!("pui-task-delete-invalid-{}", uuid::Uuid::new_v4()));
        let task = Task::new(
            "/project".into(),
            "provider".into(),
            "model".into(),
            "high".into(),
        );
        save(&home, &task).unwrap();
        let path = home.join("not-a-session.jsonl");
        fs::write(&path, "not a session").unwrap();
        assert!(delete(&home, &task.id, &[path.clone()]).is_err());
        assert!(path.exists());
        assert_eq!(load(&home).unwrap().len(), 1);
    }
    #[test]
    fn private_roundtrip_recovers_without_running_anything() {
        let home = std::env::temp_dir().join(format!("pui-task-store-{}", uuid::Uuid::new_v4()));
        let mut t = Task::new(
            "/project".into(),
            "provider".into(),
            "model".into(),
            "high".into(),
        );
        t.phase = super::super::Phase::Checking;
        save(&home, &t).unwrap();
        let saved = load(&home).unwrap();
        assert_eq!(saved[0].phase, super::super::Phase::Interrupted);
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(
                fs::metadata(home.join(format!("{}.json", t.id)))
                    .unwrap()
                    .permissions()
                    .mode()
                    & 0o777,
                0o600
            );
        }
    }
}
