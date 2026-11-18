//! Native provider management: Claude Pro/Max subscription OAuth, API keys,
//! and OpenAI-compatible local/custom endpoints with automatic `/v1/models` discovery.
use super::*;
use base64::Engine as _;
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::io::Read;
use std::path::{Path, PathBuf};

const ANTHROPIC_CLIENT_ID: &str = "9d1c250a-e61b-44d9-88ed-5944d1962f5e";
const ANTHROPIC_AUTHORIZE_URL: &str = "https://claude.ai/oauth/authorize";
const ANTHROPIC_TOKEN_URL: &str = "https://platform.claude.com/v1/oauth/token";
const ANTHROPIC_COPY_REDIRECT_URI: &str = "https://platform.claude.com/oauth/code/callback";
const ANTHROPIC_SCOPES: &str = "org:create_api_key user:profile user:inference user:sessions:claude_code user:mcp_servers user:file_upload";

const APIS: [(&str, &str); 3] = [
    (
        "openai-completions",
        "OpenAI Chat Completions (/v1/chat/completions)",
    ),
    ("openai-responses", "OpenAI Responses (/v1/responses)"),
    ("anthropic-messages", "Anthropic Messages (/v1/messages)"),
];

const BUILTIN_KEY_PROVIDERS: [(&str, &str); 6] = [
    ("anthropic", "Anthropic (Claude API Key)"),
    ("openai", "OpenAI"),
    ("google", "Google Gemini"),
    ("openrouter", "OpenRouter"),
    ("deepseek", "DeepSeek"),
    ("xai", "xAI (Grok)"),
];

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(in crate::app) struct DiscoveredModel {
    pub id: String,
    pub name: String,
    pub enabled: bool,
    pub reasoning: bool,
    pub vision: bool,
    pub context_window: u32,
    pub max_tokens: u32,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(in crate::app) struct CustomProviderSummary {
    pub id: String,
    pub name: String,
    pub base_url: String,
    pub api: String,
    pub api_key: String,
    pub models: Vec<DiscoveredModel>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(in crate::app) struct AuthSummary {
    pub provider: String,
    pub kind: String, // "oauth" or "api_key"
}

#[derive(Default)]
struct ClaudeSubscriptionFlow {
    open: bool,
    verifier: String,
    authorize_url: String,
    code_input: String,
    exchanging: bool,
    rx: Option<Receiver<Result<(), String>>>,
    error: Option<String>,
}

struct EndpointModal {
    open: bool,
    editing_id: Option<String>,
    id: String,
    name: String,
    base_url: String,
    api_index: usize,
    api_key: String,
    manual_model_id: String,
    models: Vec<DiscoveredModel>,
    discovering: bool,
    discover_rx: Option<Receiver<Result<Vec<DiscoveredModel>, String>>>,
    error: Option<String>,
    notice: Option<String>,
}

impl Default for EndpointModal {
    fn default() -> Self {
        Self {
            open: false,
            editing_id: None,
            id: "local-openai".into(),
            name: "Local OpenAI".into(),
            base_url: "http://localhost:11434/v1".into(),
            api_index: 0,
            api_key: String::new(),
            manual_model_id: String::new(),
            models: Vec::new(),
            discovering: false,
            discover_rx: None,
            error: None,
            notice: None,
        }
    }
}

#[derive(Default)]
struct ApiKeyModal {
    open: bool,
    provider_index: usize,
    custom_provider: String,
    api_key: String,
    error: Option<String>,
}

#[derive(Default)]
pub(in crate::app) struct ProvidersManager {
    loaded: bool,
    pub(in crate::app) custom_providers: Vec<CustomProviderSummary>,
    pub(in crate::app) saved_auth: Vec<AuthSummary>,
    claude: ClaudeSubscriptionFlow,
    endpoint: EndpointModal,
    api_key_modal: ApiKeyModal,
    pub(in crate::app) status_notice: Option<String>,
    pub(in crate::app) status_error: Option<String>,
    pub(in crate::app) reload_requested: bool,
}

fn agent_dir() -> Option<PathBuf> {
    crate::runtime::agent_dir()
}

fn models_json_path() -> Option<PathBuf> {
    agent_dir().map(|dir| dir.join("models.json"))
}

fn auth_json_path() -> Option<PathBuf> {
    agent_dir().map(|dir| dir.join("auth.json"))
}

fn read_json_object(path: &Path) -> Value {
    std::fs::read_to_string(path)
        .ok()
        .and_then(|s| serde_json::from_str::<Value>(&s).ok())
        .filter(|v| v.is_object())
        .unwrap_or_else(|| json!({}))
}

fn write_json_atomic(path: &Path, value: &Value, private_mode: bool) -> Result<(), String> {
    let parent = path
        .parent()
        .ok_or("Invalid configuration directory path.")?;
    std::fs::create_dir_all(parent).map_err(|_| "Cannot create ~/.pi/agent directory.")?;
    let temp = parent.join(format!(
        ".{}.tmp.{}",
        path.file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("config"),
        uuid::Uuid::new_v4()
    ));
    let mut text =
        serde_json::to_string_pretty(value).map_err(|_| "Cannot serialize configuration JSON.")?;
    text.push('\n');
    std::fs::write(&temp, text.as_bytes())
        .map_err(|_| "Cannot write temporary configuration file.")?;
    #[cfg(unix)]
    if private_mode {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(&temp, std::fs::Permissions::from_mode(0o600));
    }
    std::fs::rename(&temp, path).map_err(|_| {
        let _ = std::fs::remove_file(&temp);
        "Cannot save configuration file."
    })?;
    Ok(())
}

fn generate_pkce() -> (String, String) {
    let bytes = [
        *uuid::Uuid::new_v4().as_bytes(),
        *uuid::Uuid::new_v4().as_bytes(),
    ]
    .concat();
    let verifier = base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(bytes);
    let mut hasher = Sha256::new();
    hasher.update(verifier.as_bytes());
    let challenge = base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(hasher.finalize());
    (verifier, challenge)
}

fn build_claude_authorize_url(challenge: &str, state: &str) -> String {
    let mut url = url::Url::parse(ANTHROPIC_AUTHORIZE_URL).expect("valid authorize url");
    url.query_pairs_mut()
        .append_pair("code", "true")
        .append_pair("client_id", ANTHROPIC_CLIENT_ID)
        .append_pair("response_type", "code")
        .append_pair("redirect_uri", ANTHROPIC_COPY_REDIRECT_URI)
        .append_pair("scope", ANTHROPIC_SCOPES)
        .append_pair("code_challenge", challenge)
        .append_pair("code_challenge_method", "S256")
        .append_pair("state", state);
    url.to_string()
}

pub(in crate::app) fn parse_oauth_code_input(
    input: &str,
    expected_state: &str,
) -> Result<(String, String), String> {
    let trimmed = input.trim();
    if trimmed.is_empty() {
        return Err("Paste the authorization code shown by Claude after signing in.".into());
    }
    let (code, state) = if let Ok(parsed) = url::Url::parse(trimmed) {
        let mut c = None;
        let mut s = None;
        for (k, v) in parsed.query_pairs() {
            if k == "code" {
                c = Some(v.into_owned());
            } else if k == "state" {
                s = Some(v.into_owned());
            }
        }
        (c, s)
    } else if let Some((c, s)) = trimmed.split_once('#') {
        (Some(c.trim().to_owned()), Some(s.trim().to_owned()))
    } else {
        (Some(trimmed.to_owned()), None)
    };
    let code = code
        .filter(|c| !c.is_empty())
        .ok_or("Authorization code is missing.")?;
    if let Some(state) = &state {
        if !expected_state.is_empty() && state != expected_state {
            return Err(
                "OAuth state mismatch. Click Open Claude Sign-In again to generate a fresh code."
                    .into(),
            );
        }
    }
    let final_state = state.unwrap_or_else(|| expected_state.to_owned());
    Ok((code, final_state))
}

fn exchange_claude_oauth_code(code: &str, state: &str, verifier: &str) -> Result<(), String> {
    let payload = json!({
        "grant_type": "authorization_code",
        "client_id": ANTHROPIC_CLIENT_ID,
        "code": code,
        "state": state,
        "redirect_uri": ANTHROPIC_COPY_REDIRECT_URI,
        "code_verifier": verifier,
    });
    let script = r#"
const payload = JSON.parse(process.argv.at(-1));
const response = await fetch(process.argv.at(-2), {
  method: 'POST',
  headers: { 'Content-Type': 'application/json', 'Accept': 'application/json' },
  body: JSON.stringify(payload),
  signal: AbortSignal.timeout(20000)
});
const text = await response.text();
process.stdout.write(JSON.stringify({ ok: response.ok, status: response.status, body: text }));
"#;
    let mut command = std::process::Command::new(crate::runtime::node_binary());
    command
        .args(["--input-type=module", "--eval", script])
        .arg(ANTHROPIC_TOKEN_URL)
        .arg(payload.to_string())
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::null());
    crate::runtime::configure(&mut command);
    let output = command
        .output()
        .map_err(|_| "Cannot launch Node runtime to exchange OAuth code.")?;
    if !output.status.success() {
        return Err("OAuth token exchange process failed.".into());
    }
    let parsed: Value = serde_json::from_slice(&output.stdout)
        .map_err(|_| "Invalid response from OAuth token exchange.")?;
    if parsed["ok"] != true {
        let status = parsed["status"].as_u64().unwrap_or(0);
        return Err(format!(
            "Claude rejected the authorization code (HTTP {status}). Generate a new code and try again."
        ));
    }
    let token_data: Value = serde_json::from_str(parsed["body"].as_str().unwrap_or("{}"))
        .map_err(|_| "Claude token endpoint returned invalid JSON.")?;
    let access = token_data["access_token"]
        .as_str()
        .filter(|s| !s.is_empty())
        .ok_or("Token response is missing access_token.")?;
    let refresh = token_data["refresh_token"]
        .as_str()
        .filter(|s| !s.is_empty())
        .ok_or("Token response is missing refresh_token.")?;
    let expires_in = token_data["expires_in"].as_i64().unwrap_or(3600);
    let now_ms = chrono::Utc::now().timestamp_millis();
    let expires = now_ms + expires_in * 1000 - 5 * 60 * 1000;

    let auth_path = auth_json_path().ok_or("Cannot resolve ~/.pi/agent/auth.json path.")?;
    let mut auth = read_json_object(&auth_path);
    auth["anthropic"] = json!({
        "type": "oauth",
        "access": access,
        "refresh": refresh,
        "expires": expires,
    });
    write_json_atomic(&auth_path, &auth, true)
}

pub(in crate::app) fn parse_discovered_models(data: &Value) -> Vec<DiscoveredModel> {
    let list = data["data"]
        .as_array()
        .or_else(|| data["models"].as_array())
        .or_else(|| data.as_array());
    let Some(items) = list else {
        return Vec::new();
    };
    let mut models = Vec::new();
    let mut seen = std::collections::BTreeSet::new();
    for item in items {
        let id = item["id"]
            .as_str()
            .or_else(|| item["name"].as_str())
            .or_else(|| item["model"].as_str())
            .map(str::trim)
            .unwrap_or_default();
        if id.is_empty() || !seen.insert(id.to_owned()) {
            continue;
        }
        let name = item["display_name"]
            .as_str()
            .or_else(|| item["name"].as_str())
            .unwrap_or(id)
            .trim()
            .to_owned();
        let lower = id.to_ascii_lowercase();
        let reasoning = item["reasoning"].as_bool().unwrap_or_else(|| {
            lower.contains("r1")
                || lower.contains("qwq")
                || lower.contains("thinking")
                || lower.contains("reason")
                || lower.contains("o1")
                || lower.contains("o3")
                || lower.contains("o4")
                || lower.contains("claude-3-7")
                || lower.contains("claude-4")
                || lower.contains("claude-sonnet-4")
                || lower.contains("claude-opus-4")
                || lower.contains("gpt-5")
        });
        let vision = lower.contains("vl")
            || lower.contains("vision")
            || lower.contains("gpt-4o")
            || lower.contains("gpt-5")
            || lower.contains("claude")
            || lower.contains("gemini")
            || lower.contains("pixtral")
            || lower.contains("gemma-3");
        let context_window = item["context_window"]
            .as_u64()
            .or_else(|| item["context_length"].as_u64())
            .or_else(|| item["max_model_len"].as_u64())
            .map(|v| (v as u32).clamp(4096, 2_000_000))
            .unwrap_or(128_000);
        let max_tokens = item["max_tokens"]
            .as_u64()
            .or_else(|| item["max_completion_tokens"].as_u64())
            .map(|v| (v as u32).clamp(1024, 128_000))
            .unwrap_or(16_384);
        models.push(DiscoveredModel {
            id: id.to_owned(),
            name: if name.is_empty() { id.to_owned() } else { name },
            enabled: true,
            reasoning,
            vision,
            context_window,
            max_tokens,
        });
    }
    models
}

fn discover_models_from_endpoint(
    base_url: &str,
    api_key: &str,
) -> Result<Vec<DiscoveredModel>, String> {
    let trimmed = base_url.trim().trim_end_matches('/');
    if trimmed.is_empty() {
        return Err("Enter a base URL first (for example http://localhost:11434/v1).".into());
    }
    let parsed = url::Url::parse(trimmed)
        .map_err(|_| "Base URL must be a valid http:// or https:// URL.")?;
    if !matches!(parsed.scheme(), "http" | "https") {
        return Err("Base URL must start with http:// or https://.".into());
    }
    let models_url = if trimmed.ends_with("/models") {
        trimmed.to_owned()
    } else {
        format!("{trimmed}/models")
    };
    let fallback_url = if !trimmed.ends_with("/v1") && !trimmed.ends_with("/models") {
        Some(format!("{trimmed}/v1/models"))
    } else {
        None
    };
    let script = r#"
const [primaryUrl, fallbackUrl, apiKey] = process.argv.slice(-3);
async function tryFetch(target) {
  const headers = { 'Accept': 'application/json' };
  if (apiKey && !apiKey.startsWith('$') && !apiKey.startsWith('!')) {
    headers['Authorization'] = `Bearer ${apiKey}`;
    headers['x-api-key'] = apiKey;
  }
  const res = await fetch(target, { headers, signal: AbortSignal.timeout(10000) });
  const text = await res.text();
  return { ok: res.ok, status: res.status, body: text };
}
let result = await tryFetch(primaryUrl).catch(e => ({ ok: false, status: 0, error: String(e.message || e) }));
if (!result.ok && fallbackUrl) {
  const second = await tryFetch(fallbackUrl).catch(() => null);
  if (second && second.ok) result = second;
}
process.stdout.write(JSON.stringify(result));
"#;
    let mut command = std::process::Command::new(crate::runtime::node_binary());
    command
        .args(["--input-type=module", "--eval", script])
        .arg(&models_url)
        .arg(fallback_url.as_deref().unwrap_or(""))
        .arg(api_key.trim())
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::null());
    crate::runtime::configure(&mut command);
    let mut child = command
        .spawn()
        .map_err(|_| "Cannot start Node helper to query endpoint models.")?;
    let mut stdout = child.stdout.take().ok_or("Cannot read discovery output.")?;
    let reader = std::thread::spawn(move || {
        let mut bytes = Vec::new();
        stdout
            .by_ref()
            .take(2 * 1024 * 1024)
            .read_to_end(&mut bytes)
            .map(|_| bytes)
    });
    let deadline = std::time::Instant::now() + Duration::from_secs(12);
    loop {
        if child
            .try_wait()
            .map_err(|_| "Model discovery failed.")?
            .is_some()
        {
            break;
        }
        if std::time::Instant::now() >= deadline {
            let _ = child.kill();
            let _ = child.wait();
            let _ = reader.join();
            return Err("Model discovery timed out. Make sure your server is running.".into());
        }
        std::thread::sleep(Duration::from_millis(40));
    }
    let bytes = reader
        .join()
        .map_err(|_| "Discovery reader failed.")?
        .map_err(|_| "Cannot read model list response.")?;
    let reply: Value =
        serde_json::from_slice(&bytes).map_err(|_| "Endpoint did not return valid JSON.")?;
    if reply["ok"] != true {
        if let Some(err) = reply["error"].as_str() {
            return Err(format!("Could not connect to {models_url}: {err}"));
        }
        let status = reply["status"].as_u64().unwrap_or(0);
        return Err(format!(
            "Endpoint returned HTTP {status} at {models_url}. Check the Base URL or API key."
        ));
    }
    let body: Value = serde_json::from_str(reply["body"].as_str().unwrap_or("{}"))
        .map_err(|_| "Endpoint /models response is not valid JSON.")?;
    let models = parse_discovered_models(&body);
    if models.is_empty() {
        return Err(
            "Connected to endpoint, but its /models response returned 0 models. Load a model in your server or add a model ID below."
                .into(),
        );
    }
    Ok(models)
}

