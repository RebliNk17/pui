//! Private, bounded per-task files. A single writer thread preserves transition order.
use super::Task;
use std::{
    fs,
    io::Write,
    path::{Path, PathBuf},
};
const MAX_RECORD: u64 = 4 * 1024 * 1024;
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
pub fn save(home: &Path, task: &Task) -> Result<(), String> {
    uuid::Uuid::parse_str(&task.id).map_err(|_| "Invalid task ID")?;
    private_dir(home)?;
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
pub fn load(home: &Path) -> Result<Vec<Task>, String> {
    if !home.exists() {
        return Ok(vec![]);
    }
    private_dir(home)?;
    let mut tasks = Vec::new();
    for entry in fs::read_dir(home).map_err(|e| e.to_string())?.take(1000) {
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
        if t.version != 1
            || uuid::Uuid::parse_str(&t.id).is_err()
            || e.file_name().to_string_lossy() != format!("{}.json", t.id)
        {
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
