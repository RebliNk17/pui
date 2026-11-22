//! App-level model catalog and Pi defaults, independent of any chat.
//!
//! Pi owns provider, login and model configuration in `~/.pi/agent`; the Pi CLI
//! and every chat read the same files. Settings reads them directly (through
//! Pi's SDK for the model registry) instead of borrowing the active chat's
//! process, so Settings works whether chats are running, idle or closed.
use super::*;
use std::process::{Command, Stdio};
use std::time::Instant;

#[derive(Clone, Debug)]
pub(super) struct CatalogModel {
    pub model: protocol::Model,
    pub max_tokens: u64,
    pub text: bool,
    pub vision: bool,
    pub reasoning: bool,
    pub cache_read_cost: f64,
    pub cache_write_cost: f64,
    pub thinking_format: Option<String>,
    /// Both input and output prices are known (zero is a valid price).
    pub priced: bool,
    /// Learned from Pi for an extension-registered provider.
    pub extension: bool,
}
impl CatalogModel {
    pub fn key(&self) -> String {
        format!("{}/{}", self.model.provider, self.model.id)
    }
    /// Same eligibility rule as `scripts/permissions.ts` classifierModels.
    fn classifier_capable(&self) -> bool {
        self.text && self.model.context_window >= 8192 && self.max_tokens >= 256
    }
}

/// Pi's own defaults for new chats, from `~/.pi/agent/settings.json`.
#[derive(Clone, Debug, Default, PartialEq)]
pub(super) struct PiDefaults {
    pub provider: Option<String>,
    pub model: Option<String>,
    pub thinking: Option<String>,
    /// Pi's model scope (`enabledModels`); empty means every available model.
    pub enabled: Vec<String>,
}

#[derive(Default)]
pub(super) struct AppCatalog {
    pub models: Vec<CatalogModel>,
    pub loaded: bool,
    pub error: Option<String>,
    pub defaults: PiDefaults,
    rx: Option<mpsc::Receiver<Result<Vec<CatalogModel>, String>>>,
    pending: bool,
    pub started: bool,
}

impl AppCatalog {
    /// Reload in the background. A request during a load runs once afterwards.
    pub fn request(&mut self, ctx: &egui::Context) {
        self.started = true;
        self.defaults = read_defaults();
        if cfg!(test) {
            return;
        }
        if self.rx.is_some() {
            self.pending = true;
            return;
        }
        let (tx, rx) = mpsc::channel();
        self.rx = Some(rx);
        let ctx = ctx.clone();
        std::thread::spawn(move || {
            let _ = tx.send(fetch());
            ctx.request_repaint();
        });
    }

    /// True when a new catalog was loaded this call.
    pub fn poll(&mut self, ctx: &egui::Context) -> bool {
        let Some(rx) = &self.rx else { return false };
        let result = match rx.try_recv() {
            Ok(result) => result,
            Err(mpsc::TryRecvError::Empty) => return false,
            Err(mpsc::TryRecvError::Disconnected) => Err("Model catalog loader stopped.".into()),
        };
        self.rx = None;
        let loaded = match result {
            Ok(models) => {
                self.models = models;
                self.loaded = true;
                self.error = None;
                true
            }
            // Keep the last good catalog; report the failure.
            Err(error) => {
                self.error = Some(error);
                false
            }
        };
        if std::mem::take(&mut self.pending) {
            self.request(ctx);
        }
        loaded
    }

    pub fn loading(&self) -> bool {
        self.rx.is_some()
    }

    /// Pi's file-based providers come from the SDK; providers that Pui's own
    /// extension registers at runtime are only visible inside Pi, so keep the
    /// latest list Pi reported for them.
    pub fn learn_extension_models(&self, reported: &[protocol::Model], saved: &mut Vec<protocol::Model>) {
        if !self.loaded || reported.is_empty() {
            return;
        }
        let file_based: std::collections::BTreeSet<_> =
            self.models.iter().map(|m| m.model.provider.as_str()).collect();
        let learned: Vec<_> = reported
            .iter()
            .filter(|m| !file_based.contains(m.provider.as_str()))
            .cloned()
            .collect();
        if !learned.is_empty() {
            *saved = learned;
        }
    }

