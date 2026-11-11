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
                "Routine development actions proceed automatically. The lowest-priced suitable model in the current provider checks for dangerous effects using your recent requests. Dangerous or uncertain actions ask for approval, not an automatic denial. Classifier calls incur provider usage."
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
    pub compact_percent: u32,
    pub keep_recent_tokens: u32,
}
impl Default for Settings {
    fn default() -> Self {
        Self {
            delivery: DeliveryMode::Steer,
            permission: PermissionMode::Auto,
            compact_percent: 85,
            keep_recent_tokens: 20000,
        }
    }
}
impl Settings {
    pub fn payload(&self) -> Value {
        json!({"permission":self.permission.key(),"compactPercent":self.compact_percent.clamp(50,95),"keepRecentTokens":self.keep_recent_tokens.clamp(1000,200000)})
    }
}
mod appearance;
mod page;
mod resources;
pub(super) use page::{View, search_entries};

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn defaults_and_shortcuts_are_complementary() {
        let settings: Settings = serde_json::from_str("{}").unwrap();
        assert_eq!(settings.delivery.label(), "Steer");
        assert_eq!(settings.delivery.opposite().label(), "Queue");
        assert_eq!(settings.permission.key(), "auto");
        assert_eq!(settings.payload()["compactPercent"], 85);
        assert_eq!(settings.delivery.opposite().opposite().label(), "Steer");
    }
}
