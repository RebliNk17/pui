//! Resolve immutable app runtimes without changing Pi's writable user data.
use std::{
    ffi::{OsStr, OsString},
    path::{Path, PathBuf},
    process::Command,
};

fn bundled_at(exe: &Path) -> Option<PathBuf> {
    let macos = exe.parent()?;
    if macos.file_name()? != "MacOS" || macos.parent()?.file_name()? != "Contents" {
        return None;
    }
    Some(macos.join("../Resources/runtime"))
}

pub(crate) fn bundled() -> Option<PathBuf> {
    bundled_at(&std::env::current_exe().ok()?)
}

fn resolve_pi(
    override_bin: Option<OsString>,
    bundle: Option<&Path>,
    home: Option<&Path>,
) -> PathBuf {
    if let Some(path) = override_bin {
        return path.into();
    }
    // A damaged app must fail, not silently run an unrelated global Pi.
    if let Some(root) = bundle {
        return root.join("bin/pi");
    }
    if let Some(home) = home {
        let installed =
            home.join(".pi/agent/bin")
                .join(if cfg!(windows) { "pi.cmd" } else { "pi" });
        if installed.is_file() {
            return installed;
        }
    }
    PathBuf::from(if cfg!(windows) { "pi.cmd" } else { "pi" })
}

pub(crate) fn pi_binary() -> PathBuf {
    resolve_pi(
        std::env::var_os("PUI_PI_BIN"),
        bundled().as_deref(),
        dirs::home_dir().as_deref(),
    )
}

pub(crate) fn active_bundle() -> Option<PathBuf> {
    if std::env::var_os("PUI_PI_BIN").is_some() {
        None
    } else {
        bundled()
    }
}

pub(crate) fn node_binary() -> PathBuf {
    active_bundle()
        .map(|root| root.join("node/bin/node"))
        .unwrap_or_else(|| PathBuf::from(if cfg!(windows) { "node.exe" } else { "node" }))
}

fn resolve_sdk(
    override_sdk: Option<OsString>,
    external_pi: bool,
    bundle: Option<&Path>,
    agent: Option<&Path>,
) -> Option<PathBuf> {
    if let Some(path) = override_sdk {
        return Some(path.into());
    }
    if !external_pi {
        if let Some(root) = bundle {
            return Some(
                root.join("pi/node_modules/@earendil-works/pi-coding-agent/dist/index.js"),
            );
        }
    }
    let agent = agent?;
    let version = std::fs::read_to_string(agent.join("install/current-version")).ok()?;
    let version = version.trim();
    if version.is_empty() || version.contains(['/', '\\']) || matches!(version, "." | "..") {
        return None;
    }
    Some(
        agent
            .join("install/releases")
            .join(version)
            .join("node_modules/@earendil-works/pi-coding-agent/dist/index.js"),
    )
}

pub(crate) fn sdk() -> Option<PathBuf> {
    let agent = agent_dir();
    resolve_sdk(
        std::env::var_os("PUI_PI_SDK"),
        std::env::var_os("PUI_PI_BIN").is_some(),
        bundled().as_deref(),
        agent.as_deref(),
    )
}

/// Version of the Pi package Pui runs, read from its own package.json beside
/// the resolved SDK entry point (bundled runtime or installed release).
fn pi_version_at(sdk: &Path) -> Option<String> {
    let manifest = sdk.parent()?.parent()?.join("package.json");
    let text = std::fs::read_to_string(manifest).ok()?;
    let value: serde_json::Value = serde_json::from_str(&text).ok()?;
    if value.get("name")?.as_str()? != "@earendil-works/pi-coding-agent" {
        return None;
    }
    let version = value.get("version")?.as_str()?.trim();
    (!version.is_empty() && version.len() <= 64).then(|| version.to_owned())
}

/// Cached for the process: the bundled runtime is immutable.
pub(crate) fn pi_version() -> Option<String> {
    static VERSION: std::sync::OnceLock<Option<String>> = std::sync::OnceLock::new();
    VERSION
        .get_or_init(|| sdk().and_then(|path| pi_version_at(&path)))
        .clone()
}