fn validate_provider_id(id: &str) -> Result<String, String> {
    let trimmed = id.trim();
    if trimmed.is_empty() {
        return Err("Provider ID is required (for example: local-openai or ollama).".into());
    }
    if !trimmed
        .bytes()
        .all(|b| b.is_ascii_alphanumeric() || b"._-".contains(&b))
    {
        return Err("Provider ID may only contain letters, numbers, '-', '_', or '.'.".into());
    }
    Ok(trimmed.to_owned())
}

impl ProvidersManager {
    pub(in crate::app) fn ensure_loaded(&mut self, demo: bool) {
        if self.loaded {
            return;
        }
        self.loaded = true;
        if demo || cfg!(test) {
            return;
        }
        self.reload_from_disk();
    }

    pub(in crate::app) fn reload_from_disk(&mut self) {
        self.custom_providers.clear();
        self.saved_auth.clear();
        if let Some(auth_path) = auth_json_path() {
            let root = read_json_object(&auth_path);
            if let Some(obj) = root.as_object() {
                for (provider, entry) in obj {
                    let kind = entry["type"].as_str().unwrap_or("api_key").to_owned();
                    self.saved_auth.push(AuthSummary {
                        provider: provider.clone(),
                        kind,
                    });
                }
            }
        }
        if let Some(models_path) = models_json_path() {
            let root = read_json_object(&models_path);
            if let Some(providers) = root["providers"].as_object() {
                for (id, config) in providers {
                    let base_url = config["baseUrl"].as_str().unwrap_or_default().to_owned();
                    let name = config["name"].as_str().unwrap_or(id).to_owned();
                    let api = config["api"]
                        .as_str()
                        .unwrap_or("openai-completions")
                        .to_owned();
                    let api_key = config["apiKey"].as_str().unwrap_or_default().to_owned();
                    let mut models = Vec::new();
                    if let Some(arr) = config["models"].as_array() {
                        for m in arr {
                            let Some(mid) = m["id"].as_str() else {
                                continue;
                            };
                            let mname = m["name"].as_str().unwrap_or(mid).to_owned();
                            let reasoning = m["reasoning"].as_bool().unwrap_or(false);
                            let vision = m["input"]
                                .as_array()
                                .is_some_and(|i| i.iter().any(|v| v.as_str() == Some("image")));
                            let context_window =
                                m["contextWindow"].as_u64().unwrap_or(128_000) as u32;
                            let max_tokens = m["maxTokens"].as_u64().unwrap_or(16_384) as u32;
                            models.push(DiscoveredModel {
                                id: mid.to_owned(),
                                name: mname,
                                enabled: true,
                                reasoning,
                                vision,
                                context_window,
                                max_tokens,
                            });
                        }
                    }
                    self.custom_providers.push(CustomProviderSummary {
                        id: id.clone(),
                        name,
                        base_url,
                        api,
                        api_key,
                        models,
                    });
                }
            }
        }
    }

