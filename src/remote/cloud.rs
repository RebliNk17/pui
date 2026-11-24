//! Pure-stdlib client for connecting Pui Desktop to a headless `pui-daemon` cloud host
//! (e.g. Oracle Cloud Always Free VM over Tailscale) for explicit credential sync,
//! dedicated revocable Git SSH key provisioning, and remote repo cloning.
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::{
    collections::BTreeSet,
    io::{Read, Write},
    net::TcpStream,
    path::{Path, PathBuf},
    process::{Command, Stdio},
    time::Duration,
};

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct CloudHostConfig {
    pub name: String,
    pub host: String,
    pub port: u16,
    pub token: String,
    pub tls: bool,
}

pub const PLAYWRIGHT_RESEARCH_MCP: &str = "playwright-research";

#[derive(Clone, Debug, Default, Deserialize)]
#[serde(default)]
pub struct CloudAdminStatus {
    pub has_pi_auth: bool,
    pub pi_auth_providers: Vec<String>,
    pub has_git_ssh_key: bool,
    pub git_public_key: Option<String>,
    pub git_user_name: Option<String>,
    pub git_user_email: Option<String>,
    pub skills: Vec<String>,
    pub mcp_servers: Vec<String>,
    pub service_creds: Vec<String>,
    pub projects: Vec<String>,
    pub workspaces_dir: String,
}

#[derive(Clone, Debug)]
pub struct LocalMcpItem {
    pub name: String,
    pub blocked_on_cloud: bool,
    pub local_only: bool,
    pub reason: Option<String>,
}

#[derive(Clone, Debug)]
pub struct LocalServiceCred {
    pub id: String,
    pub label: String,
    pub scope: String,
    pub path: PathBuf,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum GitAuthChoice {
    #[default]
    DedicatedCloudKey,
    CopyMacKey,
}

#[derive(Clone, Debug)]
pub struct LocalSshKeyInfo {
    pub path: PathBuf,
    pub label: String,
    pub unencrypted: bool,
}

#[derive(Clone, Debug)]
pub struct CloudActionNotice {
    pub title: String,
    pub detail: String,
    pub copy_text: Option<String>,
    pub open_url: Option<String>,
    pub open_label: Option<String>,
}

#[derive(Clone, Debug)]
pub struct CloudSyncReport {
    pub title: String,
    pub summary: String,
    pub action_needed: Option<CloudActionNotice>,
}

#[derive(Clone, Debug, Default)]
pub struct LocalSyncInventory {
    pub providers: Vec<String>,
    pub mcps: Vec<LocalMcpItem>,
    pub skills: Vec<String>,
    pub service_creds: Vec<LocalServiceCred>,
    pub mac_ssh_key: Option<LocalSshKeyInfo>,
    pub git_remote: Option<String>,
    pub project_name: String,
}

#[derive(Clone, Debug, Default)]
pub struct CloudSyncPlan {
    pub providers: BTreeSet<String>,
    pub setup_git_ssh: bool,
    pub git_auth_choice: GitAuthChoice,
    pub add_playwright_research: bool,
    pub mcps: BTreeSet<String>,
    pub skills: BTreeSet<String>,
    pub service_creds: BTreeSet<String>,
    pub clone_current_repo: bool,
}

impl CloudSyncPlan {
    pub fn is_empty(&self) -> bool {
        self.providers.is_empty()
            && !self.setup_git_ssh
            && !self.add_playwright_research
            && self.mcps.is_empty()
            && self.skills.is_empty()
            && self.service_creds.is_empty()
            && !self.clone_current_repo
    }
}

#[derive(Clone, Debug)]
pub struct CloudGitResult {
    pub public_key: String,
    pub registered_on_github: bool,
}

pub fn parse_pairing_uri(input: &str) -> Result<CloudHostConfig, String> {
    let trimmed = input.trim();
    let Some(rest) = trimmed.strip_prefix("pui://pair?") else {
        return Err("Expected a pui://pair?... link from `pui-daemon`.".into());
    };
    let mut host = String::new();
    let mut port: u16 = 4817;
    let mut token = String::new();
    let mut name = "pui-cloud".to_owned();
    let mut tls = false;

    for pair in rest.split('&') {
        let Some((k, v)) = pair.split_once('=') else {
            continue;
        };
        let val = url_decode(v);
        match k {
            "host" => host = val.trim().to_owned(),
            "port" => {
                if let Ok(p) = val.parse::<u16>() {
                    if p > 0 {
                        port = p;
                    }
                }
            }
            "token" => token = val.trim().to_owned(),
            "name" if !val.trim().is_empty() => name = val.trim().to_owned(),
            "tls" => tls = val == "1" || val.eq_ignore_ascii_case("true"),
            _ => {}
        }
    }

    if host.is_empty() {
        return Err("Pairing link is missing `host`.".into());
    }
    if token.is_empty() {
        return Err("Pairing link is missing `token`.".into());
    }
    Ok(CloudHostConfig {
        name,
        host,
        port,
        token,
        tls,
    })
}

/// Reconstructs the `pui://pair?...` URI for this cloud host so Pui Mobile can scan or paste it.
pub fn pairing_uri(cfg: &CloudHostConfig) -> String {
    let encoded_name = percent_encode_component(&cfg.name);
    format!(
        "pui://pair?v=1&host={}&port={}&token={}&name={}&tls={}",
        cfg.host,
        cfg.port,
        cfg.token,
        encoded_name,
        if cfg.tls { 1 } else { 0 }
    )
}

fn percent_encode_component(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for b in s.bytes() {
        if b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.' | b'~') {
            out.push(b as char);
        } else {
            out.push_str(&format!("%{b:02X}"));
        }
    }
    out
}

