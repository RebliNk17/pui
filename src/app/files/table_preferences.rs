//! Metadata only: bounded per-workspace, per-file table presentation preferences.
//! No canonicalization, filesystem reads, tab loading, or access grants.
use super::*;

const LIMIT: usize = 256;
const PATH_LIMIT: usize = 4096;

#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
struct Entry {
    workspace: PathBuf,
    path: PathBuf,
}
#[derive(Default, Serialize)]
#[serde(transparent)]
pub(in crate::app) struct TableHeaders(Vec<Entry>);

fn valid(value: &Path) -> bool {
    value.is_absolute()
        && value
            .to_str()
            .is_some_and(|text| text.len() <= PATH_LIMIT && !text.chars().any(char::is_control))
        && !value
            .components()
            .any(|part| matches!(part, Component::ParentDir))
}
fn table(file: &Path) -> bool {
    file.extension()
        .and_then(|ext| ext.to_str())
        .is_some_and(|ext| ext.eq_ignore_ascii_case("csv") || ext.eq_ignore_ascii_case("tsv"))
}
fn entry(workspace: &Path, file: &Path) -> Option<Entry> {
    (valid(workspace) && valid(file) && table(file)).then(|| Entry {
        workspace: workspace.components().collect(),
        path: file.components().collect(),
    })
}
impl<'de> Deserialize<'de> for TableHeaders {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        struct Visitor;
        impl<'de> serde::de::Visitor<'de> for Visitor {
            type Value = TableHeaders;
            fn expecting(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
                f.write_str("a bounded table-header override list")
            }
            fn visit_seq<A: serde::de::SeqAccess<'de>>(
                self,
                mut seq: A,
            ) -> Result<Self::Value, A::Error> {
                let mut saved = TableHeaders::default();
                // Keep the newest bounded tail. At most LIMIT keys live in this
                // collection even if older preferences contain more records.
                while let Some(raw) = seq.next_element::<Entry>()? {
                    if let Some(key) = entry(&raw.workspace, &raw.path) {
                        saved.insert(key);
                    }
                }
                Ok(saved)
            }
        }
        deserializer.deserialize_seq(Visitor)
    }
}
impl TableHeaders {
    fn insert(&mut self, key: Entry) {
        self.0.retain(|old| old != &key);
        if self.0.len() == LIMIT {
            self.0.remove(0);
        }
        self.0.push(key);
    }
    pub fn supports(&self, workspace: &Path, file: &Path) -> bool {
        valid(workspace) && valid(file) && table(file)
    }
    pub fn header(&self, workspace: &Path, file: &Path) -> bool {
        // Path equality compares lexical components (including redundant dots)
        // without allocating another key or touching the filesystem each frame.
        !self.supports(workspace, file)
            || !self
                .0
                .iter()
                .any(|key| key.workspace == workspace && key.path == file)
    }
    pub fn set(&mut self, workspace: &Path, file: &Path, header: bool) -> bool {
        let Some(key) = entry(workspace, file) else {
            return false;
        };
        if header {
            self.0.retain(|old| old != &key);
        } else {
            self.insert(key);
        }
        true
    }
}

#[cfg(test)]
mod tests;
#[cfg(test)]
mod ui_tests;
