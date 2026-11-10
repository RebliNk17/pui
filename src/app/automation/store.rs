use super::Job;
use std::{
    fs,
    io::Write,
    path::{Path, PathBuf},
};

pub(super) fn home() -> Option<PathBuf> {
    dirs::data_local_dir().map(|p| p.join("pui/automation-v1"))
}
// The elected scheduler window is also the only writer. Other windows show a
// read-only view and can take over after its OS lock is released.
pub(super) fn lock(home: &Path) -> Result<fs::File, String> {
    crate::tasks::store::lock(home, "b686b2ef-237a-4a17-b0b7-6c9189a3ae41")
}
pub(super) fn load(home: &Path) -> Result<Vec<Job>, String> {
    let path = home.join("jobs.json");
    if !path.exists() {
        return Ok(Vec::new());
    }
    let meta = path.symlink_metadata().map_err(|e| e.to_string())?;
    if meta.file_type().is_symlink() || !meta.is_file() || meta.len() > 4 * 1024 * 1024 {
        return Err("Invalid automation store.".into());
    }
    let jobs: Vec<Job> = serde_json::from_slice(&fs::read(path).map_err(|e| e.to_string())?)
        .map_err(|_| "Cannot decode automation store")?;
    if jobs.len() > 128 {
        return Err("Automation store exceeds 128 jobs.".into());
    }
    let mut ids = std::collections::BTreeSet::new();
    for job in &jobs {
        job.validate()?;
        if !ids.insert(&job.id) {
            return Err("Duplicate automation job ID.".into());
        }
    }
    Ok(jobs)
}
pub(super) fn save(home: &Path, jobs: &[Job]) -> Result<(), String> {
    let bytes = serde_json::to_vec(jobs).map_err(|e| e.to_string())?;
    if bytes.len() > 4 * 1024 * 1024 {
        return Err("Automation store exceeds 4 MiB.".into());
    }
    let temp = home.join("jobs.tmp");
    if temp
        .symlink_metadata()
        .is_ok_and(|m| m.file_type().is_symlink())
    {
        return Err("Invalid automation temporary file.".into());
    }
    let mut options = fs::OpenOptions::new();
    options.create(true).write(true).truncate(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut file = options.open(&temp).map_err(|e| e.to_string())?;
    file.write_all(&bytes)
        .and_then(|_| file.sync_all())
        .map_err(|e| e.to_string())?;
    fs::rename(temp, home.join("jobs.json")).map_err(|e| e.to_string())
}
