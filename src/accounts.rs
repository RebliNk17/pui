//! Extra accounts per provider. Pi stores one credential per provider ID, so each extra account is
//! an alias ID (`anthropic~2`) with its own auth.json entry. This registry
//! (`pui-accounts.json` next to auth.json) is shared with `scripts/accounts.ts`, which registers
//! the alias providers in Pi, and `scripts/usage.mjs`. It never contains credentials.
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::{Duration, Instant, SystemTime};

pub const SEPARATOR: char = '~';
pub const FILE: &str = "pui-accounts.json";
const LABEL_LIMIT: usize = 80;

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct Account {
    pub id: String,
    pub base: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct Registry {
    /// Retry a request on the next account of the same provider when one hits its usage limit.
    #[serde(default = "enabled")]
    pub fallback: bool,
    #[serde(default)]
    pub accounts: Vec<Account>,
    /// Optional labels for base accounts (`anthropic` → "Personal").
    #[serde(default)]
    pub labels: BTreeMap<String, String>,
}

fn enabled() -> bool {
    true
}

impl Default for Registry {
    fn default() -> Self {
        Self {
            fallback: true,
            accounts: Vec::new(),
            labels: BTreeMap::new(),
        }
    }
}

/// Same rule as `scripts/accounts.ts`: lowercase ID segments, no path separators.
pub fn valid_segment(value: &str) -> bool {
    let mut chars = value.chars();
    chars
        .next()
        .is_some_and(|c| c.is_ascii_lowercase() || c.is_ascii_digit())
        && chars.all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || "._-".contains(c))
}

pub fn family(id: &str) -> &str {
    id.split(SEPARATOR).next().unwrap_or(id)
}

pub fn is_alias(id: &str) -> bool {
    id.contains(SEPARATOR)
}

fn clean_label(label: &str) -> Option<String> {
    let label = label.trim();
    (!label.is_empty()).then(|| label.chars().take(LABEL_LIMIT).collect())
}

pub fn path() -> Option<PathBuf> {
    crate::runtime::agent_dir().map(|dir| dir.join(FILE))
}

impl Registry {
    /// Drops malformed or duplicate entries instead of failing, like the extension does.
    fn sanitized(mut self) -> Self {
        let mut seen = std::collections::HashSet::new();
        self.accounts.retain(|account| {
            let mut parts = account.id.split(SEPARATOR);
            valid_segment(&account.base)
                && parts.next() == Some(account.base.as_str())
                && parts.next().is_some_and(valid_segment)
                && parts.next().is_none()
                && seen.insert(account.id.clone())
        });
        for account in &mut self.accounts {
            account.label = account.label.as_deref().and_then(clean_label);
        }
        self.labels = std::mem::take(&mut self.labels)
            .into_iter()
            .filter(|(id, _)| valid_segment(id))
            .filter_map(|(id, label)| clean_label(&label).map(|label| (id, label)))
            .collect();
        self
    }

    pub fn load_from(path: &Path) -> Self {
        std::fs::read(path)
            .ok()
            .and_then(|bytes| serde_json::from_slice::<Registry>(&bytes).ok())
            .unwrap_or_default()
            .sanitized()
    }

    pub fn load() -> Self {
        path().map(|p| Self::load_from(&p)).unwrap_or_default()
    }

    pub fn save_to(&self, path: &Path) -> Result<(), String> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)
                .map_err(|e| format!("Cannot create {}: {e}", parent.display()))?;
        }
        let text = serde_json::to_string_pretty(self).map_err(|e| e.to_string())?;
        let tmp = path.with_extension("json.tmp");
        std::fs::write(&tmp, format!("{text}\n"))
            .map_err(|e| format!("Cannot write {}: {e}", tmp.display()))?;
        std::fs::rename(&tmp, path).map_err(|e| format!("Cannot save {}: {e}", path.display()))?;
        invalidate_cache();
        Ok(())
    }

    pub fn save(&self) -> Result<(), String> {
        self.save_to(&path().ok_or("Cannot resolve the Pi agent directory.")?)
    }

    /// First free `base~N` (N ≥ 2) not used by the registry or an existing credential.
    pub fn next_id(&self, base: &str, taken: impl Fn(&str) -> bool) -> String {
        (2..)
            .map(|n| format!("{base}{SEPARATOR}{n}"))
            .find(|id| !taken(id) && !self.accounts.iter().any(|a| &a.id == id))
            .expect("unbounded range")
    }

    pub fn add(&mut self, id: &str, label: Option<&str>) -> Result<(), String> {
        let base = family(id).to_owned();
        if !is_alias(id) {
            if let Some(label) = label.and_then(clean_label) {
                self.labels.insert(base, label);
            }
            return Ok(());
        }
        let candidate = Account {
            id: id.to_owned(),
            base,
            label: label.and_then(clean_label),
        };
        self.accounts.retain(|a| a.id != id);
        self.accounts.push(candidate);
        let sanitized = self.clone().sanitized();
        if sanitized.accounts.len() != self.accounts.len() {
            self.accounts.pop();
            return Err(format!("'{id}' is not a valid account ID."));
        }
        Ok(())
    }

    pub fn rename(&mut self, id: &str, label: &str) {
        let label = clean_label(label);
        if let Some(account) = self.accounts.iter_mut().find(|a| a.id == id) {
            account.label = label;
        } else if let Some(label) = label {
            self.labels.insert(id.to_owned(), label);
        } else {
            self.labels.remove(id);
        }
    }

    pub fn remove(&mut self, id: &str) {
        self.accounts.retain(|a| a.id != id);
        self.labels.remove(id);
    }

    pub fn label(&self, id: &str) -> Option<&str> {
        self.accounts
            .iter()
            .find(|a| a.id == id)
            .and_then(|a| a.label.as_deref())
            .or_else(|| self.labels.get(id).map(String::as_str))
    }

    /// Account IDs of a family, base first, in registry order.
    pub fn family_ids(&self, base: &str) -> Vec<String> {
        std::iter::once(base.to_owned())
            .chain(
                self.accounts
                    .iter()
                    .filter(|a| a.base == base)
                    .map(|a| a.id.clone()),
            )
            .collect()
    }

    /// Provider label for pickers: `anthropic · Work`, `anthropic · 2`, or the plain ID.
    pub fn display(&self, id: &str) -> String {
        match (self.label(id), id.split_once(SEPARATOR)) {
            (Some(label), _) => format!("{} · {label}", family(id)),
            (None, Some((base, suffix))) => format!("{base} · {suffix}"),
            (None, None) => id.to_owned(),
        }
    }
}