    pub(in crate::app) fn poll(&mut self) {
        if let Some(result) = self.claude.rx.as_ref().and_then(|rx| rx.try_recv().ok()) {
            self.claude.rx = None;
            self.claude.exchanging = false;
            match result {
                Ok(()) => {
                    self.claude.open = false;
                    self.claude.code_input.clear();
                    self.claude.error = None;
                    self.reload_from_disk();
                    self.status_error = None;
                    self.status_notice = Some(
                        "Claude Pro/Max subscription connected! Reloading model catalog…".into(),
                    );
                    self.reload_requested = true;
                }
                Err(error) => {
                    self.claude.error = Some(error);
                }
            }
        }
        if let Some(result) = self
            .endpoint
            .discover_rx
            .as_ref()
            .and_then(|rx| rx.try_recv().ok())
        {
            self.endpoint.discover_rx = None;
            self.endpoint.discovering = false;
            match result {
                Ok(discovered) => {
                    let count = discovered.len();
                    self.endpoint.models = discovered;
                    self.endpoint.error = None;
                    self.endpoint.notice = Some(format!(
                        "Discovered {count} model{} from endpoint.",
                        if count == 1 { "" } else { "s" }
                    ));
                }
                Err(error) => {
                    self.endpoint.error = Some(error);
                    self.endpoint.notice = None;
                }
            }
        }
    }