/// Marker written once Pui owns Pi's configuration and sessions.
pub(crate) const MANAGED_MARKER: &str = ".pui-managed.json";
static MANAGED_AGENT_DIR: std::sync::RwLock<Option<PathBuf>> = std::sync::RwLock::new(None);

/// Pui's own Pi folder: providers, logins, models, defaults, resources, sessions.
pub(crate) fn pui_agent_dir() -> Option<PathBuf> {
    dirs::data_local_dir().map(|dir| dir.join("pui").join("pi-agent"))
}

/// Adopt Pui's Pi folder if it has been set up (migrated or created fresh).
/// Every Pi process Pui starts then uses it; `~/.pi/agent` is not consulted.
pub(crate) fn load_managed_agent_dir() -> Option<PathBuf> {
    let dir = pui_agent_dir().filter(|dir| dir.join(MANAGED_MARKER).is_file());
    if let Ok(mut managed) = MANAGED_AGENT_DIR.write() {
        managed.clone_from(&dir);
    }
    dir
}

pub(crate) fn managed_agent_dir() -> Option<PathBuf> {
    MANAGED_AGENT_DIR.read().ok().and_then(|dir| dir.clone())
}

/// Provider API-key variables Pi reads from its environment (pi-ai
/// env-api-keys). Pi cannot tell a mislabelled key (e.g. an NVIDIA key in
/// OPENAI_API_KEY) from a real login, so Pui only gives Pi its own logins.
pub(crate) const PROVIDER_KEY_VARS: &[&str] = &[
    "ANTHROPIC_API_KEY",
    "ANTHROPIC_AUTH_TOKEN",
    "ANTHROPIC_OAUTH_TOKEN",
    "OPENAI_API_KEY",
    "AZURE_OPENAI_API_KEY",
    "NVIDIA_API_KEY",
    "DEEPSEEK_API_KEY",
    "GEMINI_API_KEY",
    "GOOGLE_CLOUD_API_KEY",
    "GROQ_API_KEY",
    "CEREBRAS_API_KEY",
    "XAI_API_KEY",
    "TYPESAFE_API_KEY",
    "RADIUS_API_KEY",
    "OPENROUTER_API_KEY",
    "AI_GATEWAY_API_KEY",
    "ZAI_API_KEY",
    "ZAI_CODING_CN_API_KEY",
    "MISTRAL_API_KEY",
    "MINIMAX_API_KEY",
    "MINIMAX_CN_API_KEY",
    "MOONSHOT_API_KEY",
    "HF_TOKEN",
    "FIREWORKS_API_KEY",
    "TOGETHER_API_KEY",
    "BASETEN_API_KEY",
    "OPENCODE_API_KEY",
    "KIMI_API_KEY",
    "META_API_KEY",
    "CLOUDFLARE_API_KEY",
    "XIAOMI_API_KEY",
    "XIAOMI_TOKEN_PLAN_CN_API_KEY",
    "XIAOMI_TOKEN_PLAN_AMS_API_KEY",
    "XIAOMI_TOKEN_PLAN_SGP_API_KEY",
    "ANT_LING_API_KEY",
    "QWEN_TOKEN_PLAN_API_KEY",
    "QWEN_TOKEN_PLAN_CN_API_KEY",
    "QWEN_TOKEN_PLAN_TEAM_API_KEY",
    "QWEN_TOKEN_PLAN_TEAM_CN_API_KEY",
    "QWEN_TOKEN_PLAN_INDIVIDUAL_API_KEY",
    "QWEN_TOKEN_PLAN_INDIVIDUAL_CN_API_KEY",
    "COPILOT_GITHUB_TOKEN",
];

/// Off by default: Settings → General → "Use provider keys from environment".
static USE_ENVIRONMENT_KEYS: std::sync::atomic::AtomicBool =
    std::sync::atomic::AtomicBool::new(false);

pub(crate) fn set_use_environment_keys(enabled: bool) {
    USE_ENVIRONMENT_KEYS.store(enabled, std::sync::atomic::Ordering::Relaxed);
}

fn strip_environment_keys() -> bool {
    !USE_ENVIRONMENT_KEYS.load(std::sync::atomic::Ordering::Relaxed)
}

