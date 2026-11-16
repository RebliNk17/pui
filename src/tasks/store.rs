//! Private, bounded per-task files. A single writer thread preserves transition order.
use super::Task;
use std::{
    fs,
    io::Write,
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
    let task: Task = serde_json::from_slice(&fs::read(path).map_err(|e| e.to_string())?)
        .map_err(|_| "Invalid workflow record; nothing was changed.")?;
    task.validate_checkpoints()?;
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

pub fn load(home: &Path) -> Result<Vec<Task>, String> {
    private_dir(home)?;
    let mut tasks = Vec::new();
    for entry in fs::read_dir(home)
        .map_err(|e| e.to_string())?
        .filter(|entry| {
            entry.as_ref().map_or(true, |e| {
                e.path().extension().is_some_and(|ext| ext == "json")
            })
        })
        .take(1000)
    {
        let e = entry.map_err(|e| e.to_string())?;
        if e.path().extension().is_none_or(|e| e != "json") {
            continue;
        }
        let meta = fs::symlink_metadata(e.path()).map_err(|e| e.to_string())?;
        if !meta.is_file() || meta.len() > MAX_RECORD {
            continue;
        }
        let mut t: Task = serde_json::from_slice(&fs::read(e.path()).map_err(|e| e.to_string())?)
            .map_err(|_| "Invalid task record; no work was resumed")?;
        t.validate_checkpoints()?;
        if t.version != 1
            || uuid::Uuid::parse_str(&t.id).is_err()
            || e.file_name().to_string_lossy() != format!("{}.json", t.id)
        {
            continue;
        }
        if home.join(format!("{}.deleted", t.id)).exists() {
            continue;
        }
        t.recover();
        tasks.push(t);
    }
    tasks.sort_by_key(|t| std::cmp::Reverse(t.updated));
    Ok(tasks)
}
#[cfg(test)]
mod tests {
    use super::*;
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
