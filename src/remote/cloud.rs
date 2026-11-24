//! Pure-stdlib client for connecting Pui Desktop to a headless `pui-daemon` cloud host
//! (e.g. Oracle Cloud Always Free VM over Tailscale) for explicit credential sync,
//! dedicated revocable Git SSH key provisioning, and remote repo cloning.
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::{
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

#[derive(Clone, Debug, Default, Deserialize)]
#[serde(default)]
pub struct CloudAdminStatus {
    pub has_pi_auth: bool,
    pub pi_auth_providers: Vec<String>,
    pub has_git_ssh_key: bool,
    pub git_public_key: Option<String>,
    pub git_user_name: Option<String>,
    pub git_user_email: Option<String>,
    pub projects: Vec<String>,
    pub workspaces_dir: String,
}

#[derive(Clone, Debug)]
pub struct CloudGitResult {
    pub public_key: String,
    pub registered_on_github: bool,
    pub detail: String,
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
    let parsed: Value = serde_json::from_str(body_str.trim()).unwrap_or(Value::Null);
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

fn local_pi_auth_path() -> Option<PathBuf> {
    if let Ok(dir) = std::env::var("PI_CODING_AGENT_DIR") {
        let p = PathBuf::from(dir).join("auth.json");
        if p.is_file() {
            return Some(p);
        }
    }
    let home = dirs::home_dir()?;
    let p = home.join(".pi").join("agent").join("auth.json");
    p.is_file().then_some(p)
}

pub fn sync_pi_auth(cfg: &CloudHostConfig) -> Result<Vec<String>, String> {
    let auth_path = local_pi_auth_path()
        .ok_or_else(|| "No local Pi provider logins found at ~/.pi/agent/auth.json.".to_owned())?;
    let raw = std::fs::read_to_string(&auth_path)
        .map_err(|e| format!("Could not read {}: {e}", auth_path.display()))?;
    let auth: Value = serde_json::from_str(&raw)
        .map_err(|e| format!("Invalid JSON in {}: {e}", auth_path.display()))?;
    if !auth.is_object() {
        return Err("Local auth.json is not a JSON object.".into());
    }
    let (_, res) = http_request_json(
        cfg,
        "POST",
        "/v1/admin/sync-auth",
        Some(&json!({ "auth": auth, "merge": true })),
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
            detail: format!(
                "Dedicated SSH key created on {} and added to GitHub as \"{title}\".",
                cfg.name
            ),
        }),
        Ok(out) => {
            let err = String::from_utf8_lossy(&out.stderr).trim().to_owned();
            if err.to_ascii_lowercase().contains("already in use") {
                Ok(CloudGitResult {
                    public_key,
                    registered_on_github: true,
                    detail: format!(
                        "Dedicated SSH key on {} is already registered on GitHub.",
                        cfg.name
                    ),
                })
            } else {
                Ok(CloudGitResult {
                    public_key,
                    registered_on_github: false,
                    detail: format!(
                        "Dedicated SSH key ready on {}. Add the public key to GitHub manually ({err}).",
                        cfg.name
                    ),
                })
            }
        }
        Err(_) => Ok(CloudGitResult {
            public_key,
            registered_on_github: false,
            detail: format!(
                "Dedicated SSH key ready on {}. Install/login `gh` or add the copied public key in GitHub SSH settings.",
                cfg.name
            ),
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