    pub(in crate::app) fn open_claude_subscription(&mut self) {
        let (verifier, challenge) = generate_pkce();
        let authorize_url = build_claude_authorize_url(&challenge, &verifier);
        self.claude = ClaudeSubscriptionFlow {
            open: true,
            verifier,
            authorize_url,
            code_input: String::new(),
            exchanging: false,
            rx: None,
            error: None,
        };
    }

    pub(in crate::app) fn open_add_endpoint(&mut self, preset: Option<(&str, &str, &str)>) {
        let (id, name, base_url) =
            preset.unwrap_or(("local-openai", "Local OpenAI", "http://localhost:11434/v1"));
        self.endpoint = EndpointModal {
            open: true,
            editing_id: None,
            id: id.into(),
            name: name.into(),
            base_url: base_url.into(),
            api_index: 0,
            api_key: String::new(),
            manual_model_id: String::new(),
            models: Vec::new(),
            discovering: false,
            discover_rx: None,
            error: None,
            notice: None,
        };
    }

    pub(in crate::app) fn open_edit_endpoint(&mut self, summary: &CustomProviderSummary) {
        let api_index = APIS
            .iter()
            .position(|(k, _)| *k == summary.api)
            .unwrap_or(0);
        self.endpoint = EndpointModal {
            open: true,
            editing_id: Some(summary.id.clone()),
            id: summary.id.clone(),
            name: summary.name.clone(),
            base_url: summary.base_url.clone(),
            api_index,
            api_key: if summary.api_key == "local" {
                String::new()
            } else {
                summary.api_key.clone()
            },
            manual_model_id: String::new(),
            models: summary.models.clone(),
            discovering: false,
            discover_rx: None,
            error: None,
            notice: None,
        };
    }

    pub(in crate::app) fn open_api_key_modal(&mut self, provider_id: Option<&str>) {
        let idx = provider_id
            .and_then(|id| BUILTIN_KEY_PROVIDERS.iter().position(|(k, _)| *k == id))
            .unwrap_or(0);
        self.api_key_modal = ApiKeyModal {
            open: true,
            provider_index: idx,
            custom_provider: String::new(),
            api_key: String::new(),
            error: None,
        };
    }

    pub(in crate::app) fn remove_custom_provider(&mut self, id: &str) {
        let Some(path) = models_json_path() else {
            return;
        };
        let mut root = read_json_object(&path);
        if let Some(providers) = root["providers"].as_object_mut() {
            providers.remove(id);
        }
        match write_json_atomic(&path, &root, false) {
            Ok(()) => {
                self.reload_from_disk();
                self.status_error = None;
                self.status_notice =
                    Some(format!("Removed custom provider '{id}' from models.json."));
                self.reload_requested = true;
            }
            Err(err) => self.status_error = Some(err),
        }
    }

    pub(in crate::app) fn remove_saved_auth(&mut self, provider: &str) {
        let Some(path) = auth_json_path() else {
            return;
        };
        let mut root = read_json_object(&path);
        if let Some(obj) = root.as_object_mut() {
            obj.remove(provider);
        }
        match write_json_atomic(&path, &root, true) {
            Ok(()) => {
                self.reload_from_disk();
                self.status_error = None;
                self.status_notice = Some(format!("Removed stored credential for '{provider}'."));
                self.reload_requested = true;
            }
            Err(err) => self.status_error = Some(err),
        }
    }

    fn save_endpoint_to_disk(&mut self) -> Result<(), String> {
        let id = validate_provider_id(&self.endpoint.id)?;
        let base_url = self
            .endpoint
            .base_url
            .trim()
            .trim_end_matches('/')
            .to_owned();
        if base_url.is_empty() {
            return Err("Base URL is required.".into());
        }
        let parsed = url::Url::parse(&base_url)
            .map_err(|_| "Base URL must be a valid http:// or https:// URL.")?;
        if !matches!(parsed.scheme(), "http" | "https") {
            return Err("Base URL must start with http:// or https://.".into());
        }
        let selected_models: Vec<_> = self
            .endpoint
            .models
            .iter()
            .filter(|m| m.enabled && !m.id.trim().is_empty())
            .collect();
        if selected_models.is_empty() {
            return Err("Add or discover at least one model before saving this endpoint.".into());
        }
        let api = APIS
            .get(self.endpoint.api_index)
            .map(|(k, _)| *k)
            .unwrap_or("openai-completions");
        let api_key = if self.endpoint.api_key.trim().is_empty() {
            // Pi requires a non-empty apiKey to mark custom endpoint models as available.
            "local".to_owned()
        } else {
            self.endpoint.api_key.trim().to_owned()
        };
        let name = if self.endpoint.name.trim().is_empty() {
            id.clone()
        } else {
            self.endpoint.name.trim().to_owned()
        };
        let models_json: Vec<Value> = selected_models
            .into_iter()
            .map(|m| {
                let mut inputs = vec!["text"];
                if m.vision {
                    inputs.push("image");
                }
                json!({
                    "id": m.id.trim(),
                    "name": if m.name.trim().is_empty() { m.id.trim() } else { m.name.trim() },
                    "reasoning": m.reasoning,
                    "input": inputs,
                    "contextWindow": m.context_window.max(4096),
                    "maxTokens": m.max_tokens.max(1024),
                    "cost": {
                        "input": 0,
                        "output": 0,
                        "cacheRead": 0,
                        "cacheWrite": 0
                    }
                })
            })
            .collect();
        let path = models_json_path().ok_or("Cannot resolve ~/.pi/agent/models.json.")?;
        let mut root = read_json_object(&path);
        if !root["providers"].is_object() {
            root["$schema"] = json!("https://pi.dev/schemas/models.schema.json");
            root["providers"] = json!({});
        }
        if let Some(old_id) = &self.endpoint.editing_id {
            if old_id != &id {
                if let Some(map) = root["providers"].as_object_mut() {
                    map.remove(old_id);
                }
            }
        }
        root["providers"][&id] = json!({
            "name": name,
            "baseUrl": base_url,
            "api": api,
            "apiKey": api_key,
            "models": models_json,
        });
        write_json_atomic(&path, &root, false)?;
        self.endpoint.open = false;
        self.reload_from_disk();
        self.status_error = None;
        self.status_notice = Some(format!(
            "Saved endpoint '{id}' with {} model(s) to ~/.pi/agent/models.json.",
            root["providers"][&id]["models"]
                .as_array()
                .map_or(0, Vec::len)
        ));
        self.reload_requested = true;
        Ok(())
    }