fn decode_chunked_body(raw: &str) -> String {
    let mut out = String::new();
    let mut rest = raw;
    while let Some((size_line, after_size)) = rest.split_once("\r\n") {
        let hex = size_line.split(';').next().unwrap_or("").trim();
        let Ok(len) = usize::from_str_radix(hex, 16) else {
            break;
        };
        if len == 0 {
            break;
        }
        if after_size.len() < len {
            out.push_str(after_size);
            break;
        }
        out.push_str(&after_size[..len]);
        rest = after_size[len..]
            .strip_prefix("\r\n")
            .unwrap_or(&after_size[len..]);
    }
    out
}

fn http_request_json(
    cfg: &CloudHostConfig,
    method: &str,
    path: &str,
    body: Option<&Value>,
) -> Result<(u16, Value), String> {
    if cfg.tls {
        return Err(
            "Direct HTTPS cloud host sync requires plain HTTP over Tailscale (`tls=0`).".into(),
        );
    }
    let addr = format!("{}:{}", cfg.host, cfg.port);
    let mut stream = TcpStream::connect(&addr)
        .map_err(|e| format!("Could not connect to {addr} over Tailscale: {e}"))?;
    let _ = stream.set_read_timeout(Some(Duration::from_secs(30)));
    let _ = stream.set_write_timeout(Some(Duration::from_secs(15)));

    let payload = body
        .map(|b| serde_json::to_vec(b).unwrap_or_default())
        .unwrap_or_default();

    let mut req = format!(
        "{method} {path} HTTP/1.1\r\nHost: {addr}\r\nAuthorization: Bearer {}\r\nConnection: close\r\n",
        cfg.token
    );
    if body.is_some() {
        req.push_str(&format!(
            "Content-Type: application/json\r\nContent-Length: {}\r\n",
            payload.len()
        ));
    }
    req.push_str("\r\n");

    stream
        .write_all(req.as_bytes())
        .and_then(|_| {
            if !payload.is_empty() {
                stream.write_all(&payload)?;
            }
            stream.flush()
        })
        .map_err(|e| format!("Failed sending request to {addr}: {e}"))?;

    let mut raw = Vec::new();
    stream
        .read_to_end(&mut raw)
        .map_err(|e| format!("Failed reading response from {addr}: {e}"))?;
    let text = String::from_utf8_lossy(&raw);
    let Some((head, body_str)) = text.split_once("\r\n\r\n") else {
        return Err("Malformed HTTP response from cloud host.".into());
    };
    let status = head
        .lines()
        .next()
        .and_then(|l| l.split_whitespace().nth(1))
        .and_then(|code| code.parse::<u16>().ok())
        .unwrap_or(500);
    let is_chunked = head.lines().any(|l| {
        let lower = l.to_ascii_lowercase();
        lower.starts_with("transfer-encoding:") && lower.contains("chunked")
    });
    let decoded_body = if is_chunked {
        decode_chunked_body(body_str)
    } else {
        body_str.to_owned()
    };
    let trimmed = decoded_body.trim();
    let parsed: Value = if trimmed.is_empty() {
        Value::Null
    } else {
        serde_json::from_str(trimmed)
            .map_err(|e| format!("Invalid JSON response from cloud host (HTTP {status}): {e}"))?
    };
    if status >= 400 {
        let msg = parsed["error"]
            .as_str()
            .unwrap_or("Cloud host returned an error.");
        return Err(format!("HTTP {status}: {msg}"));
    }
    Ok((status, parsed))
}

