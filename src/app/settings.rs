use super::*;

#[derive(Default, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub(super) enum DeliveryMode {
    #[default]
    Steer,
    Queue,
}
impl DeliveryMode {
    pub fn opposite(self) -> Self {
        match self {
            Self::Steer => Self::Queue,
            Self::Queue => Self::Steer,
        }
    }
    pub fn label(self) -> &'static str {
        match self {
            Self::Steer => "Steer",
            Self::Queue => "Queue",
        }
    }
}
#[derive(Default, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub(super) enum PermissionMode {
    Manual,
    AcceptEdits,
    Plan,
    #[default]
    Auto,
    DontAsk,
    Bypass,
}
impl PermissionMode {
    pub const ALL: [Self; 6] = [
        Self::Auto,
        Self::Manual,
        Self::AcceptEdits,
        Self::Plan,
        Self::DontAsk,
        Self::Bypass,
    ];
    /// Permissiveness order used to bound delegated workers: a worker may run
    /// in its parent's mode or any lower one, never a higher one.
    pub fn rank(self) -> u8 {
        match self {
            Self::Plan => 0,
            Self::DontAsk => 1,
            Self::Manual => 2,
            Self::AcceptEdits => 3,
            Self::Auto => 4,
            Self::Bypass => 5,
        }
    }
    pub fn from_key(key: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|mode| mode.key() == key)
    }
    pub fn short_label(self) -> &'static str {
        if self == Self::Bypass {
            "Bypass"
        } else {
            self.label()
        }
    }
    pub fn key(self) -> &'static str {
        match self {
            Self::Manual => "manual",
            Self::AcceptEdits => "acceptEdits",
            Self::Plan => "plan",
            Self::Auto => "auto",
            Self::DontAsk => "dontAsk",
            Self::Bypass => "bypassPermissions",
        }
    }
    pub fn label(self) -> &'static str {
        match self {
            Self::Manual => "Manual",
            Self::AcceptEdits => "Accept edits",
            Self::Plan => "Plan",
            Self::Auto => "Auto",
            Self::DontAsk => "Don't ask",
            Self::Bypass => "Bypass all permissions",
        }
    }
    pub(super) fn description(self) -> &'static str {
        match self {
            Self::Manual => {
                "Reads are allowed. Approve each edit, command, network or other action."
            }
            Self::AcceptEdits => {
                "Accept workspace edits and basic local mkdir / mv / cp. Ask for scripts, other commands and network actions."
            }
            Self::Plan => {
                "Strict read-only tools and a small allowlist of inspection commands. Approve the written plan, then change mode to implement."
            }
            Self::Auto => {
                "Routine development actions proceed automatically. The cheapest available suitable model (or your override) checks for dangerous effects. Denied calls return to the agent to continue safe work; uncertain, unavailable, oversized, or repeated blocks can fall back to interactive approval. Classifier calls incur provider usage."
            }
            Self::DontAsk => {
                "Allow reads, safe inspection commands and workspace edits/moves. Automatically deny everything requiring approval; no permission dialogs."
            }
            Self::Bypass => {
                "Skip all Pui approval checks for writable agents, including shell and network actions. Use only in a trusted environment. Read-only worker restrictions remain in effect."
            }
        }
    }
}
#[derive(Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub(super) struct Settings {
    pub delivery: DeliveryMode,
    pub permission: PermissionMode,
    pub permission_classifier: Option<String>,
    pub compact_percent: u32,
    pub keep_recent_tokens: u32,
    #[serde(default = "default_auto_check_updates")]
    pub auto_check_updates: bool,
    #[serde(default = "default_auto_check_updates")]
    pub auto_download_updates: bool,
    pub auto_install_updates: bool,
    #[serde(default)]
    pub local_sandbox: bool,
    #[serde(default)]
    pub strict_sandbox: bool,
    #[serde(default)]
    pub auto_allow_rules: Vec<String>,
    #[serde(default)]
    pub auto_deny_rules: Vec<String>,
    #[serde(default = "default_auto_fallback_prompt")]
    pub auto_fallback_prompt: bool,
    /// Runtime-only: the models the picker shows (configured providers minus
    /// hidden models). Auto's classifier never selects anything outside it.
    #[serde(skip)]
    pub model_scope: Option<ModelScope>,
}
#[derive(Clone, PartialEq, Debug, Default)]
pub(super) struct ModelScope {
    pub providers: Vec<String>,
    pub hidden: Vec<String>,
}
fn default_auto_check_updates() -> bool {
    true
}
fn default_auto_fallback_prompt() -> bool {
    true
}
pub(super) fn normalized_auto_rules(rules: &[String]) -> Vec<String> {
    let mut normalized = Vec::new();
    for rule in rules {
        let cleaned: String = rule
            .chars()
            .filter(|ch| !ch.is_control())
            .collect::<String>()
            .trim()
            .chars()
            .take(240)
            .collect();
        if !cleaned.is_empty() {
            normalized.push(cleaned);
            if normalized.len() >= 32 {
                break;
            }
        }
    }
    normalized
}
impl Default for Settings {
    fn default() -> Self {
        Self {
            delivery: DeliveryMode::Steer,
            permission: PermissionMode::Auto,
            permission_classifier: None,
            compact_percent: 85,
            keep_recent_tokens: 20000,
            auto_check_updates: true,
            auto_download_updates: true,
            auto_install_updates: false,
            local_sandbox: false,
            strict_sandbox: false,
            auto_allow_rules: Vec::new(),
            auto_deny_rules: Vec::new(),
            auto_fallback_prompt: true,
            model_scope: None,
        }
    }
}
impl Settings {
    /// Disabling downloads revokes installation consent, not just its visibility.
    pub(super) fn normalize_update_preferences(&mut self) {
        if !self.auto_download_updates {
            self.auto_install_updates = false;
        }
    }

