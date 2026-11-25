//! Acknowledged pre-restart storage. eframe flush is asynchronous and has no
//! error result, so it cannot authorize unattended relaunches on its own.
use super::super::*;
use std::{
    collections::BTreeMap,
    fs::{File, OpenOptions},
    io::{Read, Write},
    path::Path,
};

const MAX_BYTES: u64 = 256 * 1024 * 1024;
const KEYS: [&str; 3] = ["preferences", "attention-v1", "local-review-notes-v1"];

#[derive(Default, Clone, Serialize, Deserialize)]
pub(in crate::app) struct Recovery {
    version: u8,
    id: String,
    base: Option<String>,
    #[serde(default)]
    ancestors: Vec<Option<String>>,
    values: BTreeMap<String, String>,
}
impl eframe::Storage for Recovery {
    fn get_string(&self, key: &str) -> Option<String> {
        self.values.get(key).cloned()
    }
    fn set_string(&mut self, key: &str, value: String) {
        self.values.insert(key.into(), value);
    }
    fn flush(&mut self) {}
}
impl Recovery {
    pub(in crate::app) fn location() -> Option<PathBuf> {
        eframe::storage_dir("Pui").map(|p| p.join("update-recovery-v1").join("snapshot.json"))
    }
    pub(in crate::app) fn seed(storage: &dyn eframe::Storage, base: Option<String>) -> Self {
        let values = KEYS
            .into_iter()
            .filter_map(|key| storage.get_string(key).map(|value| (key.into(), value)))
            .collect();
        Self {
            version: 1,
            id: uuid::Uuid::new_v4().to_string(),
            base,
            ancestors: Vec::new(),
            values,
        }
    }
    pub(in crate::app) fn id(&self) -> &str {
        &self.id
    }
    pub(in crate::app) fn copy_to(&self, storage: &mut dyn eframe::Storage) {
        for (key, value) in &self.values {
            storage.set_string(key, value.clone());
        }
    }
    pub(in crate::app) fn write(&self, path: &Path) -> Result<(), String> {
        self.write_with_directory_sync(path, |home| {
            #[cfg(unix)]
            File::open(home)
                .and_then(|dir| dir.sync_all())
                .map_err(|_| "Cannot synchronize composer recovery; update postponed.")?;
            Ok(())
        })
    }

    fn write_with_directory_sync(
        &self,
        path: &Path,
        sync: impl FnOnce(&Path) -> Result<(), String>,
    ) -> Result<(), String> {
        let bytes =
            serde_json::to_vec(self).map_err(|_| "Cannot encode update recovery snapshot.")?;
        if bytes.len() as u64 > MAX_BYTES {
            return Err(
                "Draft recovery exceeds 256 MB; update postponed without discarding anything."
                    .into(),
            );
        }
        let home = path.parent().ok_or("Invalid update recovery path.")?;
        std::fs::create_dir_all(home).map_err(|_| "Cannot create update recovery directory.")?;
        if std::fs::symlink_metadata(home)
            .map_err(|_| "Cannot inspect recovery directory.")?
            .file_type()
            .is_symlink()
        {
            return Err("Recovery directory cannot be a symlink.".into());
        }
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(home, std::fs::Permissions::from_mode(0o700))
                .map_err(|_| "Cannot secure recovery directory.")?;
        }
        let temp = home.join(format!("{}.tmp", uuid::Uuid::new_v4()));
        let result = (|| {
            let mut options = OpenOptions::new();
            options.write(true).create_new(true);
            #[cfg(unix)]
            {
                use std::os::unix::fs::OpenOptionsExt;
                options.mode(0o600).custom_flags(libc::O_NOFOLLOW);
            }
            let mut file = options
                .open(&temp)
                .map_err(|_| "Cannot open recovery snapshot.")?;
            file.write_all(&bytes)
                .and_then(|_| file.sync_all())
                .map_err(|_| "Cannot durably save composer recovery; update postponed.")?;
            std::fs::rename(&temp, path)
                .map_err(|_| "Cannot publish composer recovery; update postponed.")?;
            sync(home)
        })();
        if result.is_err() {
            let _ = std::fs::remove_file(temp);
        }
        result
    }
    pub(in crate::app) fn load(
        path: &Path,
        storage: Option<&dyn eframe::Storage>,
    ) -> Result<Option<Self>, String> {
        let file = match File::open(path) {
            Ok(file) => file,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(_) => {
                return Err(
                    "Update recovery is unreadable; the saved snapshot was retained.".into(),
                );
            }
        };
        if file
            .metadata()
            .map_err(|_| "Cannot inspect update recovery.")?
            .len()
            > MAX_BYTES
        {
            return Err(
                "Update recovery exceeds its safety bound; saved data was retained.".into(),
            );
        }
        let mut bytes = Vec::new();
        file.take(MAX_BYTES + 1)
            .read_to_end(&mut bytes)
            .map_err(|_| "Cannot read update recovery.")?;
        if bytes.len() as u64 > MAX_BYTES {
            return Err("Update recovery grew beyond its safety bound.".into());
        }
        let snapshot: Self = serde_json::from_slice(&bytes)
            .map_err(|_| "Invalid update recovery; saved data was retained.")?;
        let saved: Preferences = snapshot
            .values
            .get("preferences")
            .and_then(|raw| {
                let mut s = Self::default();
                s.values.insert("preferences".into(), raw.clone());
                eframe::get_value(&s, "preferences")
            })
            .ok_or("Update recovery has no valid preferences.")?;
        if snapshot.version != 1
            || uuid::Uuid::parse_str(&snapshot.id).is_err()
            || saved.update_recovery_id.as_deref() != Some(snapshot.id.as_str())
            || snapshot
                .values
                .keys()
                .any(|key| !KEYS.contains(&key.as_str()))
        {
            return Err("Invalid update recovery identity; saved data was retained.".into());
        }
        let on_disk: Option<Preferences> =
            storage.and_then(|s| eframe::get_value(s, "preferences"));
        let marker = on_disk
            .as_ref()
            .and_then(|p| p.update_recovery_id.as_deref());
        if marker == Some(snapshot.id.as_str()) {
            // Only disk-loaded storage can acknowledge the regular save. Never
            // delete based on frame storage's in-memory readback after flush.
            let _ = std::fs::remove_file(path);
            return Ok(None);
        }
        if on_disk.is_some()
            && marker != snapshot.base.as_deref()
            && !snapshot
                .ancestors
                .iter()
                .any(|ancestor| ancestor.as_deref() == marker)
        {
            return Err("Update recovery conflicts with newer preferences; snapshot retained, not replayed.".into());
        }
        Ok(Some(snapshot))
    }
}