pub fn fetch_status(cfg: &CloudHostConfig) -> Result<CloudAdminStatus, String> {
    let (_, val) = http_request_json(cfg, "GET", "/v1/admin/status", None)?;
    serde_json::from_value(val).map_err(|e| format!("Invalid cloud status payload: {e}"))
}

fn candidate_pi_agent_dirs() -> Vec<PathBuf> {
    let mut dirs_out = Vec::new();
    if let Ok(dir) = std::env::var("PI_CODING_AGENT_DIR") {
        let p = PathBuf::from(dir);
        if p.is_dir() {
            dirs_out.push(p);
        }
    }
    if let Some(data_dir) = dirs::data_local_dir() {
        let managed = data_dir.join("pui").join("pi-agent");
        if managed.is_dir() && !dirs_out.contains(&managed) {
            dirs_out.push(managed);
        }
    }
    if let Some(home) = dirs::home_dir() {
        let legacy = home.join(".pi").join("agent");
        if legacy.is_dir() && !dirs_out.contains(&legacy) {
            dirs_out.push(legacy);
        }
    }
    dirs_out
}

fn local_pi_file(filename: &str) -> Option<PathBuf> {
    for dir in candidate_pi_agent_dirs() {
        let p = dir.join(filename);
        if p.is_file() {
            return Some(p);
        }
    }
    None
}

fn local_skills_dirs() -> Vec<PathBuf> {
    let mut out = Vec::new();
    if let Some(home) = dirs::home_dir() {
        let agents_skills = home.join(".agents").join("skills");
        if agents_skills.is_dir() {
            out.push(agents_skills);
        }
    }
    for dir in candidate_pi_agent_dirs() {
        let s = dir.join("skills");
        if s.is_dir() && !out.contains(&s) {
            out.push(s);
        }
    }
    out
}

fn read_local_mcp_map() -> serde_json::Map<String, Value> {
    let Some(mcp_path) = local_pi_file("mcp.json") else {
        return serde_json::Map::new();
    };
    let Ok(raw) = std::fs::read_to_string(&mcp_path) else {
        return serde_json::Map::new();
    };
    let Ok(val) = serde_json::from_str::<Value>(&raw) else {
        return serde_json::Map::new();
    };
    if let Some(obj) = val.get("mcpServers").and_then(Value::as_object) {
        obj.clone()
    } else if let Some(obj) = val.as_object() {
        obj.clone()
    } else {
        serde_json::Map::new()
    }
}