    /// Disabling local sandbox also disables strict sandbox mode.
    pub(super) fn normalize_sandbox_preferences(&mut self) {
        if !self.local_sandbox {
            self.strict_sandbox = false;
        }
    }

    /// Trim non-empty Auto rules, strip control characters, and bound list/entry lengths.
    pub(super) fn normalize_auto_rules(&mut self) {
        self.auto_allow_rules = normalized_auto_rules(&self.auto_allow_rules);
        self.auto_deny_rules = normalized_auto_rules(&self.auto_deny_rules);
    }

    pub(super) fn sandbox_mode(&self) -> crate::sandbox::Mode {
        crate::sandbox::Mode::from_flags(self.local_sandbox, self.strict_sandbox)
    }

    pub fn payload(&self) -> Value {
        let strict_sandbox = self.local_sandbox && self.strict_sandbox;
        let allow_rules = normalized_auto_rules(&self.auto_allow_rules);
        let deny_rules = normalized_auto_rules(&self.auto_deny_rules);
        let mut payload = json!({
            "permission": self.permission.key(),
            "permissionClassifier": self.permission_classifier,
            "compactPercent": self.compact_percent.clamp(50, 95),
            "keepRecentTokens": self.keep_recent_tokens.clamp(1000, 200000),
            "localSandbox": self.local_sandbox,
            "strictSandbox": strict_sandbox,
            "autoAllowRules": allow_rules,
            "autoDenyRules": deny_rules,
            "autoFallbackPrompt": self.auto_fallback_prompt,
        });
        if let Some(scope) = &self.model_scope {
            payload["classifierProviders"] = json!(scope.providers);
            payload["hiddenModels"] = json!(scope.hidden);
        }
        payload
    }
    pub fn hash_payload(&self, state: &mut impl std::hash::Hasher) {
        use std::hash::Hash;
        self.permission.key().hash(state);
        self.permission_classifier.hash(state);
        self.compact_percent.clamp(50, 95).hash(state);
        self.keep_recent_tokens.clamp(1000, 200000).hash(state);
        self.local_sandbox.hash(state);
        (self.local_sandbox && self.strict_sandbox).hash(state);
        normalized_auto_rules(&self.auto_allow_rules).hash(state);
        normalized_auto_rules(&self.auto_deny_rules).hash(state);
        self.auto_fallback_prompt.hash(state);
        if let Some(scope) = &self.model_scope {
            scope.providers.hash(state);
            scope.hidden.hash(state);
        }
    }
}
/// Pi's JSON config files are written atomically, preserving other keys.
pub(super) fn write_pi_json(path: &std::path::Path, value: &Value) -> Result<(), String> {
    providers::write_json_atomic(path, value, false)
}
mod appearance;
mod model_editor;
mod page;
mod providers;
pub(in crate::app) use providers::{AuthSummary, CustomProviderSummary};
mod resources;
mod scale;
pub(super) use page::{View, search_entries};
pub(super) use page::{
    card as page_card, row as page_row, section as page_section, separator as page_separator,
    toggle as toggle_switch,
};

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn updater_defaults_migrate_and_download_disable_revokes_install_consent() {
        for mut settings in [Settings::default(), serde_json::from_str("{}").unwrap()] {
            assert!(settings.auto_check_updates);
            assert!(settings.auto_download_updates);
            assert!(!settings.auto_install_updates);
            settings.auto_install_updates = true;
            settings.auto_download_updates = false;
            settings.normalize_update_preferences();
            assert!(!settings.auto_install_updates);
            settings.auto_download_updates = true;
            settings.normalize_update_preferences();
            assert!(!settings.auto_install_updates);
        }
        let mut settings: Settings = serde_json::from_value(
            json!({"auto_download_updates":false,"auto_install_updates":true}),
        )
        .unwrap();
        settings.normalize_update_preferences();
        let restored: Settings =
            serde_json::from_value(serde_json::to_value(settings).unwrap()).unwrap();
        assert!(!restored.auto_download_updates);
        assert!(!restored.auto_install_updates);
    }

    #[test]
    fn auto_classifier_override_roundtrips_and_reaches_runtime() {
        let mut settings: Settings = serde_json::from_str("{}").unwrap();
        assert!(settings.permission_classifier.is_none());
        assert!(settings.payload()["permissionClassifier"].is_null());
        settings.permission_classifier = Some("google/gemini-flash".into());
        let restored: Settings =
            serde_json::from_value(serde_json::to_value(&settings).unwrap()).unwrap();
        assert_eq!(
            restored.permission_classifier,
            settings.permission_classifier
        );
        assert_eq!(
            restored.payload()["permissionClassifier"],
            "google/gemini-flash"
        );
        assert_eq!(restored.payload()["permission"], "auto");
    }
    #[test]
    fn model_scope_reaches_runtime_but_is_never_persisted() {
        let mut settings = Settings::default();
        assert!(settings.payload().get("classifierProviders").is_none());
        settings.model_scope = Some(ModelScope {
            providers: vec!["anthropic".into(), "antigravity".into()],
            hidden: vec!["anthropic/old".into()],
        });
        let payload = settings.payload();
        assert_eq!(
            payload["classifierProviders"],
            json!(["anthropic", "antigravity"])
        );
        assert_eq!(payload["hiddenModels"], json!(["anthropic/old"]));
        let saved = serde_json::to_value(&settings).unwrap();
        assert!(saved.get("model_scope").is_none());
        let restored: Settings = serde_json::from_value(saved).unwrap();
        assert!(restored.model_scope.is_none());
    }
    #[test]
    fn defaults_and_shortcuts_are_complementary() {
        let settings: Settings = serde_json::from_str("{}").unwrap();
        assert_eq!(settings.delivery.label(), "Steer");
        assert_eq!(settings.delivery.opposite().label(), "Queue");
        assert_eq!(settings.permission.key(), "auto");
        assert_eq!(settings.payload()["compactPercent"], 85);
        assert_eq!(settings.delivery.opposite().opposite().label(), "Steer");
    }

    #[test]
    fn sandbox_settings_default_roundtrip_normalize_and_reach_payload() {
        let mut settings: Settings = serde_json::from_str("{}").unwrap();
        assert!(!settings.local_sandbox);
        assert!(!settings.strict_sandbox);
        assert_eq!(settings.sandbox_mode(), crate::sandbox::Mode::Off);
        assert_eq!(settings.payload()["localSandbox"], false);
        assert_eq!(settings.payload()["strictSandbox"], false);

        // Enabling local sandbox alone uses Standard mode.
        settings.local_sandbox = true;
        assert_eq!(settings.sandbox_mode(), crate::sandbox::Mode::Standard);
        assert_eq!(settings.payload()["localSandbox"], true);
        assert_eq!(settings.payload()["strictSandbox"], false);

        // Enabling strict sandbox alongside local sandbox uses Strict mode.
        settings.strict_sandbox = true;
        assert_eq!(settings.sandbox_mode(), crate::sandbox::Mode::Strict);
        let restored: Settings =
            serde_json::from_value(serde_json::to_value(&settings).unwrap()).unwrap();
        assert!(restored.local_sandbox);
        assert!(restored.strict_sandbox);
        assert_eq!(restored.payload()["localSandbox"], true);
        assert_eq!(restored.payload()["strictSandbox"], true);

        // Disabling local sandbox clears strict sandbox and turns payload flags off.
        settings.local_sandbox = false;
        assert_eq!(settings.sandbox_mode(), crate::sandbox::Mode::Off);
        assert_eq!(settings.payload()["localSandbox"], false);
        assert_eq!(settings.payload()["strictSandbox"], false);
        settings.normalize_sandbox_preferences();
        assert!(!settings.strict_sandbox);
    }

    #[test]
    fn auto_rules_normalize_roundtrip_serialize_and_hash() {
        let mut settings: Settings = serde_json::from_str("{}").unwrap();
        assert!(settings.auto_allow_rules.is_empty());
        assert!(settings.auto_deny_rules.is_empty());
        assert!(settings.auto_fallback_prompt);
        assert_eq!(settings.payload()["autoAllowRules"], json!([]));
        assert_eq!(settings.payload()["autoDenyRules"], json!([]));
        assert_eq!(settings.payload()["autoFallbackPrompt"], true);

        let long_rule = "a".repeat(300);
        settings.auto_allow_rules = (0..40)
            .map(|i| match i {
                0 => "  cargo test* \t ".into(),
                1 => "   ".into(),
                2 => "npm\x00 test*\x07".into(),
                3 => long_rule.clone(),
                _ => format!("rule-{i}"),
            })
            .collect();
        settings.auto_deny_rules = vec!["\n git push* \r".into(), "".into(), "docker*".into()];
        settings.auto_fallback_prompt = false;

        // Payload normalizes even before explicit normalize_auto_rules call.
        let payload = settings.payload();
        let payload_allow = payload["autoAllowRules"].as_array().unwrap();
        assert_eq!(payload_allow.len(), 32);
        assert_eq!(payload_allow[0], "cargo test*");
        assert_eq!(payload_allow[1], "npm test*");
        assert_eq!(payload_allow[2].as_str().unwrap().len(), 240);
        assert_eq!(payload["autoDenyRules"], json!(["git push*", "docker*"]));
        assert_eq!(payload["autoFallbackPrompt"], false);

        settings.normalize_auto_rules();
        assert_eq!(settings.auto_allow_rules.len(), 32);
        assert_eq!(settings.auto_allow_rules[0], "cargo test*");
        assert_eq!(settings.auto_allow_rules[1], "npm test*");
        assert_eq!(settings.auto_allow_rules[2].len(), 240);
        assert_eq!(settings.auto_deny_rules, vec!["git push*", "docker*"]);

        let restored: Settings =
            serde_json::from_value(serde_json::to_value(&settings).unwrap()).unwrap();
        assert_eq!(restored.auto_allow_rules, settings.auto_allow_rules);
        assert_eq!(restored.auto_deny_rules, settings.auto_deny_rules);
        assert!(!restored.auto_fallback_prompt);

        let hash_of = |s: &Settings| {
            let mut hasher = std::collections::hash_map::DefaultHasher::new();
            s.hash_payload(&mut hasher);
            std::hash::Hasher::finish(&hasher)
        };
        let h1 = hash_of(&settings);
        let mut modified = settings.clone();
        modified.auto_deny_rules.push("kubectl*".into());
        assert_ne!(h1, hash_of(&modified));
        let mut toggled = settings.clone();
        toggled.auto_fallback_prompt = true;
        assert_ne!(h1, hash_of(&toggled));
    }
}
