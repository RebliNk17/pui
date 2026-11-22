//! Display-only metadata. Pi acknowledgments remain the source of model selection.
use super::*;
use std::hash::{Hash, Hasher};

#[cfg(test)]
mod tests;

const LIMIT: usize = 24;
const MAX_AGE: u64 = 7 * 24 * 60 * 60;

#[derive(Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub(super) struct Catalog {
    cwd: PathBuf,
    identity: u64,
    saved: u64,
    models: Vec<protocol::Model>,
    label: String,
}

fn now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

fn identity(cwd: &std::path::Path) -> u64 {
    let mut hash = std::collections::hash_map::DefaultHasher::new();
    let binary = rpc::pi_binary();
    let mut paths = vec![
        binary,
        cwd.join(".pi/settings.json"),
        cwd.join(".pi/models.json"),
    ];
    let home = crate::runtime::agent_dir();
    if let Some(home) = home {
        paths.extend([
            home.join("settings.json"),
            home.join("models.json"),
            home.join("auth.json"),
        ]);
    }
    if let Some(sdk) = crate::usage::sdk() {
        paths.push(sdk);
    }
    // Only metadata, never credentials or config contents, enters the cache key.
    for path in paths {
        path.hash(&mut hash);
        let metadata = std::fs::metadata(path).ok();
        metadata.as_ref().map(|m| m.len()).hash(&mut hash);
        metadata.and_then(|m| m.modified().ok()).hash(&mut hash);
    }
    hash.finish()
}

impl Pui {
    pub(super) fn seed_model_catalog(&mut self) {
        let cwd = sync::identity(&self.prefs.cwd);
        let identity = identity(&cwd);
        self.state.model_cache_key = Some(identity);
        if let Some(cache) = self.prefs.model_catalogs.iter().find(|c| {
            c.cwd == cwd && c.identity == identity && now().saturating_sub(c.saved) < MAX_AGE
        }) {
            self.state.models = cache.models.clone();
            self.state.models_cached = true;
            if self.state.model.is_empty() {
                self.state.model_hint = cache.label.clone();
            }
        }
    }

    pub(super) fn remember_model_catalog(&mut self, command: &str) {
        // Workers can have explicit --model/extension policies. They must not
        // replace the next ordinary chat's recent-model hint or catalog.
        if self
            .agents
            .nodes
            .get(&self.agents.active)
            .is_some_and(|n| n.parent.is_some() || n.access != "workspace")
        {
            return;
        }
        let cwd = sync::identity(&self.prefs.cwd);
        let identity = identity(&cwd);
        if self.state.model_cache_key != Some(identity) {
            return;
        }
        let previous = self
            .prefs
            .model_catalogs
            .iter()
            .position(|c| c.cwd == cwd && c.identity == identity);
        let mut cache = previous
            .map(|i| self.prefs.model_catalogs.remove(i))
            .unwrap_or_default();
        cache.cwd = cwd;
        cache.identity = identity;
        if command == "get_available_models" {
            cache.saved = now();
            cache.models = self.state.models.clone();
        }
        if matches!(command, "get_state" | "set_model" | "set_thinking_level") {
            cache.label = if self.state.model.is_empty() {
                String::new()
            } else {
                model_picker::selected_label(&self.state)
            };
        }
        self.prefs.model_catalogs.insert(0, cache);
        self.prefs.model_catalogs.truncate(LIMIT);
    }
}