fn classify_local_only_mcp(name: &str, cfg: &Value) -> (bool, bool, Option<String>) {
    let lower = name.to_ascii_lowercase();
    let blob = cfg.to_string().to_ascii_lowercase();
    if lower.contains("browser") || blob.contains("browsermcp") || blob.contains("chrome") {
        return (
            true,
            true,
            Some(
                "Blocked on cloud — controls local desktop Chrome; use Headless Playwright instead"
                    .into(),
            ),
        );
    }
    if blob.contains("/users/") || blob.contains("/applications/") || blob.contains("osascript") {
        return (false, true, Some("References a local macOS path".into()));
    }
    (false, false, None)
}

fn discover_local_service_creds() -> Vec<LocalServiceCred> {
    let Some(home) = dirs::home_dir() else {
        return Vec::new();
    };
    let candidates: Vec<(&str, &str, &str, Vec<PathBuf>)> = vec![
        (
            "gh",
            "GitHub CLI (gh)",
            "PRs, issues, CI checks & releases",
            vec![home.join(".config").join("gh").join("hosts.yml")],
        ),
        (
            "railway",
            "Railway",
            "Deploy & manage Railway projects/services",
            vec![home.join(".railway").join("config.json")],
        ),
        (
            "render",
            "Render",
            "Deploy & manage Render services",
            vec![
                home.join(".render").join("cli.yaml"),
                home.join(".config").join("render").join("cli.yaml"),
            ],
        ),
        (
            "vercel",
            "Vercel",
            "Deploy & manage Vercel projects",
            vec![
                home.join("Library")
                    .join("Application Support")
                    .join("com.vercel.cli")
                    .join("auth.json"),
                home.join(".config").join("vercel").join("auth.json"),
            ],
        ),
        (
            "fly",
            "Fly.io",
            "Deploy & manage Fly apps",
            vec![home.join(".fly").join("config.yml")],
        ),
        (
            "wrangler",
            "Cloudflare Wrangler",
            "Deploy Cloudflare Workers/Pages",
            vec![
                home.join("Library")
                    .join("Preferences")
                    .join(".wrangler")
                    .join("config")
                    .join("default.toml"),
                home.join(".config")
                    .join(".wrangler")
                    .join("config")
                    .join("default.toml"),
            ],
        ),
        (
            "npm",
            "npm registry",
            "Publish or install private npm packages",
            vec![home.join(".npmrc")],
        ),
    ];

    let mut found = Vec::new();
    for (id, label, scope, paths) in candidates {
        if let Some(p) = paths.into_iter().find(|p| p.is_file()) {
            if id == "npm" {
                let has_token = std::fs::read_to_string(&p)
                    .map(|s| s.contains("_authToken") || s.contains("_auth="))
                    .unwrap_or(false);
                if !has_token {
                    continue;
                }
            }
            found.push(LocalServiceCred {
                id: id.into(),
                label: label.into(),
                scope: scope.into(),
                path: p,
            });
        }
    }
    found
}

fn discover_local_ssh_key() -> Option<LocalSshKeyInfo> {
    let home = dirs::home_dir()?;
    let ssh_dir = home.join(".ssh");
    for name in ["id_ed25519", "id_rsa"] {
        let p = ssh_dir.join(name);
        if p.is_file() {
            // Check whether the key can be read without a passphrase for headless Linux use.
            let unencrypted = Command::new("ssh-keygen")
                .args(["-y", "-P", "", "-f"])
                .arg(&p)
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .status()
                .is_ok_and(|s| s.success());
            return Some(LocalSshKeyInfo {
                path: p,
                label: format!("~/.ssh/{name}"),
                unencrypted,
            });
        }
    }
    None
}