    fn save_api_key_to_disk(&mut self) -> Result<(), String> {
        let provider = if !self.api_key_modal.custom_provider.trim().is_empty() {
            validate_provider_id(&self.api_key_modal.custom_provider)?
        } else {
            BUILTIN_KEY_PROVIDERS
                .get(self.api_key_modal.provider_index)
                .map(|(id, _)| (*id).to_owned())
                .unwrap_or_else(|| "anthropic".into())
        };
        let key = self.api_key_modal.api_key.trim();
        if key.is_empty() {
            return Err("Enter an API key (or a !command secret helper).".into());
        }
        let path = auth_json_path().ok_or("Cannot resolve ~/.pi/agent/auth.json.")?;
        let mut root = read_json_object(&path);
        root[&provider] = json!({
            "type": "api_key",
            "key": key,
        });
        write_json_atomic(&path, &root, true)?;
        self.api_key_modal.open = false;
        self.api_key_modal.api_key.clear();
        self.reload_from_disk();
        self.status_error = None;
        self.status_notice = Some(format!(
            "Saved API key for '{provider}' in ~/.pi/agent/auth.json."
        ));
        self.reload_requested = true;
        Ok(())
    }

    pub(in crate::app) fn show_modals(&mut self, ctx: &egui::Context) {
        self.show_claude_modal(ctx);
        self.show_endpoint_modal(ctx);
        self.show_api_key_modal(ctx);
    }