/// Keys withheld from Pi, restored for commands the agent runs (JSON object).
pub(crate) fn shell_provider_env() -> Option<String> {
    if !strip_environment_keys() {
        return None;
    }
    let vars: serde_json::Map<String, serde_json::Value> = PROVIDER_KEY_VARS
        .iter()
        .filter_map(|name| {
            let value = std::env::var(name).ok().filter(|v| !v.is_empty())?;
            Some(((*name).to_owned(), serde_json::Value::String(value)))
        })
        .collect();
    (!vars.is_empty()).then(|| serde_json::Value::Object(vars).to_string())
}

/// Provider key variables present in Pui's environment (names only).
pub(crate) fn environment_key_names() -> Vec<&'static str> {
    PROVIDER_KEY_VARS
        .iter()
        .copied()
        .filter(|name| std::env::var_os(name).is_some_and(|v| !v.is_empty()))
        .collect()
}

pub(crate) fn agent_dir() -> Option<PathBuf> {
    if let Some(dir) = managed_agent_dir() {
        return Some(dir);
    }
    std::env::var_os("PI_CODING_AGENT_DIR")
        .map(PathBuf::from)
        .or_else(|| dirs::home_dir().map(|home| home.join(".pi/agent")))
}

pub(crate) fn prepend_bundle_path(inherited: &OsStr, bundle: Option<&Path>) -> Vec<PathBuf> {
    let mut paths = Vec::new();
    if let Some(root) = bundle {
        paths.push(root.join("bin"));
    }
    paths.extend(std::env::split_paths(inherited));
    paths
}

/// Shared by RPC and every Node helper. The app is code, never an npm/Jiti cache.
pub(crate) fn configure(command: &mut Command) {
    if let Some(path) = crate::rpc::child_path() {
        command.env("PATH", path);
    }
    if let Some(dir) = managed_agent_dir() {
        // Pui's own folder for config and sessions alike.
        command
            .env("PI_CODING_AGENT_DIR", dir)
            .env_remove("PI_CODING_AGENT_SESSION_DIR");
    }
    if strip_environment_keys() {
        for name in PROVIDER_KEY_VARS {
            command.env_remove(name);
        }
    }
    if active_bundle().is_some() {
        command.env("JITI_FS_CACHE", "false");
        if let Some(cache) = dirs::cache_dir() {
            command.env("NODE_COMPILE_CACHE", cache.join("pui/node-compile"));
            if std::env::var_os("npm_config_cache").is_none()
                && std::env::var_os("NPM_CONFIG_CACHE").is_none()
            {
                command.env("npm_config_cache", cache.join("pui/npm"));
            }
        }
        // Do not independently update the immutable harness inside a signed app.
        command.env("PI_SKIP_VERSION_CHECK", "1");
        if std::env::var_os("npm_config_prefix").is_none()
            && std::env::var_os("NPM_CONFIG_PREFIX").is_none()
        {
            if let Some(agent) = agent_dir() {
                command.env("npm_config_prefix", agent.join("npm-global"));
            }
        }
    }
}

