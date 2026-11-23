//! Durable session origin metadata (Desktop vs Android vs iOS vs Web vs CLI).
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum OriginPlatform {
    #[default]
    Desktop,
    Android,
    Ios,
    Web,
    Cli,
}

impl OriginPlatform {
    pub fn label(self) -> &'static str {
        match self {
            Self::Desktop => "Desktop",
            Self::Android => "Android",
            Self::Ios => "iOS",
            Self::Web => "Web",
            Self::Cli => "CLI",
        }
    }

    pub fn is_remote(self) -> bool {
        !matches!(self, Self::Desktop | Self::Cli)
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(default)]
pub struct SessionOrigin {
    pub platform: OriginPlatform,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub device: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub client: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub created_at: Option<i64>,
}

impl Default for SessionOrigin {
    fn default() -> Self {
        Self::desktop()
    }
}

impl SessionOrigin {
    pub fn desktop() -> Self {
        Self {
            platform: OriginPlatform::Desktop,
            device: None,
            client: Some("pui-desktop".into()),
            created_at: None,
        }
    }

    pub fn sanitized(mut self) -> Self {
        let clean = |value: Option<String>, max_len: usize| -> Option<String> {
            value
                .map(|s| {
                    s.chars()
                        .filter(|c| !c.is_control())
                        .take(max_len)
                        .collect::<String>()
                })
                .map(|s| s.trim().to_owned())
                .filter(|s| !s.is_empty())
        };
        self.device = clean(self.device, 80);
        self.client = clean(self.client, 64);
        self
    }

    pub fn badge_tooltip(&self) -> String {
        match &self.device {
            Some(device) if !device.is_empty() => {
                format!("Started from {} ({device})", self.platform.label())
            }
            _ => format!("Started from {}", self.platform.label()),
        }
    }

    pub fn custom_jsonl_entry(&self) -> Value {
        json!({
            "type": "custom",
            "customType": "pui.session-origin.v1",
            "data": {
                "version": 1,
                "platform": self.platform,
                "device": self.device,
                "client": self.client,
                "created_at": self.created_at,
            }
        })
    }

    pub fn from_jsonl_entry(entry: &Value) -> Option<Self> {
        if entry["type"] != "custom"
            || entry["customType"] != "pui.session-origin.v1"
            || entry["data"]["version"] != 1
        {
            return None;
        }
        let origin: Self = serde_json::from_value(entry["data"].clone()).ok()?;
        Some(origin.sanitized())
    }
}