pub fn discover_local_inventory(cwd: &Path) -> LocalSyncInventory {
    let mut providers = Vec::new();
    if let Some(auth_path) = local_pi_file("auth.json") {
        if let Ok(raw) = std::fs::read_to_string(&auth_path) {
            if let Ok(Value::Object(map)) = serde_json::from_str::<Value>(&raw) {
                providers = map.keys().cloned().collect();
                providers.sort();
            }
        }
    }

    let mut mcps: Vec<LocalMcpItem> = read_local_mcp_map()
        .into_iter()
        .map(|(name, cfg)| {
            let (blocked_on_cloud, local_only, reason) = classify_local_only_mcp(&name, &cfg);
            LocalMcpItem {
                name,
                blocked_on_cloud,
                local_only,
                reason,
            }
        })
        .collect();
    mcps.sort_by(|a, b| a.name.cmp(&b.name));

    let mut skill_set = BTreeSet::new();
    for dir in local_skills_dirs() {
        if let Ok(rd) = std::fs::read_dir(&dir) {
            for ent in rd.flatten() {
                if ent.file_type().is_ok_and(|t| t.is_dir()) {
                    let name = ent.file_name().to_string_lossy().into_owned();
                    if !name.starts_with('.') {
                        skill_set.insert(name);
                    }
                }
            }
        }
    }
    let skills: Vec<String> = skill_set.into_iter().collect();

    let git_remote = Command::new("git")
        .args(["remote", "get-url", "origin"])
        .current_dir(cwd)
        .output()
        .ok()
        .filter(|o| o.status.success())
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_owned())
        .filter(|s| !s.is_empty());

    let project_name = cwd
        .file_name()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_else(|| "project".into());

    LocalSyncInventory {
        providers,
        mcps,
        skills,
        service_creds: discover_local_service_creds(),
        mac_ssh_key: discover_local_ssh_key(),
        git_remote,
        project_name,
    }
}

pub fn sync_selected_pi_auth(
    cfg: &CloudHostConfig,
    selected: &BTreeSet<String>,
) -> Result<Vec<String>, String> {
    if selected.is_empty() {
        return Ok(Vec::new());
    }
    let auth_path = local_pi_file("auth.json")
        .ok_or_else(|| "No local Pi provider logins found.".to_owned())?;
    let raw = std::fs::read_to_string(&auth_path)
        .map_err(|e| format!("Could not read {}: {e}", auth_path.display()))?;
    let Value::Object(all_map) = serde_json::from_str::<Value>(&raw)
        .map_err(|e| format!("Invalid JSON in {}: {e}", auth_path.display()))?
    else {
        return Err("Local auth.json is not a JSON object.".into());
    };
    let mut filtered = serde_json::Map::new();
    for (k, v) in all_map {
        if selected.contains(&k) {
            filtered.insert(k, v);
        }
    }
    if filtered.is_empty() {
        return Ok(Vec::new());
    }
    let (_, res) = http_request_json(
        cfg,
        "POST",
        "/v1/admin/sync-auth",
        Some(&json!({ "auth": Value::Object(filtered), "merge": true })),
    )?;
    let providers = res["providers"]
        .as_array()
        .map(|arr| {
            arr.iter()
                .filter_map(Value::as_str)
                .map(str::to_owned)
                .collect()
        })
        .unwrap_or_default();
    Ok(providers)
}

pub fn sync_selected_mcps(
    cfg: &CloudHostConfig,
    selected: &BTreeSet<String>,
    add_playwright_research: bool,
) -> Result<Vec<String>, String> {
    if selected.is_empty() && !add_playwright_research {
        return Ok(Vec::new());
    }
    let all = read_local_mcp_map();
    let mut filtered = serde_json::Map::new();
    for (k, v) in all {
        if selected.contains(&k) {
            let (blocked, _, _) = classify_local_only_mcp(&k, &v);
            if !blocked {
                filtered.insert(k, v);
            }
        }
    }
    if add_playwright_research {
        filtered.insert(
            PLAYWRIGHT_RESEARCH_MCP.to_owned(),
            json!({
                "command": "npx",
                "args": ["-y", "@playwright/mcp@latest", "--headless", "--isolated"],
            }),
        );
    }
    if filtered.is_empty() {
        return Ok(Vec::new());
    }
    let (_, res) = http_request_json(
        cfg,
        "POST",
        "/v1/admin/sync-mcps",
        Some(&json!({ "mcpServers": Value::Object(filtered), "merge": true })),
    )?;
    let servers = res["mcp_servers"]
        .as_array()
        .map(|arr| {
            arr.iter()
                .filter_map(Value::as_str)
                .map(str::to_owned)
                .collect()
        })
        .unwrap_or_default();
    Ok(servers)
}

