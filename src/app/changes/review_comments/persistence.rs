//! Versioned local checkpoint. Restored anchors are always stale and never open files.
use super::*;
use std::io::Write;

const KEY: &str = "local-review-notes-v1";
const MAX_STORAGE: usize = 2 * 1024 * 1024; // bounded escaped JSON, not source text
const MAX_RECORD_STORAGE: usize = 64 * 1024;
const INVALID: &str =
    "Saved local notes were unavailable or invalid. Other preferences are unchanged.";
const PARTIAL: &str =
    "Some saved local notes were skipped because their metadata or size was invalid.";
const SAVE_LIMIT: &str = "Local notes could not be checkpointed within the storage limit. The previous checkpoint is unchanged.";

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Record {
    id: u64,
    anchor: Anchor,
    text: String,
    resolved: bool,
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Envelope {
    version: u8,
    next_id: u64,
    // Each independently bounded inner record can fail without losing its peers.
    #[serde(deserialize_with = "records")]
    records: Vec<String>,
}
fn records<'de, D: serde::Deserializer<'de>>(deserializer: D) -> Result<Vec<String>, D::Error> {
    struct Bounded;
    impl<'de> serde::de::Visitor<'de> for Bounded {
        type Value = Vec<String>;
        fn expecting(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            f.write_str("at most 64 local review records")
        }
        fn visit_seq<A: serde::de::SeqAccess<'de>>(
            self,
            mut seq: A,
        ) -> Result<Self::Value, A::Error> {
            let mut records = Vec::new();
            while let Some(record) = seq.next_element::<String>()? {
                if records.len() >= MAX_COMMENTS {
                    return Err(serde::de::Error::custom("too many local notes"));
                }
                // The outer serialized envelope is capped before parsing; record
                // strings are then bounded before parsing any nested metadata.
                if record.len() > MAX_RECORD_STORAGE {
                    records.push(String::new());
                } else {
                    records.push(record);
                }
            }
            Ok(records)
        }
    }
    deserializer.deserialize_seq(Bounded)
}
struct Limited(Vec<u8>);
impl Write for Limited {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        if self.0.len().saturating_add(bytes.len()) > MAX_STORAGE {
            return Err(std::io::Error::other("local note checkpoint limit"));
        }
        self.0.extend_from_slice(bytes);
        Ok(bytes.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}
fn path(path: &Path) -> bool {
    path.is_absolute()
        && path.to_str().is_some_and(|value| {
            !value.is_empty() && value.len() <= 4096 && !value.chars().any(char::is_control)
        })
}
fn durable(anchor: &Anchor) -> bool {
    path(&anchor.owner.root)
        && path(&anchor.root)
        && !anchor.owner.session.starts_with("agent:")
        && path(Path::new(&anchor.owner.session))
        && anchor.owner.session.len() <= 2048
}
fn valid(record: &Record) -> bool {
    let anchor = &record.anchor;
    let selection = &anchor.selection;
    let span = |(first, last): (usize, usize)| first > 0 && first <= last && last <= 1_000_000_000;
    record.id > 0
        && durable(anchor)
        && !anchor.path.is_empty()
        && anchor.path.len() <= 4096
        && !anchor.path.chars().any(char::is_control)
        && anchor.bytes() <= MAX_METADATA
        && !record.text.trim().is_empty()
        && record.text.len() <= MAX_TEXT
        && selection.chars.start < selection.chars.end
        && selection.chars.end <= MAX_DIFF_BYTES.saturating_mul(16)
        && span(selection.rows)
        && selection.rows.1 <= MAX_DIFF_BYTES
        && selection.hunk <= MAX_DIFF_BYTES
        && selection.old.is_none_or(span)
        && selection.new.is_none_or(span)
}
impl Record {
    fn from_comment(item: &Comment) -> Self {
        Self {
            id: item.id,
            anchor: item.anchor.clone(),
            text: item.text.clone(),
            resolved: item.resolved,
        }
    }
    fn into_comment(mut self) -> Comment {
        self.anchor.owner.root.shrink_to_fit();
        self.anchor.root.shrink_to_fit();
        self.anchor.owner.session.shrink_to_fit();
        self.anchor.path.shrink_to_fit();
        self.text.shrink_to_fit();
        Comment {
            id: self.id,
            anchor: self.anchor,
            text: self.text,
            resolved: self.resolved,
            stale: true,
        }
    }
}
impl Comments {
    /// Update preflight must acknowledge fresh output, not an old conditional save.
    pub(in crate::app) fn restart_checkpoint(&mut self) -> Result<Option<String>, String> {
        if self.editor.is_some() {
            return Err("Finish or cancel the local review note before restarting.".into());
        }
        let dirty = self.storage_dirty;
        self.storage_dirty = true;
        let mut storage = crate::app::restart::Recovery::default();
        self.save_checkpoint(&mut storage);
        self.storage_dirty = dirty;
        if self.storage_notice.is_some() {
            return Err("Local review notes could not be saved; restart postponed.".into());
        }
        Ok(eframe::Storage::get_string(&storage, KEY))
    }