/// Offline diagnostic through the same executable/environment path as helpers.
pub(crate) fn check() -> Result<(), String> {
    let sdk = sdk().ok_or("Pi SDK is unavailable")?;
    let mut command = Command::new(node_binary());
    configure(&mut command);
    command.args(["--input-type=module", "--eval", "const {pathToFileURL}=await import('node:url');const sdk=await import(pathToFileURL(process.argv[1]).href);if(typeof sdk.SessionManager!=='function')throw Error('SDK unavailable');process.stdout.write('bundled-node-sdk-ok');"]).arg(sdk);
    command
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::null());
    let mut child = command.spawn().map_err(|_| "Cannot execute Node runtime")?;
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(20);
    loop {
        match child.try_wait() {
            Ok(Some(_)) => break,
            Ok(None) if std::time::Instant::now() < deadline => {
                std::thread::sleep(std::time::Duration::from_millis(25));
            }
            _ => {
                let _ = child.kill();
                let _ = child.wait();
                return Err("Node runtime diagnostic timed out".into());
            }
        }
    }
    let output = child
        .wait_with_output()
        .map_err(|_| "Cannot read runtime result")?;
    if !output.status.success() || output.stdout != b"bundled-node-sdk-ok" {
        return Err("Node/SDK runtime diagnostic failed".into());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bundle_resolution_is_relocatable_and_requires_app_layout() {
        let temp = std::env::temp_dir().join(format!("pui-runtime-{}", uuid::Uuid::new_v4()));
        let macos = temp.join("Renamed App.app/Contents/MacOS");
        let root = temp.join("Renamed App.app/Contents/Resources/runtime");
        std::fs::create_dir_all(&macos).unwrap();
        std::fs::create_dir_all(&root).unwrap();
        assert_eq!(
            bundled_at(&macos.join("pui")),
            Some(macos.join("../Resources/runtime"))
        );
        assert!(bundled_at(&temp.join("target/debug/pui")).is_none());
        assert_eq!(resolve_pi(None, Some(&root), None), root.join("bin/pi"));
        assert_eq!(
            resolve_pi(Some("/external/pi".into()), Some(&root), None),
            PathBuf::from("/external/pi")
        );
        assert_eq!(
            resolve_sdk(None, false, Some(&root), None),
            Some(root.join("pi/node_modules/@earendil-works/pi-coding-agent/dist/index.js"))
        );
        assert!(resolve_sdk(None, true, Some(&root), None).is_none());
        assert_eq!(
            resolve_sdk(Some("/external/sdk.js".into()), false, Some(&root), None),
            Some("/external/sdk.js".into())
        );
        // Missing bundled binaries never cause a global fallback.
        assert!(!root.join("bin/pi").exists());
        assert_eq!(resolve_pi(None, Some(&root), None), root.join("bin/pi"));
        std::fs::remove_dir_all(temp).unwrap();
    }

    #[test]
    fn pi_processes_never_receive_provider_keys_from_the_environment() {
        let mut command = Command::new("true");
        configure(&mut command);
        let removed: Vec<_> = command
            .get_envs()
            .filter(|(_, value)| value.is_none())
            .map(|(name, _)| name.to_string_lossy().into_owned())
            .collect();
        for name in ["OPENAI_API_KEY", "GEMINI_API_KEY", "ANTHROPIC_API_KEY", "NVIDIA_API_KEY"] {
            assert!(removed.iter().any(|r| r == name), "{name} must be withheld from Pi");
        }
        assert!(!removed.iter().any(|r| r == "PATH" || r == "HOME"));
    }

    #[test]
    fn pi_version_reads_sdk_package_manifest_only() {
        let temp = std::env::temp_dir().join(format!("pui-pi-version-{}", uuid::Uuid::new_v4()));
        let package = temp.join("node_modules/@earendil-works/pi-coding-agent");
        std::fs::create_dir_all(package.join("dist")).unwrap();
        let sdk = package.join("dist/index.js");
        assert_eq!(pi_version_at(&sdk), None);
        std::fs::write(
            package.join("package.json"),
            r#"{"name":"@earendil-works/pi-coding-agent","version":"1.6.0"}"#,
        )
        .unwrap();
        assert_eq!(pi_version_at(&sdk).as_deref(), Some("1.6.0"));
        std::fs::write(
            package.join("package.json"),
            r#"{"name":"other","version":"9.9.9"}"#,
        )
        .unwrap();
        assert_eq!(pi_version_at(&sdk), None);
        std::fs::remove_dir_all(temp).unwrap();
    }

    #[test]
    fn path_prefers_bundle_and_keeps_callers_tools_and_spaces() {
        let inherited = std::env::join_paths(["/usr/bin", "/custom tools/bin"]).unwrap();
        let paths = prepend_bundle_path(&inherited, Some(Path::new("/Some App.app/runtime")));
        assert_eq!(
            paths,
            [
                PathBuf::from("/Some App.app/runtime/bin"),
                "/usr/bin".into(),
                "/custom tools/bin".into()
            ]
        );
        assert!(std::env::join_paths(paths).is_ok());
    }
}