pub fn sync_selected_service_creds(
    cfg: &CloudHostConfig,
    selected: &BTreeSet<String>,
) -> Result<Vec<String>, String> {
    if selected.is_empty() {
        return Ok(Vec::new());
    }
    let available = discover_local_service_creds();
    let mut payload = Vec::new();
    for cred in available {
        if selected.contains(&cred.id) {
            if let Ok(content) = std::fs::read_to_string(&cred.path) {
                if !content.trim().is_empty() {
                    payload.push(json!({
                        "id": cred.id,
                        "content": content,
                    }));
                }
            }
        }
    }
    if payload.is_empty() {
        return Ok(Vec::new());
    }
    let (_, res) = http_request_json(
        cfg,
        "POST",
        "/v1/admin/sync-service-creds",
        Some(&json!({ "credentials": payload })),
    )?;
    let synced = res["service_creds"]
        .as_array()
        .map(|arr| {
            arr.iter()
                .filter_map(Value::as_str)
                .map(str::to_owned)
                .collect()
        })
        .unwrap_or_default();
    Ok(synced)
}

fn collect_skill_text_files(root: &Path, dir: &Path, out: &mut Vec<Value>) {
    if out.len() >= 30 {
        return;
    }
    let Ok(rd) = std::fs::read_dir(dir) else {
        return;
    };
    for ent in rd.flatten() {
        if out.len() >= 30 {
            break;
        }
        let path = ent.path();
        let name = ent.file_name().to_string_lossy().into_owned();
        if name.starts_with('.') || name == "node_modules" {
            continue;
        }
        if ent.file_type().is_ok_and(|t| t.is_dir()) {
            collect_skill_text_files(root, &path, out);
        } else if ent.file_type().is_ok_and(|t| t.is_file()) {
            if let Ok(meta) = ent.metadata() {
                if meta.len() > 200 * 1024 {
                    continue;
                }
            }
            if let Ok(content) = std::fs::read_to_string(&path) {
                if let Ok(rel) = path.strip_prefix(root) {
                    out.push(json!({
                        "path": rel.to_string_lossy().replace('\\', "/"),
                        "content": content,
                    }));
                }
            }
        }
    }
}

pub fn sync_selected_skills(
    cfg: &CloudHostConfig,
    selected: &BTreeSet<String>,
) -> Result<Vec<String>, String> {
    if selected.is_empty() {
        return Ok(Vec::new());
    }
    let mut bundles = Vec::new();
    let dirs = local_skills_dirs();
    for skill_name in selected {
        let Some(skill_dir) = dirs.iter().map(|d| d.join(skill_name)).find(|p| p.is_dir()) else {
            continue;
        };
        let mut files = Vec::new();
        collect_skill_text_files(&skill_dir, &skill_dir, &mut files);
        if !files.is_empty() {
            bundles.push(json!({
                "name": skill_name,
                "files": files,
            }));
        }
    }
    if bundles.is_empty() {
        return Ok(Vec::new());
    }
    let (_, res) = http_request_json(
        cfg,
        "POST",
        "/v1/admin/sync-skills",
        Some(&json!({ "skills": bundles })),
    )?;
    let synced = res["skills"]
        .as_array()
        .map(|arr| {
            arr.iter()
                .filter_map(Value::as_str)
                .map(str::to_owned)
                .collect()
        })
        .unwrap_or_default();
    Ok(synced)
}