    /// SDK models plus saved extension-provider models (deduplicated).
    pub fn with_extensions(&mut self, saved: &[protocol::Model]) {
        let file_based: std::collections::BTreeSet<String> = self
            .models
            .iter()
            .filter(|m| !m.extension)
            .map(|m| m.model.provider.clone())
            .collect();
        self.models.retain(|m| !m.extension);
        self.models.extend(
            saved
                .iter()
                .filter(|m| !file_based.contains(&m.provider))
                .map(|model| CatalogModel {
                    model: model.clone(),
                    // Pi does not report these over RPC; extension providers are chat models.
                    max_tokens: 8192,
                    text: true,
                    vision: false,
                    reasoning: false,
                    cache_read_cost: 0.0,
                    cache_write_cost: 0.0,
                    thinking_format: None,
                    priced: true,
                    extension: true,
                }),
        );
    }

    /// Models the pickers show: configured providers, minus hidden models.
    pub fn visible<'a>(
        &'a self,
        configured: impl Fn(&str) -> bool + 'a,
        hidden: &'a std::collections::BTreeSet<String>,
    ) -> impl Iterator<Item = &'a CatalogModel> + 'a {
        self.models
            .iter()
            .filter(move |m| configured(&m.model.provider) && !hidden.contains(&m.key()))
    }

    /// Classifier candidates, in the same order Auto uses (cheapest first).
    pub fn classifier_models<'a>(
        &'a self,
        configured: impl Fn(&str) -> bool + 'a,
        hidden: &'a std::collections::BTreeSet<String>,
    ) -> Vec<&'a CatalogModel> {
        let mut models: Vec<_> = self
            .visible(configured, hidden)
            .filter(|m| m.classifier_capable() && in_scope(&self.defaults.enabled, &m.key()))
            .collect();
        let price = |m: &CatalogModel| {
            if m.priced {
                m.model.input_cost + m.model.output_cost
            } else {
                f64::INFINITY
            }
        };
        models.sort_by(|a, b| {
            price(a)
                .total_cmp(&price(b))
                .then_with(|| a.key().cmp(&b.key()))
        });
        models
    }
}

/// Pi `enabledModels` entries: exact `provider/id`, or `*` globs.
pub(super) fn in_scope(enabled: &[String], key: &str) -> bool {
    fn glob(pattern: &str, text: &str) -> bool {
        match pattern.split_once('*') {
            None => pattern == text,
            Some((head, rest)) => {
                let Some(tail) = text.strip_prefix(head) else {
                    return false;
                };
                tail.char_indices()
                    .map(|(i, _)| i)
                    .chain(std::iter::once(tail.len()))
                    .any(|i| glob(rest, &tail[i..]))
            }
        }
    }
    let key = key.to_lowercase();
    enabled.is_empty()
        || enabled
            .iter()
            .any(|pattern| glob(&pattern.to_lowercase(), &key))
}

fn settings_path() -> Option<PathBuf> {
    crate::runtime::agent_dir().map(|dir| dir.join("settings.json"))
}

pub(super) fn read_defaults() -> PiDefaults {
    let value = settings_path()
        .and_then(|path| std::fs::read_to_string(path).ok())
        .and_then(|text| serde_json::from_str::<Value>(&text).ok())
        .unwrap_or_default();
    parse_defaults(&value)
}

fn parse_defaults(value: &Value) -> PiDefaults {
    let text = |key: &str| {
        value[key]
            .as_str()
            .filter(|s| !s.is_empty())
            .map(str::to_owned)
    };
    PiDefaults {
        provider: text("defaultProvider"),
        model: text("defaultModel"),
        thinking: text("defaultThinkingLevel"),
        enabled: value["enabledModels"]
            .as_array()
            .map(|items| {
                items
                    .iter()
                    .filter_map(|v| v.as_str().map(str::to_owned))
                    .collect()
            })
            .unwrap_or_default(),
    }
}