static CACHE: Mutex<Option<(Instant, Option<SystemTime>, Registry)>> = Mutex::new(None);

fn invalidate_cache() {
    if let Ok(mut cache) = CACHE.lock() {
        *cache = None;
    }
}

/// Runs `f` on the registry for per-frame display; re-read at most every two seconds, and only
/// when the file changed.
fn with_cached<R>(f: impl FnOnce(&Registry) -> R) -> R {
    let Ok(mut cache) = CACHE.lock() else {
        return f(&Registry::default());
    };
    let fresh = cache
        .as_ref()
        .is_some_and(|(checked, _, _)| checked.elapsed() < Duration::from_secs(2));
    if !fresh {
        let path = path();
        let modified = path
            .as_ref()
            .and_then(|p| std::fs::metadata(p).ok())
            .and_then(|m| m.modified().ok());
        let registry = match cache.take() {
            Some((_, previous, registry)) if previous == modified => registry,
            _ => path.map(|p| Registry::load_from(&p)).unwrap_or_default(),
        };
        *cache = Some((Instant::now(), modified, registry));
    }
    match cache.as_ref() {
        Some((_, _, registry)) => f(registry),
        None => f(&Registry::default()),
    }
}

/// Display name for a provider ID in pickers and lists (`anthropic · Work`).
pub fn display_provider(id: &str) -> String {
    with_cached(|registry| {
        if is_alias(id) || registry.labels.contains_key(id) {
            registry.display(id)
        } else {
            id.to_owned()
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn registry_round_trips_and_matches_extension_validation() {
        let dir = std::env::temp_dir().join(format!("pui-accounts-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join(FILE);
        assert_eq!(Registry::load_from(&path), Registry::default());
        std::fs::write(&path, "{broken").unwrap();
        assert_eq!(Registry::load_from(&path), Registry::default());
        std::fs::write(
            &path,
            r#"{"fallback":false,"accounts":[{"id":"anthropic~work","base":"anthropic","label":" Work "},
               {"id":"openai~2","base":"anthropic"},{"id":"../x~1","base":"../x"},
               {"id":"anthropic~work","base":"anthropic"},{"id":"a~b~c","base":"a"}],
               "labels":{"anthropic":"Personal","Bad":"x"}}"#,
        )
        .unwrap();
        let registry = Registry::load_from(&path);
        assert!(!registry.fallback);
        assert_eq!(
            registry.accounts,
            [Account {
                id: "anthropic~work".into(),
                base: "anthropic".into(),
                label: Some("Work".into())
            }]
        );
        assert_eq!(registry.labels.len(), 1);
        registry.save_to(&path).unwrap();
        assert_eq!(Registry::load_from(&path), registry);
        // The JS side reads the same file shape.
        let saved: serde_json::Value =
            serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
        assert_eq!(saved["accounts"][0]["id"], "anthropic~work");
        assert_eq!(saved["fallback"], false);
        assert!(!std::fs::read_to_string(&path).unwrap().contains("refresh"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn accounts_are_added_renamed_and_removed_without_touching_others() {
        let mut registry = Registry::default();
        let taken = |id: &str| id == "anthropic~2";
        let id = registry.next_id("anthropic", taken);
        assert_eq!(
            id, "anthropic~3",
            "skips IDs that already hold a credential"
        );
        registry.add(&id, Some("work@x.com")).unwrap();
        registry.add("openai-codex~2", None).unwrap();
        registry.add("anthropic", Some("Personal")).unwrap();
        assert_eq!(registry.next_id("anthropic", taken), "anthropic~4");
        assert!(registry.add("Bad~1", None).is_err());
        assert_eq!(
            registry.family_ids("anthropic"),
            ["anthropic", "anthropic~3"]
        );
        assert_eq!(registry.display("anthropic~3"), "anthropic · work@x.com");
        assert_eq!(registry.display("openai-codex~2"), "openai-codex · 2");
        assert_eq!(registry.display("anthropic"), "anthropic · Personal");
        assert_eq!(registry.display("openai"), "openai");
        registry.rename("anthropic~3", "  Work  ");
        registry.rename("anthropic", "");
        assert_eq!(registry.label("anthropic~3"), Some("Work"));
        assert_eq!(registry.label("anthropic"), None);
        registry.remove("anthropic~3");
        assert_eq!(registry.family_ids("anthropic"), ["anthropic"]);
        assert_eq!(registry.accounts.len(), 1, "other families are kept");
        assert!(registry.fallback);
    }
}