pub fn copy_mac_ssh_key_to_cloud(cfg: &CloudHostConfig) -> Result<(), String> {
    let key_info = discover_local_ssh_key().ok_or_else(|| {
        "No local SSH key found at ~/.ssh/id_ed25519 or ~/.ssh/id_rsa.".to_owned()
    })?;
    if !key_info.unencrypted {
        return Err(format!(
            "{} is passphrase-protected and cannot be used unattended on a headless cloud host. Use a dedicated cloud key instead.",
            key_info.label
        ));
    }
    let private_key = std::fs::read_to_string(&key_info.path)
        .map_err(|e| format!("Could not read {}: {e}", key_info.label))?;
    let pub_path = PathBuf::from(format!("{}.pub", key_info.path.display()));
    let public_key = std::fs::read_to_string(&pub_path).ok();
    let user_name = local_git_config("user.name");
    let user_email = local_git_config("user.email");
    http_request_json(
        cfg,
        "POST",
        "/v1/admin/git-setup",
        Some(&json!({
            "user_name": user_name,
            "user_email": user_email,
            "private_key": private_key,
            "public_key": public_key,
        })),
    )?;
    Ok(())
}

pub fn execute_selective_sync(
    cfg: &CloudHostConfig,
    cwd: &Path,
    plan: &CloudSyncPlan,
) -> Result<CloudSyncReport, String> {
    let mut parts = Vec::new();
    let mut action_needed: Option<CloudActionNotice> = None;

    if !plan.providers.is_empty() {
        let provs = sync_selected_pi_auth(cfg, &plan.providers)?;
        parts.push(format!("{} provider(s)", provs.len()));
    }
    if plan.setup_git_ssh {
        match plan.git_auth_choice {
            GitAuthChoice::CopyMacKey => {
                copy_mac_ssh_key_to_cloud(cfg)?;
                parts.push("Mac SSH key".into());
            }
            GitAuthChoice::DedicatedCloudKey => {
                let git_res = setup_git_and_register_github(cfg)?;
                if git_res.registered_on_github {
                    parts.push("Git SSH".into());
                } else {
                    parts.push("Git SSH key created".into());
                    action_needed = Some(CloudActionNotice {
                        title: "Add cloud SSH key to GitHub".into(),
                        detail: "Public key copied — paste once in GitHub SSH settings to enable auto-clone.".into(),
                        copy_text: Some(git_res.public_key),
                        open_url: Some("https://github.com/settings/ssh/new".into()),
                        open_label: Some("Open GitHub SSH".into()),
                    });
                }
            }
        }
    }
    if !plan.mcps.is_empty() || plan.add_playwright_research {
        let mcps = sync_selected_mcps(cfg, &plan.mcps, plan.add_playwright_research)?;
        if plan.add_playwright_research && plan.mcps.is_empty() {
            parts.push("Playwright".into());
        } else {
            parts.push(format!("{} MCP(s)", mcps.len()));
        }
    }
    if !plan.service_creds.is_empty() {
        let creds = sync_selected_service_creds(cfg, &plan.service_creds)?;
        parts.push(format!("{} service(s)", creds.len()));
    }
    if !plan.skills.is_empty() {
        let skills = sync_selected_skills(cfg, &plan.skills)?;
        parts.push(format!("{} skill(s)", skills.len()));
    }
    if plan.clone_current_repo {
        let _ = clone_current_repo_to_cloud(cfg, cwd)?;
        parts.push("repo cloned".into());
    }

    let summary = if parts.is_empty() {
        "Nothing selected to sync.".into()
    } else {
        parts.join(" · ")
    };

    Ok(CloudSyncReport {
        title: format!("Synced to {}", cfg.name),
        summary,
        action_needed,
    })
}