    fn show_claude_modal(&mut self, ctx: &egui::Context) {
        if !self.claude.open {
            return;
        }
        let mut submit = false;
        let mut open_browser = false;
        let response = prompt_ui::modal(
            ctx,
            egui::Id::new("pui-claude-subscription-modal"),
            440.0,
            |ui| {
                let mut close = prompt_ui::close(ui);
                prompt_ui::badge(ui, Icon::Spark, false);
                prompt_ui::heading(
                    ui,
                    "Connect Claude Code Subscription",
                    "Sign in with your Claude Pro or Claude Max subscription using Pi's OAuth PKCE flow.",
                );
                ui.add_space(12.0);
                prompt_ui::note(
                    ui,
                    "1. Click Open Claude Sign-In below to authorize in your browser.\n2. Copy the authorization code (code#state) shown by Claude.\n3. Paste it below and click Connect Subscription.",
                );
                ui.add_space(10.0);
                if prompt_ui::button(
                    ui,
                    "Open Claude Sign-In in Browser",
                    prompt_ui::Button::Secondary,
                    !self.claude.exchanging,
                )
                .clicked()
                {
                    open_browser = true;
                }
                ui.add_space(12.0);
                ui.label(theme::small(ui, "Authorization code or redirect URL:"));
                let edit = ui.add_enabled(
                    !self.claude.exchanging,
                    theme::text_field(&mut self.claude.code_input, "Paste code#state here…")
                        .id_salt("claude-oauth-code-input")
                        .desired_width(ui.available_width()),
                );
                if edit.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                    submit = true;
                }
                if let Some(error) = &self.claude.error {
                    ui.add_space(8.0);
                    ui.add(
                        egui::Label::new(
                            RichText::new(error)
                                .size(11.5)
                                .color(theme::palette(ui.ctx()).red),
                        )
                        .wrap(),
                    );
                }
                ui.add_space(18.0);
                prompt_ui::footer(ui, |ui| {
                    if prompt_ui::button(
                        ui,
                        if self.claude.exchanging {
                            "Connecting…"
                        } else {
                            "Connect Subscription"
                        },
                        prompt_ui::Button::Primary,
                        !self.claude.exchanging && !self.claude.code_input.trim().is_empty(),
                    )
                    .clicked()
                    {
                        submit = true;
                    }
                    close |= prompt_ui::button(
                        ui,
                        "Cancel",
                        prompt_ui::Button::Secondary,
                        !self.claude.exchanging,
                    )
                    .clicked();
                });
                close
            },
        );
        if open_browser {
            let _ = std::process::Command::new("/usr/bin/open")
                .arg(&self.claude.authorize_url)
                .spawn();
        }
        if submit && !self.claude.exchanging {
            match parse_oauth_code_input(&self.claude.code_input, &self.claude.verifier) {
                Ok((code, state)) => {
                    self.claude.exchanging = true;
                    self.claude.error = None;
                    let verifier = self.claude.verifier.clone();
                    let (tx, rx) = mpsc::channel();
                    self.claude.rx = Some(rx);
                    let ctx = ctx.clone();
                    std::thread::spawn(move || {
                        let _ = tx.send(exchange_claude_oauth_code(&code, &state, &verifier));
                        ctx.request_repaint();
                    });
                }
                Err(err) => {
                    self.claude.error = Some(err);
                }
            }
        }
        if !self.claude.exchanging && (response.should_close() || response.inner) {
            self.claude.open = false;
        }
    }

    fn show_endpoint_modal(&mut self, ctx: &egui::Context) {
        if !self.endpoint.open {
            return;
        }
        let mut trigger_discover = false;
        let mut trigger_save = false;
        let mut add_manual = false;
        let colors = theme::palette(ctx);
        let response = prompt_ui::modal(
            ctx,
            egui::Id::new("pui-openai-endpoint-modal"),
            520.0,
            |ui| {
                let mut close = prompt_ui::close(ui);
                prompt_ui::badge(ui, Icon::Globe, false);
                prompt_ui::heading(
                    ui,
                    if self.endpoint.editing_id.is_some() {
                        "Edit OpenAI-Compatible Endpoint"
                    } else {
                        "Add OpenAI-Compatible Endpoint"
                    },
                    "Connect a local or custom OpenAI/Anthropic-compatible server (Ollama, LM Studio, vLLM, llama.cpp, LiteLLM) and automatically discover its models.",
                );
                ui.add_space(10.0);
                ui.horizontal_wrapped(|ui| {
                    ui.label(theme::small(ui, "Quick presets:"));
                    for (label, pid, pname, url) in [
                        (
                            "Ollama (:11434)",
                            "ollama",
                            "Ollama",
                            "http://localhost:11434/v1",
                        ),
                        (
                            "LM Studio (:1234)",
                            "lmstudio",
                            "LM Studio",
                            "http://localhost:1234/v1",
                        ),
                        (
                            "vLLM / Local (:8000)",
                            "local-openai",
                            "Local OpenAI",
                            "http://localhost:8000/v1",
                        ),
                        (
                            "llama.cpp (:8080)",
                            "llamacpp",
                            "llama.cpp",
                            "http://localhost:8080/v1",
                        ),
                    ] {
                        if prompt_ui::button(
                            ui,
                            label,
                            prompt_ui::Button::Secondary,
                            !self.endpoint.discovering,
                        )
                        .clicked()
                        {
                            if self.endpoint.editing_id.is_none() {
                                self.endpoint.id = pid.into();
                                self.endpoint.name = pname.into();
                            }
                            self.endpoint.base_url = url.into();
                        }
                    }
                });
                ui.add_space(8.0);
                ui.columns(2, |cols| {
                    cols[0].label(theme::small(&cols[0], "Provider ID"));
                    cols[0].add_enabled(
                        self.endpoint.editing_id.is_none(),
                        theme::text_field(&mut self.endpoint.id, "local-openai")
                            .id_salt("endpoint-provider-id")
                            .desired_width(cols[0].available_width()),
                    );
                    cols[1].label(theme::small(&cols[1], "Display Name"));
                    cols[1].add(
                        theme::text_field(&mut self.endpoint.name, "Local OpenAI")
                            .id_salt("endpoint-display-name")
                            .desired_width(cols[1].available_width()),
                    );
                });
                ui.add_space(6.0);
                ui.label(theme::small(
                    ui,
                    "Base URL (e.g. http://localhost:11434/v1)",
                ));
                ui.horizontal(|ui| {
                    let width = (ui.available_width() - 140.0).max(160.0);
                    ui.add(
                        theme::text_field(&mut self.endpoint.base_url, "http://localhost:11434/v1")
                            .id_salt("endpoint-base-url")
                            .desired_width(width),
                    );
                    if prompt_ui::button(
                        ui,
                        if self.endpoint.discovering {
                            "Discovering…"
                        } else {
                            "Discover Models"
                        },
                        prompt_ui::Button::Primary,
                        !self.endpoint.discovering && !self.endpoint.base_url.trim().is_empty(),
                    )
                    .clicked()
                    {
                        trigger_discover = true;
                    }
                });
                ui.add_space(6.0);
                ui.columns(2, |cols| {
                    cols[0].label(theme::small(&cols[0], "API Protocol"));
                    egui::ComboBox::from_id_salt("endpoint-api-protocol")
                        .selected_text(APIS[self.endpoint.api_index].0)
                        .width(cols[0].available_width())
                        .show_ui(&mut cols[0], |ui| {
                            for (i, (key, label)) in APIS.iter().enumerate() {
                                if ui
                                    .selectable_label(
                                        self.endpoint.api_index == i,
                                        format!("{key} · {label}"),
                                    )
                                    .clicked()
                                {
                                    self.endpoint.api_index = i;
                                }
                            }
                        });
                    cols[1].label(theme::small(&cols[1], "API Key (optional · or $ENV_VAR)"));
                    let mask_endpoint_key = !self.endpoint.api_key.starts_with('$')
                        && !self.endpoint.api_key.starts_with('!');
                    cols[1].add(
                        theme::text_field(&mut self.endpoint.api_key, "Optional for local servers")
                            .id_salt("endpoint-api-key")
                            .password(mask_endpoint_key)
                            .desired_width(cols[1].available_width()),
                    );
                });
                ui.add_space(10.0);
                ui.horizontal(|ui| {
                    let width = (ui.available_width() - 120.0).max(140.0);
                    let edit = ui.add(
                        theme::text_field(
                            &mut self.endpoint.manual_model_id,
                            "Or type a model ID manually (e.g. qwen2.5-coder:32b)…",
                        )
                        .id_salt("endpoint-manual-model")
                        .desired_width(width),
                    );
                    if (edit.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)))
                        || prompt_ui::button(
                            ui,
                            "Add Model",
                            prompt_ui::Button::Secondary,
                            !self.endpoint.manual_model_id.trim().is_empty(),
                        )
                        .clicked()
                    {
                        add_manual = true;
                    }
                });

                if let Some(notice) = &self.endpoint.notice {
                    ui.add_space(6.0);
                    ui.label(RichText::new(notice).size(11.5).color(colors.green));
                }
                if let Some(error) = &self.endpoint.error {
                    ui.add_space(6.0);
                    ui.add(
                        egui::Label::new(RichText::new(error).size(11.5).color(colors.red)).wrap(),
                    );
                }

                ui.add_space(8.0);
                let enabled_count = self.endpoint.models.iter().filter(|m| m.enabled).count();
                ui.label(theme::small(
                    ui,
                    format!(
                        "Models ({enabled_count} of {} enabled):",
                        self.endpoint.models.len()
                    ),
                ));
                egui::Frame::new()
                    .fill(prompt_ui::subtle(ui.ctx()))
                    .stroke(Stroke::new(1.0, prompt_ui::border(ui.ctx())))
                    .corner_radius(8)
                    .inner_margin(10)
                    .show(ui, |ui| {
                        ui.set_width(ui.available_width());
                        if self.endpoint.models.is_empty() {
                            ui.label(theme::small(
                                ui,
                                "Click Discover Models to fetch models from /v1/models, or add a model ID above.",
                            ));
                        } else {
                            egui::ScrollArea::vertical()
                                .id_salt("endpoint-discovered-models-scroll")
                                .max_height(180.0)
                                .show(ui, |ui| {
                                    for model in &mut self.endpoint.models {
                                        ui.horizontal(|ui| {
                                            ui.checkbox(&mut model.enabled, "");
                                            ui.add(
                                                egui::Label::new(
                                                    RichText::new(&model.id)
                                                        .size(12.0)
                                                        .strong()
                                                        .color(colors.text),
                                                )
                                                .truncate(),
                                            );
                                            ui.with_layout(
                                                Layout::right_to_left(Align::Center),
                                                |ui| {
                                                    ui.checkbox(&mut model.vision, "Vision");
                                                    ui.checkbox(&mut model.reasoning, "Reasoning");
                                                },
                                            );
                                        });
                                    }
                                });
                        }
                    });

                ui.add_space(16.0);
                prompt_ui::footer(ui, |ui| {
                    if prompt_ui::button(
                        ui,
                        "Save Endpoint",
                        prompt_ui::Button::Primary,
                        !self.endpoint.discovering,
                    )
                    .clicked()
                    {
                        trigger_save = true;
                    }
                    close |= prompt_ui::button(
                        ui,
                        "Cancel",
                        prompt_ui::Button::Secondary,
                        !self.endpoint.discovering,
                    )
                    .clicked();
                });
                close
            },
        );

        if add_manual {
            let id = self.endpoint.manual_model_id.trim().to_owned();
            if !id.is_empty() {
                if !self.endpoint.models.iter().any(|m| m.id == id) {
                    let lower = id.to_ascii_lowercase();
                    self.endpoint.models.push(DiscoveredModel {
                        id: id.clone(),
                        name: id,
                        enabled: true,
                        reasoning: lower.contains("r1")
                            || lower.contains("qwq")
                            || lower.contains("thinking"),
                        vision: lower.contains("vl") || lower.contains("vision"),
                        context_window: 128_000,
                        max_tokens: 16_384,
                    });
                }
                self.endpoint.manual_model_id.clear();
                self.endpoint.error = None;
            }
        }
        if trigger_discover && !self.endpoint.discovering {
            self.endpoint.discovering = true;
            self.endpoint.error = None;
            self.endpoint.notice = None;
            let base_url = self.endpoint.base_url.clone();
            let api_key = self.endpoint.api_key.clone();
            let (tx, rx) = mpsc::channel();
            self.endpoint.discover_rx = Some(rx);
            let ctx = ctx.clone();
            std::thread::spawn(move || {
                let _ = tx.send(discover_models_from_endpoint(&base_url, &api_key));
                ctx.request_repaint();
            });
        }
        if trigger_save {
            if let Err(err) = self.save_endpoint_to_disk() {
                self.endpoint.error = Some(err);
            }
        }
        if !self.endpoint.discovering && (response.should_close() || response.inner) {
            self.endpoint.open = false;
        }
    }

    fn show_api_key_modal(&mut self, ctx: &egui::Context) {
        if !self.api_key_modal.open {
            return;
        }
        let mut save = false;
        let response = prompt_ui::modal(
            ctx,
            egui::Id::new("pui-provider-api-key-modal"),
            420.0,
            |ui| {
                let mut close = prompt_ui::close(ui);
                prompt_ui::badge(ui, Icon::Shield, false);
                prompt_ui::heading(
                    ui,
                    "Save Provider API Key",
                    "Saves an API key or !command helper to ~/.pi/agent/auth.json with private 0600 file permissions.",
                );
                ui.add_space(12.0);
                ui.label(theme::small(ui, "Provider"));
                egui::ComboBox::from_id_salt("api-key-provider-select")
                    .selected_text(format!(
                        "{} ({})",
                        BUILTIN_KEY_PROVIDERS[self.api_key_modal.provider_index].1,
                        BUILTIN_KEY_PROVIDERS[self.api_key_modal.provider_index].0
                    ))
                    .width(ui.available_width())
                    .show_ui(ui, |ui| {
                        for (i, (id, label)) in BUILTIN_KEY_PROVIDERS.iter().enumerate() {
                            if ui
                                .selectable_label(
                                    self.api_key_modal.provider_index == i,
                                    format!("{label} ({id})"),
                                )
                                .clicked()
                            {
                                self.api_key_modal.provider_index = i;
                            }
                        }
                    });
                ui.add_space(6.0);
                ui.label(theme::small(
                    ui,
                    "Or custom provider ID (leave blank to use selection above):",
                ));
                ui.add(
                    theme::text_field(
                        &mut self.api_key_modal.custom_provider,
                        "Optional provider ID (e.g. mistral, groq)…",
                    )
                    .id_salt("api-key-custom-provider")
                    .desired_width(ui.available_width()),
                );
                ui.add_space(8.0);
                ui.label(theme::small(ui, "API Key (or !command)"));
                let is_command = self.api_key_modal.api_key.trim_start().starts_with('!');
                let edit = ui.add(
                    theme::text_field(&mut self.api_key_modal.api_key, "sk-…")
                        .id_salt("api-key-secret-value")
                        .password(!is_command)
                        .desired_width(ui.available_width()),
                );
                if edit.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                    save = true;
                }
                if let Some(error) = &self.api_key_modal.error {
                    ui.add_space(8.0);
                    ui.add(
                        egui::Label::new(
                            RichText::new(error)
                                .size(11.5)
                                .color(theme::palette(ui.ctx()).red),
                        )
                        .wrap(),
                    );
                }
                ui.add_space(18.0);
                prompt_ui::footer(ui, |ui| {
                    if prompt_ui::button(
                        ui,
                        "Save API Key",
                        prompt_ui::Button::Primary,
                        !self.api_key_modal.api_key.trim().is_empty(),
                    )
                    .clicked()
                    {
                        save = true;
                    }
                    close |= prompt_ui::button(ui, "Cancel", prompt_ui::Button::Secondary, true)
                        .clicked();
                });
                close
            },
        );
        if save {
            if let Err(err) = self.save_api_key_to_disk() {
                self.api_key_modal.error = Some(err);
            }
        }
        if response.should_close() || response.inner {
            self.api_key_modal.open = false;
        }
    }

    pub(in crate::app) fn any_modal_open(&self) -> bool {
        self.claude.open || self.endpoint.open || self.api_key_modal.open
    }

    pub(in crate::app) fn escape(&mut self) -> bool {
        if self.claude.open && !self.claude.exchanging {
            self.claude.open = false;
            return true;
        }
        if self.endpoint.open && !self.endpoint.discovering {
            self.endpoint.open = false;
            return true;
        }
        if self.api_key_modal.open {
            self.api_key_modal.open = false;
            return true;
        }
        false
    }
}