impl Pui {
    pub(in crate::app) fn save_before_update(
        &mut self,
        storage: &mut dyn eframe::Storage,
        path: &Path,
    ) -> Result<(), String> {
        let prior = self.prefs.update_recovery_id.clone();
        // Replacing an unacknowledged journal must retain its disk ancestor,
        // not chain to a marker that may exist only in memory/failed saves.
        let (base, mut ancestors) =
            match Recovery::load(path, None)? {
                Some(previous) if Some(previous.id.as_str()) == prior.as_deref() => {
                    (previous.base, previous.ancestors)
                }
                Some(_) => return Err(
                    "Unacknowledged update recovery conflicts with this window; restart postponed."
                        .into(),
                ),
                None => (prior.clone(), Vec::new()),
            };
        if !ancestors.contains(&prior) {
            ancestors.push(prior.clone());
        }
        let mut recovery = Recovery::seed(storage, base);
        recovery.ancestors = ancestors;
        // Review notes have a conditional encoder; explicitly obtain fresh output
        // and propagate limit/editor errors instead of relying on a seeded value.
        if let Some(notes) = self.changes.review_comments.restart_checkpoint()? {
            eframe::Storage::set_string(&mut recovery, "local-review-notes-v1", notes);
        }
        self.prefs.update_recovery_id = Some(recovery.id().into());
        eframe::App::save(self, &mut recovery);
        // set_value logs serialization errors; verify every required value using
        // the same checked encoders before giving the file a fresh identity.
        let result = (|| {
            let expected = eframe::Storage::get_string(&recovery, "preferences")
                .ok_or("Preferences were not checkpointed.")?;
            let captured: Preferences = eframe::get_value(&recovery, "preferences")
                .ok_or("Preferences recovery could not be encoded.")?;
            if captured.update_recovery_id != self.prefs.update_recovery_id
                || expected.is_empty()
                || serde_json::to_value(&captured)
                    .map_err(|_| "Cannot validate saved preferences.")?
                    != serde_json::to_value(&self.prefs)
                        .map_err(|_| "Cannot validate current preferences.")?
            {
                return Err("Fresh preferences recovery was not captured.".into());
            }
            let attention: attention::Stored = eframe::get_value(&recovery, "attention-v1")
                .ok_or("Attention recovery was not captured.")?;
            if serde_json::to_value(&attention)
                .map_err(|_| "Cannot validate attention recovery.")?
                != serde_json::to_value(attention::Stored(self.attention.persisted()))
                    .map_err(|_| "Cannot validate current attention.")?
            {
                return Err("Fresh attention recovery was not captured.".into());
            }
            recovery.write(path)
        })();
        if result.is_err() {
            // Publication may precede a failed directory fsync. Retain that
            // complete snapshot rather than deleting both it and its predecessor.
            let published = File::open(path)
                .ok()
                .and_then(|file| {
                    serde_json::from_reader::<_, Recovery>(file.take(MAX_BYTES + 1)).ok()
                })
                .is_some_and(|saved| saved.id == recovery.id);
            if !published {
                self.prefs.update_recovery_id = prior;
            }
            recovery.copy_to(storage);
            // Match the retained published snapshot, or roll back only when no
            // new snapshot exists. Either way installation remains aborted.
            eframe::set_value(storage, "preferences", &self.prefs);
            return result;
        }
        recovery.copy_to(storage);
        storage.flush();
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn fixture() -> (PathBuf, Recovery) {
        let path = std::env::temp_dir()
            .join(format!("pui-update-recovery-test-{}", uuid::Uuid::new_v4()))
            .join("snapshot.json");
        let mut snapshot = Recovery::seed(&Recovery::default(), None);
        let prefs = Preferences {
            update_recovery_id: Some(snapshot.id.clone()),
            ..Default::default()
        };
        eframe::set_value(&mut snapshot, "preferences", &prefs);
        (path, snapshot)
    }
    #[test]
    fn failed_directory_sync_retains_replacement_journal_for_crash_recovery() {
        let (path, first) = fixture();
        first.write(&path).unwrap();
        let mut next = first.clone();
        next.id = uuid::Uuid::new_v4().to_string();
        next.ancestors.push(Some(first.id));
        let prefs = Preferences {
            update_recovery_id: Some(next.id.clone()),
            ..Default::default()
        };
        eframe::set_value(&mut next, "preferences", &prefs);
        assert!(
            next.write_with_directory_sync(
                &path,
                |_| Err("injected directory sync failure".into())
            )
            .is_err()
        );
        let restored = Recovery::load(&path, None).unwrap().unwrap();
        assert_eq!(restored.id, next.id);
    }

    #[test]
    fn durable_snapshot_overlays_stale_storage_and_acknowledges_only_disk_marker() {
        let (path, snapshot) = fixture();
        snapshot.write(&path).unwrap();
        assert!(
            Recovery::load(&path, Some(&Recovery::default()))
                .unwrap()
                .is_some()
        );
        let mut persisted = Recovery::default();
        snapshot.copy_to(&mut persisted);
        assert!(Recovery::load(&path, Some(&persisted)).unwrap().is_none());
        assert!(!path.exists());
    }
    #[test]
    fn checked_preflight_captures_composer_and_failed_save_keeps_marker() {
        let ctx = egui::Context::default();
        let mut app = Pui::new(
            &eframe::CreationContext::_new_kittest(ctx),
            Options {
                demo: true,
                ..Default::default()
            },
        );
        app.demo = false;
        app.state = State::default();
        app.new_chat = true;
        app.draft = "Do not lose this composer".into();
        app.draft_images.set_images(vec![
            crate::media::Attachment::from_json(
                &json!({"type":"image","data":"retained","mimeType":"image/png"}),
            )
            .unwrap(),
        ]);
        let (path, _) = fixture();
        let mut storage = Recovery::default();
        app.save_before_update(&mut storage, &path).unwrap();
        let snapshot = Recovery::load(&path, None).unwrap().unwrap();
        let prefs: Preferences = eframe::get_value(&snapshot, "preferences").unwrap();
        let mut restored = Pui::new(
            &eframe::CreationContext::_new_kittest(egui::Context::default()),
            Options {
                demo: true,
                ..Default::default()
            },
        );
        restored.demo = false;
        restored.prefs.cwd = app.prefs.cwd.clone();
        restored.restart_pending = prefs.restart_checkpoints;
        restored.draft.clear();
        restored.restore_blank_checkpoint();
        assert_eq!(restored.draft, app.draft);
        assert_eq!(restored.draft_images.images()[0].data.as_ref(), "retained");
        // Two preflights while the regular file remains at its original marker
        // must still recover the latest journal (including across a relaunch).
        let disk_before = Recovery::default();
        let disk_after_first = storage.clone();
        app.save_before_update(&mut storage, &path).unwrap();
        assert!(Recovery::load(&path, Some(&disk_before)).unwrap().is_some());
        assert!(
            Recovery::load(&path, Some(&disk_after_first))
                .unwrap()
                .is_some()
        );
        let prior = app.prefs.update_recovery_id.clone();
        assert!(
            app.save_before_update(&mut storage, &path.join("blocked.json"))
                .is_err()
        );
        assert_eq!(app.prefs.update_recovery_id, prior);
        assert_eq!(app.draft, "Do not lose this composer");
        assert!(!app.agents.exiting);
    }

    #[test]
    fn corrupt_conflicting_and_failed_writes_never_authorize_relaunch() {
        let (path, snapshot) = fixture();
        snapshot.write(&path).unwrap();
        let mut other = Recovery::default();
        eframe::set_value(
            &mut other,
            "preferences",
            &Preferences {
                update_recovery_id: Some("unrelated-newer-save".into()),
                ..Default::default()
            },
        );
        assert!(Recovery::load(&path, Some(&other)).is_err());
        assert!(path.exists());
        std::fs::write(&path, b"invalid").unwrap();
        assert!(Recovery::load(&path, None).is_err());
        let blocked = path.join("not-a-directory.json");
        assert!(snapshot.write(&blocked).is_err());
    }
}