fn local_git_config(key: &str) -> Option<String> {
    Command::new("git")
        .args(["config", "--global", "--get", key])
        .output()
        .ok()
        .filter(|o| o.status.success())
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_owned())
        .filter(|s| !s.is_empty())
}

pub fn setup_git_and_register_github(cfg: &CloudHostConfig) -> Result<CloudGitResult, String> {
    let user_name = local_git_config("user.name");
    let user_email = local_git_config("user.email");
    let (_, res) = http_request_json(
        cfg,
        "POST",
        "/v1/admin/git-setup",
        Some(&json!({
            "user_name": user_name,
            "user_email": user_email,
            "regenerate_key": false,
        })),
    )?;
    let public_key = res["public_key"]
        .as_str()
        .ok_or_else(|| "Cloud host did not return a public SSH key.".to_owned())?
        .trim()
        .to_owned();

    // Register the cloud host's dedicated revocable public key on GitHub via local `gh` CLI (if available).
    let title = format!("Pui Cloud ({})", cfg.name);
    let registered = Command::new("gh")
        .args(["ssh-key", "add", "-", "--title", &title])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .and_then(|mut child| {
            if let Some(mut stdin) = child.stdin.take() {
                let _ = writeln!(stdin, "{public_key}");
            }
            child.wait_with_output()
        });

    match registered {
        Ok(out) if out.status.success() => Ok(CloudGitResult {
            public_key,
            registered_on_github: true,
        }),
        Ok(out) => {
            let err = String::from_utf8_lossy(&out.stderr).trim().to_owned();
            Ok(CloudGitResult {
                public_key,
                registered_on_github: err.to_ascii_lowercase().contains("already in use"),
            })
        }
        Err(_) => Ok(CloudGitResult {
            public_key,
            registered_on_github: false,
        }),
    }
}

pub fn clone_current_repo_to_cloud(cfg: &CloudHostConfig, cwd: &Path) -> Result<String, String> {
    let out = Command::new("git")
        .args(["remote", "get-url", "origin"])
        .current_dir(cwd)
        .output()
        .map_err(|e| format!("Could not inspect git remote in {}: {e}", cwd.display()))?;
    if !out.status.success() {
        return Err("Current workspace has no Git `origin` remote to clone.".into());
    }
    let url = String::from_utf8_lossy(&out.stdout).trim().to_owned();
    let (_, res) = http_request_json(
        cfg,
        "POST",
        "/v1/projects/clone",
        Some(&json!({ "url": url })),
    )?;
    let path = res["path"].as_str().unwrap_or("workspace").to_owned();
    let existed = res["already_exists"].as_bool().unwrap_or(false);
    if existed {
        Ok(format!(
            "Repository already exists on {} at {path}.",
            cfg.name
        ))
    } else {
        Ok(format!("Cloned into {} at {path}.", cfg.name))
    }
}

fn url_decode(s: &str) -> String {
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'+' => {
                out.push(b' ');
                i += 1;
            }
            b'%' if i + 2 < bytes.len() => {
                if let Ok(hex) = std::str::from_utf8(&bytes[i + 1..i + 3]) {
                    if let Ok(b) = u8::from_str_radix(hex, 16) {
                        out.push(b);
                        i += 3;
                        continue;
                    }
                }
                out.push(b'%');
                i += 1;
            }
            b => {
                out.push(b);
                i += 1;
            }
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_cloud_pairing_uri() {
        let cfg = parse_pairing_uri(
            "pui://pair?v=1&host=100.84.93.31&port=4817&token=abc123&name=pui-cloud&tls=0",
        )
        .unwrap();
        assert_eq!(cfg.host, "100.84.93.31");
        assert_eq!(cfg.port, 4817);
        assert_eq!(cfg.token, "abc123");
        assert_eq!(cfg.name, "pui-cloud");
        assert!(!cfg.tls);

        assert!(parse_pairing_uri("https://example.com").is_err());
        assert!(parse_pairing_uri("pui://pair?v=1&host=100.84.93.31").is_err());
    }
}