impl Pui {
    pub(in crate::app) fn refresh_providers_catalog(&mut self) {
        self.send(protocol::command("get_available_models"));
        self.send(protocol::command("get_state"));
        self.send(protocol::command("get_available_thinking_levels"));
    }

    pub(in crate::app) fn settings_providers(&mut self, ui: &mut egui::Ui) {
        self.settings_view.providers.ensure_loaded(self.demo);
        let colors = theme::palette(ui.ctx());

        if let Some(notice) = &self.settings_view.providers.status_notice {
            ui.add_space(8.0);
            ui.label(RichText::new(notice).size(12.0).color(colors.green));
        }
        if let Some(error) = &self.settings_view.providers.status_error {
            ui.add_space(8.0);
            ui.label(RichText::new(error).size(12.0).color(colors.red));
        }

        // 1. Quick Add / Connect section
        page::section(
            ui,
            "Add or connect provider",
            "Connect your Claude Code subscription, a local OpenAI-compatible server with auto model discovery, or a provider API key.",
        );
        let claude_connected = self
            .settings_view
            .providers
            .saved_auth
            .iter()
            .any(|a| a.provider == "anthropic" && a.kind == "oauth");
        page::card(ui, |ui| {
            page::row(
                ui,
                "Claude Code Subscription (Claude Pro / Max)",
                if claude_connected {
                    "Connected via OAuth in ~/.pi/agent/auth.json. Claude models are available in the model picker."
                } else {
                    "Sign in with your Anthropic Claude Pro or Max subscription via OAuth."
                },
                |ui| {
                    if ui
                        .button(if claude_connected {
                            "Reconnect Claude…"
                        } else {
                            "Connect Subscription…"
                        })
                        .clicked()
                    {
                        self.settings_view.providers.open_claude_subscription();
                    }
                },
            );
            page::separator(ui);
            page::row(
                ui,
                "OpenAI-Compatible Endpoint (Local / Custom)",
                "Connect Ollama, LM Studio, vLLM, llama.cpp or any /v1/models OpenAI-compatible server with automatic model discovery.",
                |ui| {
                    if ui.button("Add Endpoint…").clicked() {
                        self.settings_view.providers.open_add_endpoint(None);
                    }
                },
            );
            page::separator(ui);
            page::row(
                ui,
                "Provider API Key",
                "Store an API key for Anthropic, OpenAI, Google Gemini, OpenRouter, DeepSeek or xAI.",
                |ui| {
                    if ui.button("Add API Key…").clicked() {
                        self.settings_view.providers.open_api_key_modal(None);
                    }
                },
            );
        });

        // 2. Custom & Local OpenAI-Compatible Endpoints
        let custom_list = self.settings_view.providers.custom_providers.clone();
        if !custom_list.is_empty() {
            page::section(
                ui,
                "OpenAI-compatible & custom endpoints",
                "Configured in ~/.pi/agent/models.json. Edit or re-run auto model discovery anytime.",
            );
            let mut edit_target = None;
            let mut remove_target = None;
            let mut rediscover_target = None;
            page::card(ui, |ui| {
                for (index, item) in custom_list.iter().enumerate() {
                    if index > 0 {
                        page::separator(ui);
                    }
                    let model_names: Vec<&str> =
                        item.models.iter().take(4).map(|m| m.id.as_str()).collect();
                    let summary = if item.models.is_empty() {
                        format!("{} · {} · no models listed", item.base_url, item.api)
                    } else if item.models.len() > 4 {
                        format!(
                            "{} · {} models ({}, +{} more)",
                            item.base_url,
                            item.models.len(),
                            model_names.join(", "),
                            item.models.len() - 4
                        )
                    } else {
                        format!(
                            "{} · {} model(s): {}",
                            item.base_url,
                            item.models.len(),
                            model_names.join(", ")
                        )
                    };
                    page::row(
                        ui,
                        &format!("{} ({})", item.name, item.id),
                        &summary,
                        |ui| {
                            if ui.button("Remove").clicked() {
                                remove_target = Some(item.id.clone());
                            }
                            if ui.button("Edit / Discover…").clicked() {
                                edit_target = Some(item.clone());
                            }
                            if item.models.is_empty() && ui.button("Discover").clicked() {
                                rediscover_target = Some(item.clone());
                            }
                        },
                    );
                }
            });
            if let Some(target) = edit_target.or(rediscover_target) {
                self.settings_view.providers.open_edit_endpoint(&target);
            }
            if let Some(id) = remove_target {
                self.settings_view.providers.remove_custom_provider(&id);
            }
        }

        // 3. Active providers from Pi
        page::section(
            ui,
            "Active providers in Pi",
            "Providers currently active in your Pi model catalog. Newly added endpoints or credentials appear after refreshing or starting a new chat.",
        );
        let mut providers = BTreeMap::<String, usize>::new();
        for model in &self.state.models {
            *providers.entry(model.provider.clone()).or_default() += 1;
        }
        let saved_auth = self.settings_view.providers.saved_auth.clone();
        if providers.is_empty() && saved_auth.is_empty() {
            page::card(ui, |ui| {
                page::row(
                    ui,
                    "No catalog loaded",
                    "Connect a provider above or open a workspace chat to load models from Pi.",
                    |ui| {
                        if ui.button("Refresh catalog").clicked() {
                            self.refresh_providers_catalog();
                        }
                    },
                )
            });
        } else {
            let mut remove_auth = None;
            page::card(ui, |ui| {
                let mut rows_drawn = 0;
                for (provider, count) in &providers {
                    if rows_drawn > 0 {
                        page::separator(ui);
                    }
                    rows_drawn += 1;
                    let auth_badge = saved_auth
                        .iter()
                        .find(|a| &a.provider == provider)
                        .map(|a| {
                            if a.kind == "oauth" {
                                " · OAuth subscription"
                            } else {
                                " · Stored API key"
                            }
                        })
                        .unwrap_or("");
                    page::row(
                        ui,
                        provider,
                        &format!("{count} models available from Pi{auth_badge}"),
                        |ui| {
                            if saved_auth.iter().any(|a| &a.provider == provider)
                                && ui.button("Sign out").clicked()
                            {
                                remove_auth = Some(provider.clone());
                            }
                            ui.label(theme::small(
                                ui,
                                if provider == &self.state.provider {
                                    "Current provider"
                                } else {
                                    "Available"
                                },
                            ));
                        },
                    );
                }
                for auth in &saved_auth {
                    if providers.contains_key(&auth.provider) {
                        continue;
                    }
                    if rows_drawn > 0 {
                        page::separator(ui);
                    }
                    rows_drawn += 1;
                    let kind_label = if auth.kind == "oauth" {
                        "OAuth subscription saved · reload chat to load models"
                    } else {
                        "API key saved · reload chat to load models"
                    };
                    page::row(ui, &auth.provider, kind_label, |ui| {
                        if ui.button("Sign out").clicked() {
                            remove_auth = Some(auth.provider.clone());
                        }
                    });
                }
            });
            if let Some(provider) = remove_auth {
                self.settings_view.providers.remove_saved_auth(&provider);
            }
        }

        // 4. Reload / CLI fallback section
        page::section(ui, "Catalog & CLI sign-in", "");
        page::card(ui, |ui| {
            page::row(
                ui,
                "Refresh & reload active chat models",
                "Reloads ~/.pi/agent/models.json and ~/.pi/agent/auth.json in Pi so newly added providers and models appear immediately.",
                |ui| {
                    if ui.button("Reload chat & models").clicked() {
                        self.settings_view.providers.reload_from_disk();
                        self.refresh_providers_catalog();
                        if self.connected && self.state.ready() && !self.demo {
                            self.request_resource_reload();
                        }
                    }
                },
            );
            page::separator(ui);
            page::row(
                ui,
                "Terminal sign-in (All Pi providers)",
                "For other OAuth providers (GitHub Copilot, OpenAI ChatGPT/Codex, Radius, Google Vertex), you can also run Pi's interactive /login in your terminal.",
                |ui| {
                    let command = if crate::runtime::active_bundle().is_some() {
                        format!(
                            "'{}' /login",
                            crate::runtime::pi_binary()
                                .to_string_lossy()
                                .replace('\'', "'\\''")
                        )
                    } else {
                        "pi /login".into()
                    };
                    if ui
                        .button("Copy sign-in command")
                        .on_hover_text(&command)
                        .clicked()
                    {
                        ui.ctx().copy_text(command);
                    }
                },
            );
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_openai_and_ollama_model_discovery_payloads() {
        let payload = json!({
            "object": "list",
            "data": [
                {
                    "id": "qwen2.5-coder:32b",
                    "object": "model",
                    "context_length": 65536
                },
                {
                    "id": "deepseek-r1:14b",
                    "object": "model"
                },
                {
                    "id": "llava-vision:7b",
                    "object": "model"
                }
            ]
        });
        let models = parse_discovered_models(&payload);
        assert_eq!(models.len(), 3);
        assert_eq!(models[0].id, "qwen2.5-coder:32b");
        assert_eq!(models[0].context_window, 65536);
        assert!(!models[0].reasoning);
        assert!(models[1].reasoning);
        assert!(models[2].vision);
    }

    #[test]
    fn parses_claude_oauth_code_and_verifies_state() {
        let (code, state) = parse_oauth_code_input("abc123code#state999", "state999").unwrap();
        assert_eq!(code, "abc123code");
        assert_eq!(state, "state999");

        let url = "https://platform.claude.com/oauth/code/callback?code=urlcode&state=state999";
        let (code2, state2) = parse_oauth_code_input(url, "state999").unwrap();
        assert_eq!(code2, "urlcode");
        assert_eq!(state2, "state999");

        assert!(parse_oauth_code_input("abc#wrongstate", "state999").is_err());
        assert!(parse_oauth_code_input("   ", "state999").is_err());
    }
}