    pub(in crate::app) fn update_restart_blocked(&self) -> bool {
        self.editor.is_some() || self.confirming()
    }

    /// Called only at normal app save, never per repaint. Pending editor is excluded.
    pub(in crate::app) fn save_checkpoint(&mut self, storage: &mut dyn eframe::Storage) {
        if !self.storage_dirty {
            return;
        }
        let mut records = Vec::new();
        for item in self
            .items
            .iter()
            .filter(|item| durable(&item.anchor))
            .take(MAX_COMMENTS)
        {
            let record = Record::from_comment(item);
            if !valid(&record) {
                self.storage_notice = Some(SAVE_LIMIT);
                return;
            }
            let Ok(encoded) = serde_json::to_string(&record) else {
                self.storage_notice = Some(SAVE_LIMIT);
                return;
            };
            if encoded.len() > MAX_RECORD_STORAGE {
                self.storage_notice = Some(SAVE_LIMIT);
                return;
            }
            records.push(encoded);
        }
        let mut writer = Limited(Vec::new());
        if serde_json::to_writer(
            &mut writer,
            &Envelope {
                version: 1,
                next_id: self.next_id,
                records,
            },
        )
        .is_err()
        {
            self.storage_notice = Some(SAVE_LIMIT);
            return;
        }
        let Ok(encoded) = String::from_utf8(writer.0) else {
            self.storage_notice = Some(SAVE_LIMIT);
            return;
        };
        storage.set_string(KEY, encoded);
        self.storage_dirty = false;
        self.storage_notice = None;
    }
    pub(in crate::app) fn restore_checkpoint(&mut self, storage: Option<&dyn eframe::Storage>) {
        // Never replace a live store or restore transient interaction state twice.
        if self.storage_loaded
            || !self.items.is_empty()
            || self.editor.is_some()
            || self.next_id != 0
        {
            return;
        }
        self.storage_loaded = true;
        let Some(raw) = storage.and_then(|storage| storage.get_string(KEY)) else {
            return;
        };
        if raw.len() > MAX_STORAGE {
            self.storage_notice = Some(INVALID);
            return;
        }
        let Ok(envelope) = serde_json::from_str::<Envelope>(&raw) else {
            self.storage_notice = Some(INVALID);
            return;
        };
        if envelope.version != 1 || envelope.records.len() > MAX_COMMENTS {
            self.storage_notice = Some(INVALID);
            return;
        }
        let mut ids = std::collections::BTreeSet::new();
        for encoded in envelope.records {
            if encoded.len() > MAX_RECORD_STORAGE {
                self.storage_notice = Some(PARTIAL);
                continue;
            }
            let Ok(record) = serde_json::from_str::<Record>(&encoded) else {
                self.storage_notice = Some(PARTIAL);
                continue;
            };
            if !valid(&record) || !ids.insert(record.id) {
                self.storage_notice = Some(PARTIAL);
                continue;
            }
            let item = record.into_comment();
            if self
                .bytes()
                .saturating_add(item.anchor.bytes())
                .saturating_add(item.text.capacity())
                > MAX_TOTAL
            {
                self.storage_notice = Some(PARTIAL);
                continue;
            }
            self.next_id = self.next_id.max(item.id);
            self.items.push(item);
        }
        self.next_id = self.next_id.max(envelope.next_id);
        self.storage_dirty = false;
    }
}

#[cfg(test)]
mod tests;
#[cfg(test)]
mod ui_tests;