/// Update Pi's defaults for new chats, preserving every other setting.
pub(super) fn write_defaults(
    provider_model: Option<(&str, &str)>,
    thinking: Option<&str>,
) -> Result<(), String> {
    let path = settings_path().ok_or("Pi's settings directory is unavailable.")?;
    let mut value = std::fs::read_to_string(&path)
        .ok()
        .and_then(|text| serde_json::from_str::<Value>(&text).ok())
        .filter(Value::is_object)
        .unwrap_or_else(|| json!({}));
    if let Some((provider, model)) = provider_model {
        value["defaultProvider"] = json!(provider);
        value["defaultModel"] = json!(model);
    }
    if let Some(level) = thinking {
        value["defaultThinkingLevel"] = json!(level);
    }
    settings::write_pi_json(&path, &value)
}

fn fetch() -> Result<Vec<CatalogModel>, String> {
    let sdk = crate::runtime::sdk()
        .filter(|p| p.is_file())
        .ok_or("Pi SDK not found.")?;
    let mut command = Command::new(crate::runtime::node_binary());
    command
        .args([
            "--input-type=module",
            "--eval",
            include_str!("../../scripts/catalog.mjs"),
        ])
        .arg(sdk)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null());
    crate::runtime::configure(&mut command);
    let mut child = command
        .spawn()
        .map_err(|_| "Could not start Node for the model catalog.")?;
    let start = Instant::now();
    loop {
        if child
            .try_wait()
            .map_err(|_| "Model catalog failed.")?
            .is_some()
        {
            break;
        }
        if start.elapsed() > Duration::from_secs(25) {
            let _ = child.kill();
            let _ = child.wait();
            return Err("Loading Pi's model catalog timed out.".into());
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    let output = child
        .wait_with_output()
        .map_err(|_| "Model catalog failed.")?;
    parse(&output.stdout)
}

fn parse(stdout: &[u8]) -> Result<Vec<CatalogModel>, String> {
    let record: Value =
        serde_json::from_slice(stdout).map_err(|_| "Model catalog returned invalid data.")?;
    if let Some(error) = record["error"].as_str() {
        return Err(error.into());
    }
    let models = record["models"]
        .as_array()
        .ok_or("Model catalog returned invalid data.")?;
    Ok(models
        .iter()
        .filter(|m| m["id"].as_str().is_some_and(|s| !s.is_empty()))
        .map(|m| CatalogModel {
            model: protocol::Model {
                id: protocol::string(m, "id").into(),
                name: protocol::string(m, "name").into(),
                provider: protocol::string(m, "provider").into(),
                context_window: m["contextWindow"].as_u64().unwrap_or_default(),
                input_cost: m["cost"]["input"].as_f64().unwrap_or_default(),
                output_cost: m["cost"]["output"].as_f64().unwrap_or_default(),
            },
            max_tokens: m["maxTokens"].as_u64().unwrap_or_default(),
            text: m["input"]
                .as_array()
                .is_some_and(|kinds| kinds.iter().any(|k| k == "text")),
            vision: m["input"]
                .as_array()
                .is_some_and(|kinds| kinds.iter().any(|k| k == "image")),
            reasoning: m["reasoning"].as_bool().unwrap_or(false),
            cache_read_cost: m["cost"]["cacheRead"].as_f64().unwrap_or_default(),
            cache_write_cost: m["cost"]["cacheWrite"].as_f64().unwrap_or_default(),
            thinking_format: m["thinkingFormat"]
                .as_str()
                .filter(|s| !s.is_empty())
                .map(str::to_owned),
            priced: m["cost"]["input"].is_number() && m["cost"]["output"].is_number(),
            extension: false,
        })
        .collect())
}

#[cfg(test)]
pub(super) fn fixture(models: &[(&str, &str, f64, f64)]) -> Vec<CatalogModel> {
    parse(
        json!({"models": models.iter().map(|(provider, id, input, output)| json!({
            "id": id, "name": id, "provider": provider, "contextWindow": 200000,
            "maxTokens": 8192, "input": ["text"], "cost": {"input": input, "output": output}
        })).collect::<Vec<_>>()})
        .to_string()
        .as_bytes(),
    )
    .unwrap()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classifier_candidates_match_auto_rules_and_picker_scope() {
        let mut catalog = AppCatalog {
            models: fixture(&[
                ("anthropic", "opus", 5.0, 25.0),
                ("google", "flash", 0.1, 0.4),
                ("Nvidia", "glm", 0.0, 0.0),
                ("anthropic", "haiku", 1.0, 5.0),
            ]),
            loaded: true,
            ..Default::default()
        };
        let mut unpriced = fixture(&[("anthropic", "mystery", 0.0, 0.0)]);
        unpriced[0].priced = false;
        catalog.models.extend(unpriced);
        let hidden = std::collections::BTreeSet::from(["anthropic/haiku".to_owned()]);
        let configured = |p: &str| p != "google"; // env-only key: not in the picker
        let keys: Vec<_> = catalog
            .classifier_models(configured, &hidden)
            .iter()
            .map(|m| m.key())
            .collect();
        assert_eq!(keys, ["Nvidia/glm", "anthropic/opus", "anthropic/mystery"]);
        catalog.defaults.enabled = vec!["anthropic/*".into()];
        let keys: Vec<_> = catalog
            .classifier_models(configured, &hidden)
            .iter()
            .map(|m| m.key())
            .collect();
        assert_eq!(keys, ["anthropic/opus", "anthropic/mystery"]);
    }

    #[test]
    fn extension_providers_are_learned_from_pi_and_kept_without_a_chat() {
        let mut catalog = AppCatalog {
            models: fixture(&[("anthropic", "opus", 5.0, 25.0), ("Nvidia", "glm", 0.0, 0.0)]),
            loaded: true,
            ..Default::default()
        };
        let reported: Vec<_> = fixture(&[
            ("anthropic", "opus", 5.0, 25.0),
            ("antigravity", "claude-opus-5-5", 0.0, 0.0),
            ("antigravity~2", "gemini-3-pro", 0.0, 0.0),
        ])
        .into_iter()
        .map(|m| m.model)
        .collect();
        let mut saved = Vec::new();
        catalog.learn_extension_models(&reported, &mut saved);
        assert_eq!(
            saved.iter().map(|m| m.provider.as_str()).collect::<Vec<_>>(),
            ["antigravity", "antigravity~2"]
        );
        catalog.with_extensions(&saved);
        catalog.with_extensions(&saved); // idempotent
        assert_eq!(catalog.models.len(), 4);
        // A chat without extension providers (e.g. a worker) never erases them.
        catalog.learn_extension_models(&reported[..1], &mut saved);
        assert_eq!(saved.len(), 2);
    }

    #[test]
    fn enabled_models_patterns_and_defaults_parse() {
        assert!(in_scope(&[], "any/model"));
        let scope = vec!["google/gemini-*".to_owned(), "anthropic/claude-opus-5-5".into()];
        assert!(in_scope(&scope, "google/gemini-2.5-pro"));
        assert!(in_scope(&scope, "anthropic/claude-opus-5-5"));
        assert!(!in_scope(&scope, "anthropic/claude-haiku-4-5"));
        let defaults = parse_defaults(&json!({
            "defaultProvider": "anthropic", "defaultModel": "claude-opus-5-5",
            "defaultThinkingLevel": "medium", "enabledModels": ["a/*"]
        }));
        assert_eq!(defaults.model.as_deref(), Some("claude-opus-5-5"));
        assert_eq!(defaults.thinking.as_deref(), Some("medium"));
        assert_eq!(defaults.enabled, ["a/*"]);
        assert!(parse(br#"{"error":"x"}"#).is_err());
    }
}
