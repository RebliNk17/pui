//! Native provider management: Claude Pro/Max subscription OAuth, Google Antigravity
//! OAuth, Google Gemini & Vertex AI credentials, API keys, and OpenAI/Anthropic/Google
//! compatible endpoints with automatic model discovery.
use super::*;
use base64::Engine as _;
use sha2::{Digest, Sha256};
use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

mod accounts;

const ANTHROPIC_CLIENT_ID: &str = "9d1c250a-e61b-44d9-88ed-5944d1962f5e";
const ANTHROPIC_TOKEN_URL: &str = "https://platform.claude.com/v1/oauth/token";
const ANTHROPIC_COPY_REDIRECT_URI: &str = "https://platform.claude.com/oauth/code/callback";

const ANTIGRAVITY_AUTHORIZE_URL: &str = "https://accounts.google.com/o/oauth2/v2/auth";
const ANTIGRAVITY_TOKEN_URL: &str = "https://oauth2.googleapis.com/token";
/// Google's installed-app OAuth client shipped in the Antigravity app and `agy` CLI. Installed-app
/// client secrets are not confidential (RFC 8252); PKCE protects the exchange. Using it lets Pui
/// offer the same one-click Google sign-in as agy. ANTIGRAVITY_CLIENT_ID/SECRET or the Advanced
/// fields override it with your own Google Cloud OAuth client.
const ANTIGRAVITY_DEFAULT_CLIENT_ID: &str =
    "1071006060591-tmhssin2h21lcre235vtolojh4g403ep.apps.googleusercontent.com";
const ANTIGRAVITY_DEFAULT_CLIENT_SECRET: &str = "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";
const ANTIGRAVITY_REDIRECT_URI: &str = "http://localhost:51121/oauth-callback";
const ANTIGRAVITY_SCOPES: &str = "https://www.googleapis.com/auth/aicode https://www.googleapis.com/auth/cloud-platform https://www.googleapis.com/auth/userinfo.email https://www.googleapis.com/auth/userinfo.profile https://www.googleapis.com/auth/cclog https://www.googleapis.com/auth/experimentsandconfigs";

const APIS: [(&str, &str); 5] = [
    (
        "openai-completions",
        "OpenAI Chat Completions (/v1/chat/completions)",
    ),
    ("openai-responses", "OpenAI Responses (/v1/responses)"),
    ("anthropic-messages", "Anthropic Messages (/v1/messages)"),
    (
        "google-generative-ai",
        "Google Generative AI / Gemini (/v1beta/models)",
    ),
    ("google-vertex", "Google Vertex AI (/v1)"),
];

const BUILTIN_KEY_PROVIDERS: [(&str, &str); 6] = [
    ("google", "Google Gemini (GEMINI_API_KEY)"),
    ("anthropic", "Anthropic (Claude API Key)"),
    ("openai", "OpenAI"),
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
    pub detail: Option<String>,
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

#[derive(Default)]
struct AntigravitySubscriptionFlow {
    open: bool,
    /// auth.json key: `antigravity` or an extra account such as `antigravity~2`.
    target: String,
    /// Show the custom-client and paste-callback controls.
    advanced: bool,
    /// Custom OAuth client; empty uses the built-in Antigravity client.
    client_id: String,
    client_secret: String,
    verifier: String,
    challenge: String,
    state: String,
    callback_input: String,
    exchanging: bool,
    listening: bool,
    rx: Option<Receiver<Result<String, String>>>,
    cancelled: Arc<AtomicBool>,
    child: Arc<Mutex<Option<std::process::Child>>>,
    error: Option<String>,
}

impl AntigravitySubscriptionFlow {
    fn cancel(&mut self) {
        self.cancelled.store(true, Ordering::Relaxed);
        if let Ok(mut slot) = self.child.lock()
            && let Some(mut child) = slot.take()
        {
            let _ = child.kill();
            let _ = child.wait();
        }
        self.rx = None;
        self.exchanging = false;
        self.listening = false;
    }
}

impl Drop for AntigravitySubscriptionFlow {
    fn drop(&mut self) {
        self.cancel();
    }
}

struct VertexModal {
    open: bool,
    mode: usize, // 0 = ADC, 1 = API Key, 2 = Service Account JSON
    project_id: String,
    location: String,
    api_key: String,
    credentials_path: String,
    error: Option<String>,
}

impl Default for VertexModal {
    fn default() -> Self {
        Self {
            open: false,
            mode: 0,
            project_id: String::new(),
            location: "us-central1".into(),
            api_key: String::new(),
            credentials_path: String::new(),
            error: None,
        }
    }
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
    /// Keep the provider's existing key and save this one as another account.
    additional: bool,
    provider_index: usize,
    custom_provider: String,
    api_key: String,
    error: Option<String>,
}

#[derive(Default)]
pub(in crate::app) struct ProvidersManager {
    loaded: bool,
    /// Last observed (mtime, len) of auth.json/models.json and when they were checked.
    config_stamp: Option<Vec<Option<(std::time::SystemTime, u64)>>>,
    config_checked: Option<std::time::Instant>,
    disk_loaded: bool,
    pub(in crate::app) custom_providers: Vec<CustomProviderSummary>,
    pub(in crate::app) saved_auth: Vec<AuthSummary>,
    pub(in crate::app) model_overrides:
        std::collections::BTreeMap<(String, String), super::model_editor::ModelOverride>,
    pub(in crate::app) model_thinking_levels: std::collections::BTreeMap<(String, String), String>,
    pub(in crate::app) model_fallbacks: std::collections::BTreeMap<(String, String), String>,
    claude: ClaudeSubscriptionFlow,
    antigravity: AntigravitySubscriptionFlow,
    vertex: VertexModal,
    endpoint: EndpointModal,
    api_key_modal: ApiKeyModal,
    accounts: accounts::AccountsState,
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

fn settings_json_path() -> Option<PathBuf> {
    agent_dir().map(|dir| dir.join("settings.json"))
}

fn fallbacks_json_path() -> Option<PathBuf> {
    agent_dir().map(|dir| dir.join(super::model_editor::FALLBACKS_FILE))
}

fn read_json_object(path: &Path) -> Value {
    std::fs::read_to_string(path)
        .ok()
        .and_then(|s| serde_json::from_str::<Value>(&s).ok())
        .filter(|v| v.is_object())
        .unwrap_or_else(|| json!({}))
}

pub(super) fn write_json_atomic(
    path: &Path,
    value: &Value,
    private_mode: bool,
) -> Result<(), String> {
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

/// Field label that wraps left-aligned. `ui.columns` uses a justified layout, where egui would
/// stretch each wrapped line's glyphs across the column ("G o o g l e").
fn field_label(ui: &mut egui::Ui, text: &str) {
    ui.with_layout(egui::Layout::top_down(egui::Align::Min), |ui| {
        ui.add(egui::Label::new(theme::small(ui, text)).wrap());
    });
}

/// Custom client when provided, otherwise the built-in Antigravity client and its public secret.
fn antigravity_client(client_id: &str, client_secret: &str) -> (String, String) {
    let id = client_id.trim();
    if id.is_empty() || id == ANTIGRAVITY_DEFAULT_CLIENT_ID {
        (
            ANTIGRAVITY_DEFAULT_CLIENT_ID.into(),
            ANTIGRAVITY_DEFAULT_CLIENT_SECRET.into(),
        )
    } else {
        (id.into(), client_secret.trim().into())
    }
}

fn build_antigravity_authorize_url(client_id: &str, challenge: &str, state: &str) -> String {
    let mut url = url::Url::parse(ANTIGRAVITY_AUTHORIZE_URL).expect("valid google authorize url");
    url.query_pairs_mut()
        .append_pair("client_id", client_id.trim())
        .append_pair("response_type", "code")
        .append_pair("redirect_uri", ANTIGRAVITY_REDIRECT_URI)
        .append_pair("scope", ANTIGRAVITY_SCOPES)
        .append_pair("code_challenge", challenge)
        .append_pair("code_challenge_method", "S256")
        .append_pair("state", state)
        .append_pair("access_type", "offline")
        .append_pair("prompt", "consent");
    url.to_string()
}

pub(in crate::app) fn parse_oauth_code_input(
    input: &str,
    expected_state: &str,
) -> Result<(String, String), String> {
    let trimmed = input.trim();
    if trimmed.is_empty() {
        return Err("Paste the authorization code or callback URL after signing in.".into());
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
    } else if trimmed.contains("code=") {
        let qs = trimmed.trim_start_matches('?');
        let fake_url = format!("http://localhost/callback?{qs}");
        if let Ok(parsed) = url::Url::parse(&fake_url) {
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
        } else {
            (Some(trimmed.to_owned()), None)
        }
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
            return Err("OAuth state mismatch. Click Sign In again to start a fresh flow.".into());
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

fn exchange_antigravity_oauth_code(
    client_id: &str,
    client_secret: &str,
    code: &str,
    verifier: &str,
    target: &str,
    cancelled: &AtomicBool,
) -> Result<String, String> {
    if cancelled.load(Ordering::Relaxed) {
        return Err("Sign-in cancelled.".into());
    }
    let (client_id, client_secret) = antigravity_client(client_id, client_secret);
    let (client_id, client_secret) = (client_id.as_str(), client_secret.as_str());
    let script = r#"
import { createHash } from 'node:crypto';
const [tokenUrl, clientId, clientSecret, redirectUri, code, verifier] = process.argv.slice(-6);
const params = {
  client_id: clientId,
  code,
  grant_type: 'authorization_code',
  redirect_uri: redirectUri,
  code_verifier: verifier,
};
if (clientSecret) params.client_secret = clientSecret;
const tokenRes = await fetch(tokenUrl, {
  method: 'POST',
  headers: { 'Content-Type': 'application/x-www-form-urlencoded' },
  body: new URLSearchParams(params).toString(),
  signal: AbortSignal.timeout(20000),
});
const tokenText = await tokenRes.text();
if (!tokenRes.ok) {
  process.stdout.write(JSON.stringify({ ok: false, status: tokenRes.status, error: tokenText.slice(0, 300) }));
  process.exit(0);
}
const tokenData = JSON.parse(tokenText);
const access = tokenData.access_token;
const refresh = tokenData.refresh_token;
const expiresIn = tokenData.expires_in || 3600;
let email;
try {
  const userRes = await fetch('https://www.googleapis.com/oauth2/v1/userinfo?alt=json', {
    headers: { Authorization: `Bearer ${access}` },
    signal: AbortSignal.timeout(8000),
  });
  if (userRes.ok) {
    const u = await userRes.json();
    email = u.email;
  }
} catch {}
let projectId;
for (const ep of ['https://cloudcode-pa.googleapis.com', 'https://daily-cloudcode-pa.sandbox.googleapis.com']) {
  try {
    const res = await fetch(`${ep}/v1internal:loadCodeAssist`, {
      method: 'POST',
      headers: {
        Authorization: `Bearer ${access}`,
        'Content-Type': 'application/json',
        'User-Agent': 'antigravity/2.23.0 darwin/arm64',
        'X-Goog-Api-Client': 'google-cloud-sdk vscode_cloudshelleditor/0.1',
      },
      body: JSON.stringify({ metadata: { ideType: 'ANTIGRAVITY', platform: 'PLATFORM_UNSPECIFIED', pluginType: 'GEMINI' } }),
      signal: AbortSignal.timeout(8000),
    });
    if (res.ok) {
      const d = await res.json();
      const p = d.antigravityProjectId || d.projectId || d.backendProjectId || d.cloudaicompanionProject || (d.cloudaicompanionProject && d.cloudaicompanionProject.id);
      if (typeof p === 'string' && p) { projectId = p; break; }
    }
  } catch {}
}
if (!projectId) {
  const bytes = createHash('sha1').update(`antigravity:${email || 'antigravity-default'}`).digest().subarray(0, 16);
  bytes[6] = (bytes[6] & 0x0f) | 0x50;
  bytes[8] = (bytes[8] & 0x3f) | 0x80;
  const hex = bytes.toString('hex');
  projectId = `${hex.slice(0, 8)}-${hex.slice(8, 12)}-${hex.slice(12, 16)}-${hex.slice(16, 20)}-${hex.slice(20)}`;
}
process.stdout.write(JSON.stringify({ ok: true, access, refresh, expiresIn, email, projectId }));
"#;
    let mut command = std::process::Command::new(crate::runtime::node_binary());
    command
        .args(["--input-type=module", "--eval", script])
        .arg(ANTIGRAVITY_TOKEN_URL)
        .arg(client_id)
        .arg(client_secret)
        .arg(ANTIGRAVITY_REDIRECT_URI)
        .arg(code)
        .arg(verifier)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::null());
    crate::runtime::configure(&mut command);
    let output = command
        .output()
        .map_err(|_| "Cannot launch Node runtime to exchange Google OAuth code.")?;
    if !output.status.success() {
        return Err("Google OAuth token exchange process failed.".into());
    }
    let parsed: Value = serde_json::from_slice(&output.stdout)
        .map_err(|_| "Invalid response from Google OAuth token exchange.")?;
    if parsed["ok"] != true {
        let status = parsed["status"].as_u64().unwrap_or(0);
        return Err(format!(
            "Google OAuth rejected the authorization code (HTTP {status}). Try signing in again."
        ));
    }
    let access = parsed["access"]
        .as_str()
        .filter(|s| !s.is_empty())
        .ok_or("Google token response is missing access_token.")?;
    let refresh = parsed["refresh"]
        .as_str()
        .filter(|s| !s.is_empty())
        .ok_or("Google token response is missing refresh_token.")?;
    let expires_in = parsed["expiresIn"].as_i64().unwrap_or(3600);
    let project_id = parsed["projectId"]
        .as_str()
        .unwrap_or("antigravity-default");
    let email = parsed["email"].as_str().unwrap_or_default().to_owned();
    let now_ms = chrono::Utc::now().timestamp_millis();
    let expires = now_ms + expires_in * 1000 - 5 * 60 * 1000;

    if cancelled.load(Ordering::Relaxed) {
        return Err("Sign-in cancelled.".into());
    }
    let auth_path = auth_json_path().ok_or("Cannot resolve ~/.pi/agent/auth.json path.")?;
    let mut auth = read_json_object(&auth_path);
    let mut entry = json!({
        "type": "oauth",
        "access": access,
        "refresh": refresh,
        "expires": expires,
        "projectId": project_id,
        "clientId": client_id,
    });
    if !client_secret.is_empty() {
        entry["clientSecret"] = json!(client_secret);
    }
    if !email.is_empty() {
        entry["email"] = json!(email.clone());
    }
    auth[target] = entry;
    write_json_atomic(&auth_path, &auth, true)?;
    Ok(email)
}

#[allow(clippy::too_many_arguments)]
fn run_antigravity_browser_login(
    client: (&str, &str),
    authorize_url: &str,
    expected_state: &str,
    verifier: &str,
    target: &str,
    cancelled: &AtomicBool,
    child_slot: &Arc<Mutex<Option<std::process::Child>>>,
) -> Result<String, String> {
    let (client_id, client_secret) = client;
    let script = r#"
import { createServer } from 'node:http';
import { spawn } from 'node:child_process';
const [authUrl, expectedState] = process.argv.slice(-2);
const server = createServer((req, res) => {
  const url = new URL(req.url || '', 'http://localhost:51121');
  if (url.pathname !== '/oauth-callback') {
    res.writeHead(404, { 'Content-Type': 'text/plain; charset=utf-8' });
    res.end('Not found');
    return;
  }
  const code = url.searchParams.get('code');
  const state = url.searchParams.get('state');
  const error = url.searchParams.get('error');
  if (error || !code || state !== expectedState) {
    res.writeHead(400, { 'Content-Type': 'text/html; charset=utf-8' });
    res.end('<h3>Google Antigravity sign-in failed.</h3><p>Return to Pui and try again.</p>');
    process.stdout.write(JSON.stringify({ ok: false, error: error || 'Invalid state or missing code' }));
    setTimeout(() => process.exit(0), 100);
    return;
  }
  res.writeHead(200, { 'Content-Type': 'text/html; charset=utf-8' });
  res.end('<h3>Google Antigravity connected!</h3><p>You can close this tab and return to Pui.</p>');
  process.stdout.write(JSON.stringify({ ok: true, code }));
  setTimeout(() => process.exit(0), 100);
});
server.on('error', (err) => {
  process.stdout.write(JSON.stringify({ ok: false, error: String(err.message || err) }));
  process.exit(0);
});
server.listen(51121, '127.0.0.1', () => {
  try {
    spawn('/usr/bin/open', [authUrl], { detached: true, stdio: 'ignore' }).unref();
  } catch {}
});
setTimeout(() => {
  process.stdout.write(JSON.stringify({ ok: false, error: 'Timed out waiting for browser sign-in. You can also paste the callback URL manually.' }));
  process.exit(0);
}, 180000);
"#;
    let mut command = std::process::Command::new(crate::runtime::node_binary());
    command
        .args(["--input-type=module", "--eval", script])
        .arg(authorize_url)
        .arg(expected_state)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::null());
    crate::runtime::configure(&mut command);
    let mut child = command.spawn().map_err(|_| {
        "Cannot start local OAuth callback listener. Paste the callback URL below instead."
            .to_owned()
    })?;
    let mut stdout = child.stdout.take();
    if let Ok(mut slot) = child_slot.lock() {
        *slot = Some(child);
    }
    let mut raw = Vec::new();
    if let Some(out) = stdout.as_mut() {
        let _ = out.read_to_end(&mut raw);
    }
    if let Ok(mut slot) = child_slot.lock()
        && let Some(mut child) = slot.take()
    {
        let _ = child.wait();
    }
    if cancelled.load(Ordering::Relaxed) {
        return Err("Sign-in cancelled.".into());
    }
    let reply: Value = serde_json::from_slice(&raw).map_err(|_| {
        "Browser callback listener exited before completing sign-in. Paste the callback URL below if you signed in."
            .to_owned()
    })?;
    if reply["ok"] != true {
        let err = reply["error"]
            .as_str()
            .unwrap_or("Local callback failed. Paste the callback URL below.");
        return Err(err.to_owned());
    }
    let code = reply["code"]
        .as_str()
        .filter(|s| !s.is_empty())
        .ok_or("Callback did not contain an authorization code.")?;
    exchange_antigravity_oauth_code(client_id, client_secret, code, verifier, target, cancelled)
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
        let raw_id = item["id"]
            .as_str()
            .or_else(|| item["name"].as_str())
            .or_else(|| item["model"].as_str())
            .map(str::trim)
            .unwrap_or_default();
        // Google /v1beta/models returns names like "models/gemini-2.5-pro"; strip the leading "models/" prefix.
        let id = raw_id.strip_prefix("models/").unwrap_or(raw_id);
        if id.is_empty() || !seen.insert(id.to_owned()) {
            continue;
        }
        // If Google's supportedGenerationMethods is present, skip pure embedding/attributed models.
        if let Some(methods) = item["supportedGenerationMethods"].as_array() {
            let can_generate = methods.iter().any(|m| {
                matches!(
                    m.as_str(),
                    Some("generateContent") | Some("streamGenerateContent")
                )
            });
            if !can_generate {
                continue;
            }
        }
        let name = item["displayName"]
            .as_str()
            .or_else(|| item["display_name"].as_str())
            .or_else(|| item["name"].as_str())
            .map(|n| n.strip_prefix("models/").unwrap_or(n))
            .unwrap_or(id)
            .trim()
            .to_owned();
        let lower = id.to_ascii_lowercase();
        let reasoning = item["reasoning"]
            .as_bool()
            .or_else(|| item["thinking"].as_bool())
            .unwrap_or_else(|| {
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
                    || lower.contains("gemini-2.5")
                    || lower.contains("gemini-3")
                    || lower.contains("gemini-4")
            });
        let vision = lower.contains("vl")
            || lower.contains("vision")
            || lower.contains("gpt-4o")
            || lower.contains("gpt-5")
            || lower.contains("claude")
            || lower.contains("gemini")
            || lower.contains("pixtral")
            || lower.contains("gemma-3")
            || lower.contains("gemma-4");
        let context_window = item["context_window"]
            .as_u64()
            .or_else(|| item["context_length"].as_u64())
            .or_else(|| item["max_model_len"].as_u64())
            .or_else(|| item["inputTokenLimit"].as_u64())
            .map(|v| (v as u32).clamp(4096, 2_000_000))
            .unwrap_or(128_000);
        let max_tokens = item["max_tokens"]
            .as_u64()
            .or_else(|| item["max_completion_tokens"].as_u64())
            .or_else(|| item["outputTokenLimit"].as_u64())
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
    let fallback_url = if !trimmed.ends_with("/v1")
        && !trimmed.ends_with("/v1beta")
        && !trimmed.ends_with("/models")
    {
        Some(format!("{trimmed}/v1/models"))
    } else {
        None
    };
    let script = r#"
const [primaryUrl, fallbackUrl, apiKey] = process.argv.slice(-3);
async function tryFetch(target) {
  const headers = { 'Accept': 'application/json' };
  let url = target;
  if (apiKey && !apiKey.startsWith('$') && !apiKey.startsWith('!')) {
    headers['Authorization'] = `Bearer ${apiKey}`;
    headers['x-api-key'] = apiKey;
    headers['x-goog-api-key'] = apiKey;
  }
  const res = await fetch(url, { headers, signal: AbortSignal.timeout(10000) });
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
            "Connected to endpoint, but its /models response returned 0 chat models. Load a model in your server or add a model ID below."
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

fn validate_auth_provider_id(id: &str) -> Result<String, String> {
    let trimmed = id.trim();
    if let Some((base, suffix)) = trimmed.split_once(crate::accounts::SEPARATOR) {
        if crate::accounts::valid_segment(base) && crate::accounts::valid_segment(suffix) {
            return Ok(trimmed.to_owned());
        }
        return Err("Account alias must look like 'provider~2'.".into());
    }
    validate_provider_id(trimmed)
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

    /// Providers the model picker offers, or `None` before disk state is known.
    /// Environment-only keys are deliberately excluded, matching the picker.
    pub(in crate::app) fn configured_providers(&self) -> Option<Vec<String>> {
        if !self.disk_loaded {
            return None;
        }
        let mut providers: Vec<String> = self
            .saved_auth
            .iter()
            .map(|a| a.provider.clone())
            .chain(self.custom_providers.iter().map(|c| c.id.clone()))
            .collect();
        providers.sort();
        providers.dedup();
        Some(providers)
    }

    /// Cheap metadata check (two stats, at most every 2s). Returns true when
    /// Pi's provider files changed, e.g. a provider added here, by the Pi CLI
    /// or by hand, so every chat can refresh without restarting Pui.
    pub(in crate::app) fn config_changed(&mut self, now: std::time::Instant) -> bool {
        if self
            .config_checked
            .is_some_and(|at| now.saturating_duration_since(at) < std::time::Duration::from_secs(2))
        {
            return false;
        }
        self.config_checked = Some(now);
        self.stamp_changed([
            auth_json_path(),
            models_json_path(),
            settings_json_path(),
            fallbacks_json_path(),
        ])
    }

    fn stamp_changed(&mut self, paths: impl IntoIterator<Item = Option<PathBuf>>) -> bool {
        let stamp: Vec<_> = paths
            .into_iter()
            .map(|path| {
                let meta = std::fs::metadata(path?).ok()?;
                Some((meta.modified().ok()?, meta.len()))
            })
            .collect();
        let previous = self.config_stamp.replace(stamp.clone());
        previous.is_some_and(|previous| previous != stamp)
    }

    pub(in crate::app) fn is_provider_configured(&self, provider: &str) -> bool {
        if !self.disk_loaded {
            return true;
        }
        self.saved_auth.iter().any(|a| a.provider == provider)
            || self.custom_providers.iter().any(|c| c.id == provider)
    }

    #[cfg(test)]
    pub(in crate::app) fn set_configured_providers_for_test(
        &mut self,
        auth_ids: &[&str],
        custom_ids: &[&str],
    ) {
        self.loaded = true;
        self.disk_loaded = true;
        self.saved_auth = auth_ids
            .iter()
            .map(|id| AuthSummary {
                provider: (*id).to_owned(),
                kind: "oauth".into(),
                detail: None,
            })
            .collect();
        self.custom_providers = custom_ids
            .iter()
            .map(|id| CustomProviderSummary {
                id: (*id).to_owned(),
                name: (*id).to_owned(),
                base_url: "http://localhost:11434/v1".into(),
                api: "openai-completions".into(),
                api_key: String::new(),
                models: Vec::new(),
            })
            .collect();
    }

    pub(in crate::app) fn reload_from_disk(&mut self) {
        self.disk_loaded = true;
        self.accounts.registry = None;
        self.custom_providers.clear();
        self.saved_auth.clear();
        self.model_overrides.clear();
        self.model_thinking_levels.clear();
        self.model_fallbacks.clear();
        if let Some(auth_path) = auth_json_path() {
            let root = read_json_object(&auth_path);
            if let Some(obj) = root.as_object() {
                for (provider, entry) in obj {
                    let kind = entry["type"].as_str().unwrap_or("api_key").to_owned();
                    let detail = if let Some(email) = entry["email"].as_str() {
                        Some(email.to_owned())
                    } else if let Some(project) = entry["env"]["GOOGLE_CLOUD_PROJECT"].as_str() {
                        let loc = entry["env"]["GOOGLE_CLOUD_LOCATION"]
                            .as_str()
                            .unwrap_or("global");
                        Some(format!("ADC: {project} ({loc})"))
                    } else {
                        None
                    };
                    self.saved_auth.push(AuthSummary {
                        provider: provider.clone(),
                        kind,
                        detail,
                    });
                }
            }
        }
        if let Some(models_path) = models_json_path() {
            let root = read_json_object(&models_path);
            self.model_overrides = super::model_editor::read_model_overrides(&root);
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
                    if !base_url.is_empty() || !models.is_empty() {
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
        if let Some(settings_path) = settings_json_path() {
            let root = read_json_object(&settings_path);
            self.model_thinking_levels = super::model_editor::read_model_thinking_levels(&root);
        }
        if let Some(fallbacks_path) = fallbacks_json_path() {
            self.model_fallbacks = super::model_editor::read_model_fallbacks(&fallbacks_path);
        }
    }

    pub(in crate::app) fn has_model_customizations(&self, provider: &str, model_id: &str) -> bool {
        let key = (provider.to_owned(), model_id.to_owned());
        self.model_overrides.contains_key(&key)
            || self.model_thinking_levels.contains_key(&key)
            || self.model_fallbacks.contains_key(&key)
    }

    pub(in crate::app) fn apply_model_edits(
        &mut self,
        patch: &super::model_editor::ModelEditPatch,
    ) -> Result<(), String> {
        use super::model_editor::{
            FieldEdit, apply_override_patch_to_json, apply_thinking_level_to_settings,
            serialize_model_fallbacks, validate_fallback_target,
        };
        if patch.targets.is_empty() {
            return Ok(());
        }
        if let FieldEdit::Set(target) = &patch.fallback {
            let edited: std::collections::BTreeSet<String> = patch
                .targets
                .iter()
                .map(|(p, id)| format!("{p}/{id}"))
                .collect();
            validate_fallback_target(&edited, target, &self.model_fallbacks)?;
        }

        // 1. models.json overrides
        let models_path = models_json_path().ok_or("Cannot resolve ~/.pi/agent/models.json.")?;
        let mut models_root = read_json_object(&models_path);
        let mut models_dirty = false;
        for (provider, model_id) in &patch.targets {
            if apply_override_patch_to_json(&mut models_root, provider, model_id, patch)? {
                models_dirty = true;
            }
        }
        if models_dirty {
            write_json_atomic(&models_path, &models_root, false)?;
        }

        // 2. settings.json per-model thinking level
        if !matches!(patch.thinking_level, FieldEdit::Keep) {
            let settings_path =
                settings_json_path().ok_or("Cannot resolve ~/.pi/agent/settings.json.")?;
            let mut settings_root = read_json_object(&settings_path);
            let mut settings_dirty = false;
            for (provider, model_id) in &patch.targets {
                if apply_thinking_level_to_settings(
                    &mut settings_root,
                    provider,
                    model_id,
                    &patch.thinking_level,
                ) {
                    settings_dirty = true;
                }
            }
            if settings_dirty {
                write_json_atomic(&settings_path, &settings_root, false)?;
            }
        }

        // 3. pui-model-fallbacks.json
        if !matches!(patch.fallback, FieldEdit::Keep) {
            let fallbacks_path = fallbacks_json_path()
                .ok_or("Cannot resolve ~/.pi/agent/pui-model-fallbacks.json.")?;
            let mut map = super::model_editor::read_model_fallbacks(&fallbacks_path);
            for (provider, model_id) in &patch.targets {
                let key = (provider.clone(), model_id.clone());
                match &patch.fallback {
                    FieldEdit::Set(target) => {
                        map.insert(key, target.clone());
                    }
                    FieldEdit::Clear => {
                        map.remove(&key);
                    }
                    FieldEdit::Keep => {}
                }
            }
            write_json_atomic(&fallbacks_path, &serialize_model_fallbacks(&map), false)?;
        }

        self.reload_from_disk();
        self.status_error = None;
        self.reload_requested = true;
        Ok(())
    }

    pub(in crate::app) fn reset_model_edits(
        &mut self,
        targets: &[(String, String)],
    ) -> Result<(), String> {
        use super::model_editor::{
            FieldEdit, apply_thinking_level_to_settings, clear_override_in_json,
            serialize_model_fallbacks,
        };
        if targets.is_empty() {
            return Ok(());
        }
        if let Some(models_path) = models_json_path() {
            let mut models_root = read_json_object(&models_path);
            let mut dirty = false;
            for (provider, model_id) in targets {
                dirty |= clear_override_in_json(&mut models_root, provider, model_id);
            }
            if dirty {
                write_json_atomic(&models_path, &models_root, false)?;
            }
        }
        if let Some(settings_path) = settings_json_path() {
            let mut settings_root = read_json_object(&settings_path);
            let mut dirty = false;
            for (provider, model_id) in targets {
                dirty |= apply_thinking_level_to_settings(
                    &mut settings_root,
                    provider,
                    model_id,
                    &FieldEdit::Clear,
                );
            }
            if dirty {
                write_json_atomic(&settings_path, &settings_root, false)?;
            }
        }
        if let Some(fallbacks_path) = fallbacks_json_path() {
            let mut map = super::model_editor::read_model_fallbacks(&fallbacks_path);
            let mut dirty = false;
            for key in targets {
                dirty |= map.remove(key).is_some();
            }
            if dirty {
                write_json_atomic(&fallbacks_path, &serialize_model_fallbacks(&map), false)?;
            }
        }
        self.reload_from_disk();
        self.status_error = None;
        self.reload_requested = true;
        Ok(())
    }

    pub(in crate::app) fn poll(&mut self) {
        self.poll_accounts();
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
            .antigravity
            .rx
            .as_ref()
            .and_then(|rx| rx.try_recv().ok())
        {
            self.antigravity.rx = None;
            self.antigravity.exchanging = false;
            self.antigravity.listening = false;
            match result {
                Ok(email) if crate::accounts::is_alias(&self.antigravity.target) => {
                    self.antigravity.open = false;
                    self.antigravity.callback_input.clear();
                    self.antigravity.error = None;
                    let target = self.antigravity.target.clone();
                    self.account_signed_in(&target, Some(email.as_str()).filter(|e| !e.is_empty()));
                }
                Ok(email) => {
                    self.antigravity.open = false;
                    self.antigravity.callback_input.clear();
                    self.antigravity.error = None;
                    self.reload_from_disk();
                    self.status_error = None;
                    let suffix = if email.is_empty() {
                        String::new()
                    } else {
                        format!(" ({email})")
                    };
                    self.status_notice = Some(format!(
                        "Google Antigravity subscription connected{suffix}! Reloading model catalog…"
                    ));
                    self.reload_requested = true;
                }
                Err(error) => {
                    self.antigravity.error = Some(error);
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

    pub(in crate::app) fn open_antigravity_for(&mut self, target: &str) {
        let (verifier, challenge) = generate_pkce();
        let state = generate_pkce().0;
        let mut client_id = std::env::var("ANTIGRAVITY_CLIENT_ID").unwrap_or_default();
        let mut client_secret = std::env::var("ANTIGRAVITY_CLIENT_SECRET").unwrap_or_default();
        // A previous custom-client sign-in must refresh with that client, so keep it selected.
        if let Some(path) = auth_json_path() {
            let root = read_json_object(&path);
            if client_id.trim().is_empty() {
                if let Some(cid) = root[target]["clientId"].as_str() {
                    client_id = cid.to_owned();
                }
            }
            if client_secret.trim().is_empty() {
                if let Some(csec) = root[target]["clientSecret"].as_str() {
                    client_secret = csec.to_owned();
                }
            }
        }
        if client_id.trim() == ANTIGRAVITY_DEFAULT_CLIENT_ID {
            client_id.clear();
            client_secret.clear();
        }
        self.antigravity.cancel();
        self.antigravity = AntigravitySubscriptionFlow {
            open: true,
            target: target.to_owned(),
            advanced: !client_id.trim().is_empty(),
            client_id,
            client_secret,
            verifier,
            challenge,
            state,
            callback_input: String::new(),
            exchanging: false,
            listening: false,
            rx: None,
            cancelled: Arc::new(AtomicBool::new(false)),
            child: Arc::new(Mutex::new(None)),
            error: None,
        };
    }

    pub(in crate::app) fn open_vertex_modal(&mut self) {
        let mut modal = VertexModal {
            open: true,
            ..VertexModal::default()
        };
        if let Some(path) = auth_json_path() {
            let root = read_json_object(&path);
            let entry = &root["google-vertex"];
            if entry.is_object() {
                if let Some(key) = entry["key"].as_str() {
                    modal.mode = 1;
                    modal.api_key = key.to_owned();
                } else if let Some(env) = entry["env"].as_object() {
                    if let Some(p) = env.get("GOOGLE_CLOUD_PROJECT").and_then(Value::as_str) {
                        modal.project_id = p.to_owned();
                    }
                    if let Some(l) = env.get("GOOGLE_CLOUD_LOCATION").and_then(Value::as_str) {
                        modal.location = l.to_owned();
                    }
                    if let Some(cred) = env
                        .get("GOOGLE_APPLICATION_CREDENTIALS")
                        .and_then(Value::as_str)
                    {
                        modal.mode = 2;
                        modal.credentials_path = cred.to_owned();
                    } else {
                        modal.mode = 0;
                    }
                }
            }
        }
        self.vertex = modal;
    }

    pub(in crate::app) fn open_add_endpoint(&mut self, preset: Option<(&str, &str, &str, usize)>) {
        let (id, name, base_url, api_index) = preset.unwrap_or((
            "local-openai",
            "Local OpenAI",
            "http://localhost:11434/v1",
            0,
        ));
        self.endpoint = EndpointModal {
            open: true,
            editing_id: None,
            id: id.into(),
            name: name.into(),
            base_url: base_url.into(),
            api_index,
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
        let builtin =
            provider_id.and_then(|id| BUILTIN_KEY_PROVIDERS.iter().position(|(k, _)| *k == id));
        let custom_provider = match (provider_id, builtin) {
            (Some(id), None) if !id.trim().is_empty() => id.trim().to_owned(),
            _ => String::new(),
        };
        self.api_key_modal = ApiKeyModal {
            open: true,
            additional: false,
            provider_index: builtin.unwrap_or(0),
            custom_provider,
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

    fn save_vertex_to_disk(&mut self) -> Result<(), String> {
        let path = auth_json_path().ok_or("Cannot resolve ~/.pi/agent/auth.json.")?;
        let mut root = read_json_object(&path);
        if self.vertex.mode == 1 {
            let key = self.vertex.api_key.trim();
            if key.is_empty() {
                return Err("Enter a Google Cloud Vertex AI API key.".into());
            }
            root["google-vertex"] = json!({
                "type": "api_key",
                "key": key,
            });
        } else {
            let project = self.vertex.project_id.trim();
            let location = self.vertex.location.trim();
            if project.is_empty() {
                return Err("Enter your Google Cloud Project ID.".into());
            }
            if location.is_empty() {
                return Err(
                    "Enter a Google Cloud Location (for example us-central1 or global).".into(),
                );
            }
            let mut env = json!({
                "GOOGLE_CLOUD_PROJECT": project,
                "GOOGLE_CLOUD_LOCATION": location,
            });
            if self.vertex.mode == 2 {
                let cred_path = self.vertex.credentials_path.trim();
                if cred_path.is_empty() {
                    return Err("Enter the path to your service account JSON file.".into());
                }
                env["GOOGLE_APPLICATION_CREDENTIALS"] = json!(cred_path);
            }
            root["google-vertex"] = json!({
                "type": "api_key",
                "env": env,
            });
        }
        write_json_atomic(&path, &root, true)?;
        self.vertex.open = false;
        self.vertex.api_key.clear();
        self.reload_from_disk();
        self.status_error = None;
        self.status_notice =
            Some("Saved Google Vertex AI configuration to ~/.pi/agent/auth.json.".into());
        self.reload_requested = true;
        Ok(())
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
        let existing_overrides = root["providers"][&id]
            .get("modelOverrides")
            .cloned()
            .filter(|v| v.is_object());
        let mut provider_entry = json!({
            "name": name,
            "baseUrl": base_url,
            "api": api,
            "apiKey": api_key,
            "models": models_json,
        });
        if let Some(overrides) = existing_overrides {
            provider_entry["modelOverrides"] = overrides;
        }
        root["providers"][&id] = provider_entry;
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
            validate_auth_provider_id(&self.api_key_modal.custom_provider)?
        } else {
            BUILTIN_KEY_PROVIDERS
                .get(self.api_key_modal.provider_index)
                .map(|(id, _)| (*id).to_owned())
                .unwrap_or_else(|| "google".into())
        };
        let key = self.api_key_modal.api_key.trim().to_owned();
        if key.is_empty() {
            return Err("Enter an API key (or a !command secret helper).".into());
        }
        let path = auth_json_path().ok_or("Cannot resolve ~/.pi/agent/auth.json.")?;
        let mut root = read_json_object(&path);
        let additional = self.api_key_modal.additional
            && !crate::accounts::is_alias(&provider)
            && root[&provider].is_object();
        let provider = if additional {
            self.account_target(&provider)
        } else {
            provider
        };
        root[&provider] = json!({
            "type": "api_key",
            "key": key,
        });
        write_json_atomic(&path, &root, true)?;
        if additional {
            self.registry().add(&provider, None)?;
            self.registry().save()?;
        }
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
        self.show_antigravity_modal(ctx);
        self.show_vertex_modal(ctx);
        self.show_endpoint_modal(ctx);
        self.show_api_key_modal(ctx);
        self.show_account_modals(ctx);
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

    fn show_antigravity_modal(&mut self, ctx: &egui::Context) {
        if !self.antigravity.open {
            return;
        }
        let mut start_browser_flow = false;
        let mut submit_manual = false;
        let response = prompt_ui::modal(
            ctx,
            egui::Id::new("pui-antigravity-subscription-modal"),
            480.0,
            |ui| {
                let mut close = prompt_ui::close(ui);
                prompt_ui::badge(ui, Icon::Spark, false);
                prompt_ui::heading(
                    ui,
                    "Connect Google Antigravity",
                    "Sign in with your Google account to use Antigravity models (Gemini, Claude, GPT-OSS) with Pi's tools and approvals.",
                );
                ui.add_space(14.0);
                let custom = !self.antigravity.client_id.trim().is_empty();
                if prompt_ui::button(
                    ui,
                    if self.antigravity.listening {
                        "Waiting for Google sign-in…"
                    } else if self.antigravity.exchanging {
                        "Connecting…"
                    } else {
                        "Sign in with Google"
                    },
                    prompt_ui::Button::Primary,
                    !self.antigravity.exchanging,
                )
                .clicked()
                {
                    start_browser_flow = true;
                }
                ui.add_space(4.0);
                ui.add(
                    egui::Label::new(theme::small(
                        ui,
                        if custom {
                            "Opens your browser using your custom OAuth client. Pui finishes sign-in on localhost:51121."
                        } else {
                            "Opens your browser with the same Google sign-in the Antigravity app and agy CLI use. Pui finishes sign-in on localhost:51121."
                        },
                    ))
                    .wrap(),
                );
                if let Some(error) = &self.antigravity.error {
                    ui.add_space(6.0);
                    ui.add(
                        egui::Label::new(
                            RichText::new(error)
                                .size(11.5)
                                .color(theme::palette(ui.ctx()).red),
                        )
                        .wrap(),
                    );
                }
                ui.add_space(10.0);
                prompt_ui::note(
                    ui,
                    "Unofficial: Pui signs in as the Antigravity app. Google may change or restrict this access; usage counts against your Antigravity quota.",
                );
                ui.add_space(6.0);
                if prompt_ui::text_link(
                    ui,
                    if self.antigravity.advanced {
                        "Hide advanced options"
                    } else {
                        "Advanced: own OAuth client or paste callback URL"
                    },
                    Icon::Settings,
                    true,
                )
                .clicked()
                {
                    self.antigravity.advanced = !self.antigravity.advanced;
                }
                if self.antigravity.advanced {
                    ui.add_space(4.0);
                    // Stacked fields: egui justifies wrapped labels inside ui.columns.
                    ui.add(egui::Label::new(theme::small(ui, "OAuth client ID · optional")).wrap());
                    ui.add_enabled(
                        !self.antigravity.exchanging,
                        theme::text_field(
                            &mut self.antigravity.client_id,
                            "Built-in Antigravity client",
                        )
                        .id_salt("antigravity-client-id-input")
                        .desired_width(ui.available_width()),
                    );
                    ui.add(egui::Label::new(theme::small(ui, "Client secret · optional")).wrap());
                    ui.add_enabled(
                        !self.antigravity.exchanging && custom,
                        theme::text_field(&mut self.antigravity.client_secret, "Optional")
                            .id_salt("antigravity-client-secret-input")
                            .password(true)
                            .desired_width(ui.available_width()),
                    );
                    ui.add(
                        egui::Label::new(theme::small(
                            ui,
                            "Signing in on another device? Paste the final localhost:51121/oauth-callback URL:",
                        ))
                        .wrap(),
                    );
                    let edit = ui.add_enabled(
                        !self.antigravity.exchanging || self.antigravity.listening,
                        theme::text_field(
                            &mut self.antigravity.callback_input,
                            "http://localhost:51121/oauth-callback?state=…&code=…",
                        )
                        .id_salt("antigravity-oauth-callback-input")
                        .desired_width(ui.available_width()),
                    );
                    if edit.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                        submit_manual = true;
                    }
                }
                ui.add_space(16.0);
                prompt_ui::footer(ui, |ui| {
                    close |= prompt_ui::button(ui, "Cancel", prompt_ui::Button::Secondary, true)
                        .clicked();
                    if self.antigravity.advanced
                        && prompt_ui::button(
                            ui,
                            "Connect pasted URL",
                            prompt_ui::Button::Secondary,
                            !self.antigravity.callback_input.trim().is_empty()
                                && (!self.antigravity.exchanging || self.antigravity.listening),
                        )
                        .clicked()
                    {
                        submit_manual = true;
                    }
                });
                close
            },
        );
        if start_browser_flow && !self.antigravity.exchanging {
            let (client_id, client_secret) =
                antigravity_client(&self.antigravity.client_id, &self.antigravity.client_secret);
            {
                self.antigravity.cancel();
                self.antigravity.cancelled = Arc::new(AtomicBool::new(false));
                self.antigravity.child = Arc::new(Mutex::new(None));
                self.antigravity.exchanging = true;
                self.antigravity.listening = true;
                self.antigravity.error = None;
                let auth_url = build_antigravity_authorize_url(
                    &client_id,
                    &self.antigravity.challenge,
                    &self.antigravity.state,
                );
                let state = self.antigravity.state.clone();
                let verifier = self.antigravity.verifier.clone();
                let target = self.antigravity.target.clone();
                let cancelled = self.antigravity.cancelled.clone();
                let child = self.antigravity.child.clone();
                let (tx, rx) = mpsc::channel();
                self.antigravity.rx = Some(rx);
                let ctx = ctx.clone();
                std::thread::spawn(move || {
                    let _ = tx.send(run_antigravity_browser_login(
                        (&client_id, &client_secret),
                        &auth_url,
                        &state,
                        &verifier,
                        &target,
                        &cancelled,
                        &child,
                    ));
                    ctx.request_repaint();
                });
            }
        }
        if submit_manual {
            match parse_oauth_code_input(&self.antigravity.callback_input, &self.antigravity.state)
            {
                Ok((code, _)) => {
                    let client_id = self.antigravity.client_id.trim().to_owned();
                    let client_secret = self.antigravity.client_secret.trim().to_owned();
                    self.antigravity.cancel();
                    self.antigravity.cancelled = Arc::new(AtomicBool::new(false));
                    self.antigravity.exchanging = true;
                    self.antigravity.listening = false;
                    self.antigravity.error = None;
                    let verifier = self.antigravity.verifier.clone();
                    let target = self.antigravity.target.clone();
                    let cancelled = self.antigravity.cancelled.clone();
                    let (tx, rx) = mpsc::channel();
                    self.antigravity.rx = Some(rx);
                    let ctx = ctx.clone();
                    std::thread::spawn(move || {
                        let _ = tx.send(exchange_antigravity_oauth_code(
                            &client_id,
                            &client_secret,
                            &code,
                            &verifier,
                            &target,
                            &cancelled,
                        ));
                        ctx.request_repaint();
                    });
                }
                Err(err) => {
                    self.antigravity.error = Some(err);
                }
            }
        }
        if response.should_close() || response.inner {
            self.antigravity.open = false;
            self.antigravity.cancel();
        }
    }

    fn show_vertex_modal(&mut self, ctx: &egui::Context) {
        if !self.vertex.open {
            return;
        }
        let mut save = false;
        let response = prompt_ui::modal(
            ctx,
            egui::Id::new("pui-google-vertex-modal"),
            450.0,
            |ui| {
                let mut close = prompt_ui::close(ui);
                prompt_ui::badge(ui, Icon::Globe, false);
                prompt_ui::heading(
                    ui,
                    "Configure Google Vertex AI",
                    "Connect Google Vertex AI using Application Default Credentials (gcloud auth application-default login), a Google Cloud API key, or a service account file.",
                );
                ui.add_space(12.0);
                ui.horizontal_wrapped(|ui| {
                    for (idx, label) in [
                        (0, "Application Default Credentials (ADC)"),
                        (1, "Google Cloud API Key"),
                        (2, "Service Account JSON"),
                    ] {
                        if prompt_ui::button(
                            ui,
                            label,
                            if self.vertex.mode == idx {
                                prompt_ui::Button::Primary
                            } else {
                                prompt_ui::Button::Secondary
                            },
                            true,
                        )
                        .clicked()
                        {
                            self.vertex.mode = idx;
                            self.vertex.error = None;
                        }
                    }
                });
                ui.add_space(10.0);
                if self.vertex.mode == 1 {
                    ui.label(theme::small(
                        ui,
                        "Google Cloud API Key (GOOGLE_CLOUD_API_KEY):",
                    ));
                    let is_cmd = self.vertex.api_key.trim_start().starts_with('!');
                    ui.add(
                        theme::text_field(&mut self.vertex.api_key, "AIza… or !command")
                            .id_salt("vertex-api-key-input")
                            .password(!is_cmd)
                            .desired_width(ui.available_width()),
                    );
                } else {
                    if self.vertex.mode == 0 {
                        prompt_ui::note(
                            ui,
                            "Uses ~/.config/gcloud/application_default_credentials.json from `gcloud auth application-default login`.",
                        );
                        ui.add_space(8.0);
                    }
                    ui.columns(2, |cols| {
                        field_label(
                            &mut cols[0],
                            "Google Cloud Project ID (GOOGLE_CLOUD_PROJECT)",
                        );
                        cols[0].add(
                            theme::text_field(&mut self.vertex.project_id, "my-gcp-project")
                                .id_salt("vertex-project-id")
                                .desired_width(cols[0].available_width()),
                        );
                        field_label(&mut cols[1], "Location (GOOGLE_CLOUD_LOCATION)");
                        cols[1].add(
                            theme::text_field(&mut self.vertex.location, "us-central1 or global")
                                .id_salt("vertex-location")
                                .desired_width(cols[1].available_width()),
                        );
                    });
                    if self.vertex.mode == 2 {
                        ui.add_space(8.0);
                        ui.label(theme::small(
                            ui,
                            "Service Account JSON Path (GOOGLE_APPLICATION_CREDENTIALS):",
                        ));
                        ui.add(
                            theme::text_field(
                                &mut self.vertex.credentials_path,
                                "/path/to/service-account.json",
                            )
                            .id_salt("vertex-sa-path")
                            .desired_width(ui.available_width()),
                        );
                    }
                }
                if let Some(error) = &self.vertex.error {
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
                    if prompt_ui::button(ui, "Save Vertex AI", prompt_ui::Button::Primary, true)
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
            if let Err(err) = self.save_vertex_to_disk() {
                self.vertex.error = Some(err);
            }
        }
        if response.should_close() || response.inner {
            self.vertex.open = false;
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
            530.0,
            |ui| {
                let mut close = prompt_ui::close(ui);
                prompt_ui::badge(ui, Icon::Globe, false);
                prompt_ui::heading(
                    ui,
                    if self.endpoint.editing_id.is_some() {
                        "Edit Compatible Endpoint"
                    } else {
                        "Add Compatible Endpoint"
                    },
                    "Connect a local or custom OpenAI, Anthropic, or Google Gemini/Vertex-compatible server and automatically discover its models.",
                );
                ui.add_space(10.0);
                ui.horizontal_wrapped(|ui| {
                    ui.label(theme::small(ui, "Quick presets:"));
                    for (label, pid, pname, url, api_idx) in [
                        (
                            "Ollama (:11434)",
                            "ollama",
                            "Ollama",
                            "http://localhost:11434/v1",
                            0,
                        ),
                        (
                            "LM Studio (:1234)",
                            "lmstudio",
                            "LM Studio",
                            "http://localhost:1234/v1",
                            0,
                        ),
                        (
                            "vLLM / Local (:8000)",
                            "local-openai",
                            "Local OpenAI",
                            "http://localhost:8000/v1",
                            0,
                        ),
                        (
                            "llama.cpp (:8080)",
                            "llamacpp",
                            "llama.cpp",
                            "http://localhost:8080/v1",
                            0,
                        ),
                        (
                            "Google Gemini",
                            "google-custom",
                            "Google Gemini Custom",
                            "https://generativelanguage.googleapis.com/v1beta",
                            3,
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
                            self.endpoint.api_index = api_idx;
                        }
                    }
                });
                ui.add_space(8.0);
                ui.columns(2, |cols| {
                    field_label(&mut cols[0], "Provider ID");
                    cols[0].add_enabled(
                        self.endpoint.editing_id.is_none(),
                        theme::text_field(&mut self.endpoint.id, "local-openai")
                            .id_salt("endpoint-provider-id")
                            .desired_width(cols[0].available_width()),
                    );
                    field_label(&mut cols[1], "Display Name");
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
                    field_label(&mut cols[0], "API Protocol");
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
                    field_label(&mut cols[1], "API Key (optional · or $ENV_VAR)");
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
                            "Or type a model ID manually (e.g. qwen2.5-coder:32b or gemini-2.5-pro)…",
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
                                "Click Discover Models to fetch models from /models, or add a model ID above.",
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
                            || lower.contains("thinking")
                            || lower.contains("gemini"),
                        vision: lower.contains("vl")
                            || lower.contains("vision")
                            || lower.contains("gemini"),
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
                let selected = if self.api_key_modal.custom_provider.trim().is_empty() {
                    BUILTIN_KEY_PROVIDERS[self.api_key_modal.provider_index]
                        .0
                        .to_owned()
                } else {
                    self.api_key_modal.custom_provider.trim().to_owned()
                };
                if !crate::accounts::is_alias(&selected)
                    && self.saved_auth.iter().any(|a| a.provider == selected)
                {
                    ui.add_space(6.0);
                    prompt_ui::compact_toggle_row(
                        ui,
                        &mut self.api_key_modal.additional,
                        "Add as another account (keep the existing key)",
                    );
                }
                ui.add_space(8.0);
                ui.label(theme::small(ui, "API Key (or !command)"));
                let is_command = self.api_key_modal.api_key.trim_start().starts_with('!');
                let edit = ui.add(
                    theme::text_field(&mut self.api_key_modal.api_key, "AIza… / sk-…")
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
        self.claude.open
            || self.antigravity.open
            || self.vertex.open
            || self.endpoint.open
            || self.api_key_modal.open
            || self.account_modal_open()
    }

    pub(in crate::app) fn escape(&mut self) -> bool {
        if self.claude.open && !self.claude.exchanging {
            self.claude.open = false;
            return true;
        }
        if self.antigravity.open {
            self.antigravity.open = false;
            self.antigravity.cancel();
            return true;
        }
        if self.vertex.open {
            self.vertex.open = false;
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
        self.close_account_modal()
    }
}

impl Pui {
    /// Reload Pi's model registry in every live chat. Extension commands run
    /// immediately even mid-run (an in-flight response keeps its model), so a
    /// busy chat no longer keeps a stale catalog.
    pub(in crate::app) fn refresh_providers_catalog(&mut self) {
        let commands = [
            json!({"type": "prompt", "message": "/pui-reload-models"}),
            protocol::command("get_available_models"),
            protocol::command("get_state"),
            protocol::command("get_available_thinking_levels"),
        ];
        if self.client.is_some() {
            for command in &commands {
                self.send(command.clone());
            }
        }
        if self.demo {
            return;
        }
        for chat in self.agents.parked.values_mut() {
            if let Some(client) = chat.client.as_mut() {
                for command in &commands {
                    let _ = client.send(command.clone());
                }
            }
        }
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

        // 1. Subscriptions & Cloud Accounts
        page::section(
            ui,
            "Add or connect provider",
            "Connect your Google Antigravity or Claude Code subscription, configure Google Gemini / Vertex AI, or add a compatible endpoint with automatic model discovery.",
        );
        page::card(ui, |ui| {
            page::row(
                ui,
                "Sign in with subscription",
                "Claude Pro/Max, ChatGPT Plus/Pro, or Google Antigravity via OAuth.",
                |ui| {
                    if ui.button("Antigravity…").clicked() {
                        self.settings_view
                            .providers
                            .add_account(ui.ctx(), "antigravity");
                    }
                    if ui.button("ChatGPT…").clicked() {
                        self.settings_view
                            .providers
                            .add_account(ui.ctx(), "openai-codex");
                    }
                    if ui.button("Claude…").clicked() {
                        self.settings_view
                            .providers
                            .add_account(ui.ctx(), "anthropic");
                    }
                },
            );
            page::separator(ui);
            page::row(
                ui,
                "API key or Vertex AI",
                "Google Gemini, Anthropic, OpenAI, OpenRouter, DeepSeek, xAI, or Google Cloud Vertex AI.",
                |ui| {
                    if ui.button("Vertex AI…").clicked() {
                        self.settings_view.providers.open_vertex_modal();
                    }
                    if ui.button("API Key…").clicked() {
                        self.settings_view.providers.open_api_key_modal(None);
                    }
                },
            );
            page::separator(ui);
            page::row(
                ui,
                "Local or custom endpoint",
                "Ollama, LM Studio, vLLM, llama.cpp, or compatible API URLs with auto model discovery.",
                |ui| {
                    if ui.button("Add Endpoint…").clicked() {
                        self.settings_view.providers.open_add_endpoint(None);
                    }
                },
            );
        });

        // Unified Accounts card (subscriptions, API keys, and custom endpoints).
        self.settings_accounts(ui);

        // Reload / CLI fallback section
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
                "Claude, ChatGPT and Antigravity sign in under Accounts. For other OAuth providers (GitHub Copilot, Radius), run Pi's interactive /login in your terminal.",
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
    fn parses_google_v1beta_models_payload_and_filters_embeddings() {
        let payload = json!({
            "models": [
                {
                    "name": "models/gemini-2.5-pro",
                    "displayName": "Gemini 2.5 Pro",
                    "inputTokenLimit": 1048576,
                    "outputTokenLimit": 65536,
                    "supportedGenerationMethods": ["generateContent", "countTokens"]
                },
                {
                    "name": "models/text-embedding-004",
                    "displayName": "Text Embedding 004",
                    "supportedGenerationMethods": ["embedContent"]
                }
            ]
        });
        let models = parse_discovered_models(&payload);
        assert_eq!(models.len(), 1);
        assert_eq!(models[0].id, "gemini-2.5-pro");
        assert_eq!(models[0].name, "Gemini 2.5 Pro");
        assert_eq!(models[0].context_window, 1_048_576);
        assert_eq!(models[0].max_tokens, 65_536);
        assert!(models[0].reasoning);
        assert!(models[0].vision);
    }

    #[test]
    fn parses_claude_and_antigravity_oauth_code_and_verifies_state() {
        let (code, state) = parse_oauth_code_input("abc123code#state999", "state999").unwrap();
        assert_eq!(code, "abc123code");
        assert_eq!(state, "state999");

        let url = "https://platform.claude.com/oauth/code/callback?code=urlcode&state=state999";
        let (code2, state2) = parse_oauth_code_input(url, "state999").unwrap();
        assert_eq!(code2, "urlcode");
        assert_eq!(state2, "state999");

        let agy_url = "http://localhost:51121/oauth-callback?state=agystate&code=4/0AeanS0123";
        let (code3, state3) = parse_oauth_code_input(agy_url, "agystate").unwrap();
        assert_eq!(code3, "4/0AeanS0123");
        assert_eq!(state3, "agystate");

        assert!(parse_oauth_code_input("abc#wrongstate", "state999").is_err());
        assert!(parse_oauth_code_input("   ", "state999").is_err());
    }

    #[test]
    fn antigravity_sign_in_defaults_to_built_in_client_and_keeps_custom_override() {
        let (id, secret) = antigravity_client("  ", "ignored");
        assert_eq!(id, ANTIGRAVITY_DEFAULT_CLIENT_ID);
        assert_eq!(secret, ANTIGRAVITY_DEFAULT_CLIENT_SECRET);
        let (id, secret) = antigravity_client(" mine.apps.googleusercontent.com ", " s ");
        assert_eq!(
            (id.as_str(), secret.as_str()),
            ("mine.apps.googleusercontent.com", "s")
        );
        let url = build_antigravity_authorize_url(&antigravity_client("", "").0, "c", "st");
        let url = url::Url::parse(&url).unwrap();
        let query: std::collections::HashMap<_, _> = url.query_pairs().into_owned().collect();
        assert_eq!(query["client_id"], ANTIGRAVITY_DEFAULT_CLIENT_ID);
        assert_eq!(query["redirect_uri"], ANTIGRAVITY_REDIRECT_URI);
        assert_eq!(query["code_challenge"], "c");
        for (name, source) in [
            (
                "antigravity.ts",
                include_str!("../../../scripts/antigravity.ts"),
            ),
            ("usage.mjs", include_str!("../../../scripts/usage.mjs")),
        ] {
            assert!(
                source.contains(ANTIGRAVITY_DEFAULT_CLIENT_ID)
                    && source.contains(ANTIGRAVITY_DEFAULT_CLIENT_SECRET),
                "{name} must refresh with the same built-in client"
            );
        }
        assert!(
            !url.as_str().contains(ANTIGRAVITY_DEFAULT_CLIENT_SECRET),
            "the client secret is only sent to the token endpoint"
        );
    }

    fn antigravity_modal_texts(
        dark: bool,
        width: f32,
        zoom: f32,
        setup: impl Fn(&mut ProvidersManager),
    ) -> (Vec<(String, bool, egui::Rect)>, egui::Rect) {
        let ctx = egui::Context::default();
        crate::theme::install(&ctx);
        ctx.set_visuals(if dark {
            egui::Visuals::dark()
        } else {
            egui::Visuals::light()
        });
        ctx.set_zoom_factor(zoom);
        let mut manager = ProvidersManager::default();
        manager.antigravity.open = true;
        setup(&mut manager);
        let mut texts = Vec::new();
        let mut screen = egui::Rect::NOTHING;
        for _ in 0..3 {
            let output = ctx.run(
                egui::RawInput {
                    screen_rect: Some(egui::Rect::from_min_size(
                        egui::Pos2::ZERO,
                        egui::vec2(width, 900.0),
                    )),
                    ..Default::default()
                },
                |ctx| {
                    screen = ctx.screen_rect();
                    manager.show_antigravity_modal(ctx)
                },
            );
            texts.clear();
            fn collect(shape: &egui::Shape, out: &mut Vec<(String, bool, egui::Rect)>) {
                match shape {
                    egui::Shape::Text(t) => out.push((
                        t.galley.text().to_owned(),
                        t.galley.job.justify,
                        t.galley.rect.translate(t.pos.to_vec2()),
                    )),
                    egui::Shape::Vec(v) => v.iter().for_each(|s| collect(s, out)),
                    _ => {}
                }
            }
            for shape in &output.shapes {
                collect(&shape.shape, &mut texts);
            }
        }
        assert!(
            manager.antigravity.open,
            "rendering alone must not close the dialog"
        );
        (texts, screen)
    }

    #[test]
    fn antigravity_modal_is_one_click_with_readable_labels_in_both_themes() {
        for dark in [false, true] {
            for (width, zoom) in [(360.0, 1.0), (640.0, 1.0), (640.0, 1.5)] {
                for advanced in [false, true] {
                    let (texts, screen) = antigravity_modal_texts(dark, width, zoom, |m| {
                        m.antigravity.advanced = advanced
                    });
                    let has = |needle: &str| texts.iter().any(|(t, ..)| t.contains(needle));
                    assert!(has("Sign in with Google"));
                    assert!(has("Unofficial"));
                    assert!(!has("ANTIGRAVITY_CLIENT_ID"), "no env-var jargon in labels");
                    assert_eq!(has("OAuth client ID"), advanced);
                    assert_eq!(has("Connect pasted URL"), advanced);
                    for (text, justified, rect) in &texts {
                        assert!(!justified, "{text:?} is glyph-justified");
                        assert!(
                            rect.right() <= screen.right() + 1.0,
                            "{text:?} overflows {width}pt at {zoom}x: {rect:?}"
                        );
                    }
                }
            }
        }
        let (texts, _) = antigravity_modal_texts(false, 640.0, 1.0, |m| {
            m.antigravity.exchanging = true;
            m.antigravity.listening = true;
        });
        assert!(
            texts
                .iter()
                .any(|(t, ..)| t == "Waiting for Google sign-in…")
        );
    }

    #[test]
    fn column_field_labels_wrap_without_justified_glyph_spacing() {
        let ctx = egui::Context::default();
        crate::theme::install(&ctx);
        let output = ctx.run(
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(300.0, 400.0),
                )),
                ..Default::default()
            },
            |ctx| {
                egui::CentralPanel::default().show(ctx, |ui| {
                    ui.columns(2, |cols| {
                        field_label(
                            &mut cols[0],
                            "Google Cloud Project ID (GOOGLE_CLOUD_PROJECT)",
                        )
                    });
                });
            },
        );
        let mut found = false;
        for shape in output.shapes {
            if let egui::Shape::Text(t) = shape.shape {
                if t.galley.text().starts_with("Google Cloud") {
                    found = true;
                    assert!(
                        t.galley.rows.len() > 1,
                        "long label wraps in a narrow column"
                    );
                    assert!(!t.galley.job.justify);
                }
            }
        }
        assert!(found);
    }

    #[test]
    fn cancelling_add_provider_modals_without_saving_never_adds_provider() {
        let mut manager = ProvidersManager::default();
        manager.set_configured_providers_for_test(&[], &[]);
        assert!(!manager.is_provider_configured("openai"));
        assert!(!manager.is_provider_configured("google-vertex"));
        assert!(!manager.is_provider_configured("local-test"));
        assert!(!manager.is_provider_configured("antigravity"));

        // Open API key modal, enter draft data, cancel without saving.
        manager.open_api_key_modal(Some("openai"));
        manager.api_key_modal.api_key = "sk-unsaved".into();
        assert!(manager.escape());
        assert!(!manager.api_key_modal.open);
        assert!(manager.saved_auth.is_empty());
        assert!(!manager.is_provider_configured("openai"));

        // Open Vertex modal, enter draft data, cancel without saving.
        manager.open_vertex_modal();
        manager.vertex.project_id = "unsaved-project".into();
        assert!(manager.escape());
        assert!(!manager.vertex.open);
        assert!(manager.saved_auth.is_empty());
        assert!(!manager.is_provider_configured("google-vertex"));

        // Open custom endpoint modal, enter draft data, cancel without saving.
        manager.open_add_endpoint(None);
        manager.endpoint.id = "local-test".into();
        manager.endpoint.base_url = "http://localhost:11434/v1".into();
        assert!(manager.escape());
        assert!(!manager.endpoint.open);
        assert!(manager.custom_providers.is_empty());
        assert!(!manager.is_provider_configured("local-test"));

        // Open Antigravity modal, cancel without completing sign-in: cancellation flag is set so
        // any in-flight exchange refuses to write to auth.json.
        manager.open_antigravity_for("antigravity");
        let cancelled = manager.antigravity.cancelled.clone();
        assert!(!cancelled.load(Ordering::Relaxed));
        assert!(manager.escape());
        assert!(!manager.antigravity.open);
        assert!(cancelled.load(Ordering::Relaxed));
        let exchange_result = exchange_antigravity_oauth_code(
            "",
            "",
            "fake-code",
            "fake-verifier",
            "antigravity",
            &cancelled,
        );
        assert!(exchange_result.is_err());
        assert!(manager.saved_auth.is_empty());
        assert!(!manager.is_provider_configured("antigravity"));
    }
}

#[cfg(test)]
mod config_watch_tests {
    use super::*;
    #[test]
    fn provider_file_changes_are_detected_without_a_restart() {
        let dir = std::env::temp_dir().join(format!("pui-config-watch-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let paths = || {
            [
                Some(dir.join("auth.json")),
                Some(dir.join("models.json")),
                Some(dir.join("settings.json")),
            ]
        };
        let mut manager = ProvidersManager::default();
        assert!(
            !manager.stamp_changed(paths()),
            "first check records a baseline"
        );
        assert!(!manager.stamp_changed(paths()));
        std::fs::write(dir.join("models.json"), r#"{"providers":{"Nvidia":{}}}"#).unwrap();
        assert!(
            manager.stamp_changed(paths()),
            "new provider file is detected"
        );
        assert!(!manager.stamp_changed(paths()));
        std::fs::write(dir.join("auth.json"), r#"{"nvidia":{}}"#).unwrap();
        assert!(manager.stamp_changed(paths()), "new login is detected");
        // The public check is rate limited to one pair of stats per 2 seconds.
        let start = std::time::Instant::now();
        manager.config_changed(start);
        assert!(!manager.config_changed(start + std::time::Duration::from_millis(500)));
        std::fs::remove_dir_all(dir).unwrap();
    }
}
