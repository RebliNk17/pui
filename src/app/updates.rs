//! GitHub release checks, background auto-check, ZIP download/verification,
//! staged installation, and post-restart temporary cleanup.
use super::*;
use sha2::{Digest, Sha256};
use std::io::Read;
use std::path::{Path, PathBuf};

const DEFAULT_REPOSITORY: &str = "RebliNk17/pui";
const AUTO_CHECK_INTERVAL: Duration = Duration::from_secs(6 * 60 * 60);

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct Release {
    pub version: String,
    pub url: String,
    pub body: String,
    pub zip_url: Option<String>,
    pub sums_url: Option<String>,
    pub newer: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) enum InstallState {
    Idle,
    Downloading,
    Ready {
        staged_dir: PathBuf,
        staged_app: PathBuf,
        target_app: Option<PathBuf>,
    },
    Failed(String),
}

impl Default for InstallState {
    fn default() -> Self {
        Self::Idle
    }
}

pub(super) struct Updates {
    pub open: bool,
    pub auto_check: bool,
    result: Option<Result<Release, String>>,
    rx: Option<Receiver<Result<Release, String>>>,
    install: InstallState,
    install_rx: Option<Receiver<Result<(PathBuf, PathBuf, Option<PathBuf>), String>>>,
    last_auto_check: Option<std::time::Instant>,
    checked: bool,
    cleaned_up: bool,
}

impl Default for Updates {
    fn default() -> Self {
        Self {
            open: false,
            auto_check: true,
            result: None,
            rx: None,
            install: InstallState::Idle,
            install_rx: None,
            last_auto_check: None,
            checked: false,
            cleaned_up: false,
        }
    }
}

pub(super) fn repository() -> Option<&'static str> {
    option_env!("PUI_GITHUB_REPOSITORY")
        .filter(|s| !s.trim().is_empty())
        .or(Some(DEFAULT_REPOSITORY))
}

pub(super) fn configured() -> bool {
    repository().is_some_and(validate_repository)
}

fn validate_repository(repo: &str) -> bool {
    let parts: Vec<_> = repo.split('/').collect();
    parts.len() == 2
        && parts.iter().all(|s| {
            !s.is_empty()
                && ![".", ".."].contains(s)
                && s.bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b"._-".contains(&b))
        })
}

fn valid_asset_url(repo: &str, url: &str) -> bool {
    let prefix = format!("https://github.com/{repo}/releases/download/");
    let api_prefix = format!("https://api.github.com/repos/{repo}/releases/assets/");
    (url.starts_with(&prefix) || url.starts_with(&api_prefix))
        && !url.contains("..")
        && !url
            .bytes()
            .any(|b| b.is_ascii_whitespace() || b.is_ascii_control())
}

fn release(repo: &str, current: &str, data: &Value) -> Result<Release, String> {
    if !validate_repository(repo) {
        return Err(
            "Invalid GitHub release repository. Configure OWNER/REPO at build time.".into(),
        );
    }
    if data["draft"] != false || data["prerelease"] != false {
        return Err("GitHub did not return a stable release.".into());
    }
    let tag = data["tag_name"]
        .as_str()
        .ok_or("GitHub release has no version tag.")?;
    let version = semver::Version::parse(tag.strip_prefix('v').unwrap_or(tag))
        .map_err(|_| "Release tag is not a semantic version (expected v1.2.3).")?;
    if !version.pre.is_empty() {
        return Err("GitHub did not return a stable release.".into());
    }
    let current =
        semver::Version::parse(current).map_err(|_| "This build has an invalid version.")?;
    let mut zip_url = None;
    let mut sums_url = None;
    if let Some(assets) = data["assets"].as_array() {
        // Prefer signed notarized macOS arm64 zip over adhoc zip if both exist.
        for asset in assets {
            let Some(name) = asset["name"].as_str() else {
                continue;
            };
            let Some(download) = asset["browser_download_url"].as_str() else {
                continue;
            };
            if !valid_asset_url(repo, download) {
                continue;
            }
            if name == "SHA256SUMS" {
                sums_url = Some(download.to_owned());
            } else if name.starts_with("Pui-")
                && name.contains("macos-arm64")
                && name.ends_with(".zip")
            {
                if zip_url.is_none() || !name.contains("-adhoc.") {
                    zip_url = Some(download.to_owned());
                }
            }
        }
    }
    let body = data["body"]
        .as_str()
        .unwrap_or_default()
        .chars()
        .take(4096)
        .collect::<String>();
    // Ignore html_url from API data. Only open the configured repository's page.
    Ok(Release {
        version: version.to_string(),
        url: format!("https://github.com/{repo}/releases/latest"),
        body,
        zip_url,
        sums_url,
        newer: version > current,
    })
}

fn check(repo: &str) -> Result<Release, String> {
    if !validate_repository(repo) {
        return Err("Invalid GitHub release repository.".into());
    }
    // Use the bundled Node when packaged; fetch is bounded and receives no Pi
    // config, auth or tokens. No new HTTP runtime or idle worker is required.
    let script = r#"
const response = await fetch(process.argv.at(-1), {headers:{Accept:'application/vnd.github+json','User-Agent':'Pui-release-check'},redirect:'follow',signal:AbortSignal.timeout(12000)});
if(!response.ok){process.stdout.write(JSON.stringify({status:response.status}));process.exit(0);}
const reader=response.body.getReader();const chunks=[];let bytes=0;
while(true){const {value,done}=await reader.read();if(done)break;bytes+=value.byteLength;if(bytes>1048576){await reader.cancel();throw Error('response too large');}chunks.push(value);}
process.stdout.write(Buffer.concat(chunks).toString('utf8'));
"#;
    let mut command = std::process::Command::new(crate::runtime::node_binary());
    command
        .args(["--input-type=module", "--eval", script])
        .arg(format!(
            "https://api.github.com/repos/{repo}/releases/latest"
        ))
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::null());
    crate::runtime::configure(&mut command);
    let mut child = command
        .spawn()
        .map_err(|_| "Cannot start the release checker. Reinstall Pui or check the Node runtime for this development build.")?;
    let mut stdout = child
        .stdout
        .take()
        .ok_or("Cannot read the release response.")?;
    let reader = std::thread::spawn(move || {
        let mut bytes = Vec::new();
        stdout
            .by_ref()
            .take(1024 * 1024 + 1)
            .read_to_end(&mut bytes)
            .map(|_| bytes)
    });
    let deadline = std::time::Instant::now() + Duration::from_secs(15);
    loop {
        if child
            .try_wait()
            .map_err(|_| "Release check failed.")?
            .is_some()
        {
            break;
        }
        if std::time::Instant::now() >= deadline {
            let _ = child.kill();
            let _ = child.wait();
            let _ = reader.join();
            return Err("GitHub release check timed out. Try again later.".into());
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    let bytes = reader
        .join()
        .map_err(|_| "Release response failed.")?
        .map_err(|_| "Cannot read the GitHub response.")?;
    if bytes.len() > 1024 * 1024 {
        return Err("GitHub release response is too large.".into());
    }
    let data: Value = serde_json::from_slice(&bytes)
        .map_err(|_| "Release check failed. Check the network and try again.")?;
    if let Some(status) = data["status"].as_u64() {
        return Err(match status {
            404 => "No published release was found. The repository may be private or not published yet.".into(),
            403 | 429 => "GitHub rate limit reached. Try again later.".into(),
            _ => format!("GitHub release check failed (HTTP {status})."),
        });
    }
    release(repo, env!("CARGO_PKG_VERSION"), &data)
}

/// Detect the currently running `.app` bundle path when running inside a packaged macOS app.
pub(super) fn current_app_bundle() -> Option<PathBuf> {
    let exe = std::env::current_exe().ok()?;
    bundle_from_executable(&exe)
}

fn bundle_from_executable(exe: &Path) -> Option<PathBuf> {
    let macos = exe.parent()?;
    if macos.file_name()?.to_str()? != "MacOS" {
        return None;
    }
    let contents = macos.parent()?;
    if contents.file_name()?.to_str()? != "Contents" {
        return None;
    }
    let app = contents.parent()?;
    if app.extension()?.to_str()? != "app" {
        return None;
    }
    Some(app.to_path_buf())
}

fn parse_expected_sha256(sums_text: &str, filename: &str) -> Option<String> {
    for line in sums_text.lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        let mut parts = line.split_whitespace();
        let sha = parts.next()?;
        let name = parts.next()?.trim_start_matches('*');
        if name == filename && sha.len() == 64 && sha.bytes().all(|b| b.is_ascii_hexdigit()) {
            return Some(sha.to_ascii_lowercase());
        }
    }
    None
}

fn sha256_file(path: &Path) -> Result<String, String> {
    let mut file =
        std::fs::File::open(path).map_err(|_| "Cannot read downloaded update archive.")?;
    let mut hasher = Sha256::new();
    let mut buffer = vec![0_u8; 256 * 1024];
    loop {
        let count = file
            .read(&mut buffer)
            .map_err(|_| "Failed reading downloaded update archive.")?;
        if count == 0 {
            break;
        }
        hasher.update(&buffer[..count]);
    }
    Ok(format!("{:x}", hasher.finalize()))
}

fn download_url_to_file(url: &str, dest: &Path, max_bytes: u64) -> Result<(), String> {
    let status = std::process::Command::new("/usr/bin/curl")
        .args([
            "-fsSL",
            "--max-time",
            "600",
            "--max-filesize",
            &max_bytes.to_string(),
            "-o",
        ])
        .arg(dest)
        .arg(url)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .map_err(|_| "Cannot start curl to download the update.")?;
    if !status.success() {
        return Err("Update download failed. Check your connection and try again.".into());
    }
    Ok(())
}

fn download_and_stage(
    repo: &str,
    zip_url: &str,
    sums_url: Option<&str>,
) -> Result<(PathBuf, PathBuf, Option<PathBuf>), String> {
    if !valid_asset_url(repo, zip_url) {
        return Err("Refusing to download from an untrusted update URL.".into());
    }
    if let Some(sums) = sums_url {
        if !valid_asset_url(repo, sums) {
            return Err("Refusing to download checksums from an untrusted URL.".into());
        }
    }
    let filename = zip_url
        .rsplit('/')
        .next()
        .filter(|name| name.ends_with(".zip") && !name.contains(".."))
        .ok_or("Invalid release archive filename.")?;
    let staging = std::env::temp_dir().join(format!("pui-update-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&staging)
        .map_err(|_| "Cannot create temporary directory for update.")?;
    let result = (|| -> Result<(PathBuf, PathBuf, Option<PathBuf>), String> {
        let zip_path = staging.join(filename);
        download_url_to_file(zip_url, &zip_path, 600 * 1024 * 1024)?;
        if let Some(sums_url) = sums_url {
            let sums_path = staging.join("SHA256SUMS");
            download_url_to_file(sums_url, &sums_path, 1024 * 1024)?;
            let sums_text =
                std::fs::read_to_string(&sums_path).map_err(|_| "Cannot read update checksums.")?;
            let expected = parse_expected_sha256(&sums_text, filename)
                .ok_or("Release archive is missing from SHA256SUMS.")?;
            let actual = sha256_file(&zip_path)?;
            if actual != expected {
                return Err("Downloaded update failed SHA-256 checksum verification.".into());
            }
        }
        let unpack = staging.join("unpacked");
        std::fs::create_dir_all(&unpack).map_err(|_| "Cannot create unpack directory.")?;
        let status = std::process::Command::new("/usr/bin/ditto")
            .arg("-x")
            .arg("-k")
            .arg(&zip_path)
            .arg(&unpack)
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status()
            .map_err(|_| "Cannot extract the downloaded update archive.")?;
        if !status.success() {
            return Err("Failed to extract the downloaded update archive.".into());
        }
        // Remove the downloaded zip immediately after unpacking to save disk space.
        let _ = std::fs::remove_file(&zip_path);
        let staged_app = unpack.join("Pui.app");
        if !staged_app.join("Contents/MacOS/pui").is_file() {
            return Err("Extracted update does not contain a valid Pui.app bundle.".into());
        }
        let verify = std::process::Command::new("/usr/bin/codesign")
            .args(["--verify", "--deep", "--strict"])
            .arg(&staged_app)
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status()
            .map_err(|_| "Cannot verify the downloaded app signature.")?;
        if !verify.success() {
            return Err("Downloaded Pui.app failed macOS code-signature verification.".into());
        }
        Ok((staging.clone(), staged_app, current_app_bundle()))
    })();
    if result.is_err() {
        let _ = std::fs::remove_dir_all(&staging);
    }
    result
}

/// Remove any leftover `pui-update-*` staging directories in the system temp folder from previous updates.
pub(super) fn cleanup_stale_staging_dirs() {
    let Ok(entries) = std::fs::read_dir(std::env::temp_dir()) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path
            .file_name()
            .and_then(|n| n.to_str())
            .is_some_and(|name| name.starts_with("pui-update-"))
        {
            let _ = std::fs::remove_dir_all(path);
        }
    }
}

fn spawn_installer_and_relaunch(
    staged_dir: &Path,
    staged_app: &Path,
    target_app: &Path,
) -> Result<(), String> {
    let pid = std::process::id().to_string();
    // Background helper waits for the current Pui process to exit completely,
    // replaces the .app bundle atomically inside target's directory, removes
    // the backup and staging directory, and launches the updated app.
    let script = r#"
pid="$1"
staged_app="$2"
target_app="$3"
staged_dir="$4"
for _ in $(seq 1 600); do
  if ! kill -0 "$pid" 2>/dev/null; then
    break
  fi
  sleep 0.1
done
if kill -0 "$pid" 2>/dev/null; then
  exit 1
fi
backup="${target_app}.previous.$$"
rm -rf "$backup" 2>/dev/null || true
if mv "$target_app" "$backup" && mv "$staged_app" "$target_app"; then
  rm -rf "$backup" "$staged_dir" 2>/dev/null || true
  xattr -dr com.apple.quarantine "$target_app" 2>/dev/null || true
  open "$target_app"
else
  if [[ ! -e "$target_app" && -e "$backup" ]]; then
    mv "$backup" "$target_app" 2>/dev/null || true
  fi
  rm -rf "$staged_dir" 2>/dev/null || true
  exit 1
fi
"#;
    std::process::Command::new("/bin/bash")
        .arg("-c")
        .arg(script)
        .arg("pui-updater")
        .arg(&pid)
        .arg(staged_app)
        .arg(target_app)
        .arg(staged_dir)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
        .map_err(|_| "Could not start the background update installer.")?;
    Ok(())
}

impl Updates {
    pub(super) fn available_version(&self) -> Option<&str> {
        match &self.result {
            Some(Ok(release)) if release.newer => Some(&release.version),
            _ => None,
        }
    }

    pub(super) fn ready_to_install(&self) -> bool {
        matches!(self.install, InstallState::Ready { .. })
    }

    pub(super) fn maybe_auto_check(&mut self, ctx: &egui::Context, enabled: bool, demo: bool) {
        self.auto_check = enabled;
        if !self.cleaned_up && !demo && !cfg!(test) {
            self.cleaned_up = true;
            std::thread::spawn(cleanup_stale_staging_dirs);
        }
        if !enabled || demo || cfg!(test) || self.rx.is_some() {
            return;
        }
        let due = self
            .last_auto_check
            .is_none_or(|last| last.elapsed() >= AUTO_CHECK_INTERVAL);
        if due {
            self.last_auto_check = Some(std::time::Instant::now());
            self.check_quietly(ctx);
        }
    }

    pub(super) fn check_quietly(&mut self, ctx: &egui::Context) {
        if self.rx.is_some() {
            return;
        }
        let Some(repo) = repository() else {
            return;
        };
        let (tx, rx) = mpsc::channel();
        self.rx = Some(rx);
        let ctx = ctx.clone();
        std::thread::spawn(move || {
            let _ = tx.send(check(repo));
            ctx.request_repaint();
        });
    }

    pub(super) fn start(&mut self, ctx: &egui::Context) {
        self.open = true;
        if self.rx.is_some()
            || matches!(
                self.install,
                InstallState::Downloading | InstallState::Ready { .. }
            )
        {
            return;
        }
        self.checked = true;
        self.result = None;
        self.last_auto_check = Some(std::time::Instant::now());
        self.check_quietly(ctx);
    }

    pub(super) fn start_download(&mut self, ctx: &egui::Context) {
        if self.install_rx.is_some() {
            return;
        }
        let Some(repo) = repository() else {
            return;
        };
        let Some(Ok(release)) = &self.result else {
            return;
        };
        let Some(zip_url) = release.zip_url.clone() else {
            self.install = InstallState::Failed(
                "This release does not include a macOS arm64 ZIP archive.".into(),
            );
            return;
        };
        let sums_url = release.sums_url.clone();
        if let InstallState::Ready { staged_dir, .. } = &self.install {
            let _ = std::fs::remove_dir_all(staged_dir);
        }
        self.install = InstallState::Downloading;
        let (tx, rx) = mpsc::channel();
        self.install_rx = Some(rx);
        let ctx = ctx.clone();
        std::thread::spawn(move || {
            let _ = tx.send(download_and_stage(repo, &zip_url, sums_url.as_deref()));
            ctx.request_repaint();
        });
    }

    pub(super) fn cancel_staged(&mut self) {
        if let InstallState::Ready { staged_dir, .. } = &self.install {
            let _ = std::fs::remove_dir_all(staged_dir);
        }
        self.install = InstallState::Idle;
    }

    pub(super) fn poll(&mut self) {
        if let Some(result) = self.rx.as_ref().and_then(|rx| rx.try_recv().ok()) {
            self.rx = None;
            self.checked = true;
            self.result = Some(result);
        }
        if let Some(result) = self.install_rx.as_ref().and_then(|rx| rx.try_recv().ok()) {
            self.install_rx = None;
            self.install = match result {
                Ok((staged_dir, staged_app, target_app)) => InstallState::Ready {
                    staged_dir,
                    staged_app,
                    target_app,
                },
                Err(error) => InstallState::Failed(error),
            };
        }
    }

    fn status(&self, configured: bool) -> (&'static str, String) {
        if !configured {
            return (
                "Updates aren’t configured",
                "This build has no GitHub release repository configured. No network request was made."
                    .to_owned(),
            );
        }
        match &self.install {
            InstallState::Downloading => {
                return (
                    "Downloading update…",
                    "Downloading, verifying SHA-256 checksums and checking code signatures in the background."
                        .to_owned(),
                );
            }
            InstallState::Ready { target_app, .. } => {
                let version = self
                    .result
                    .as_ref()
                    .and_then(|r| r.as_ref().ok())
                    .map(|r| r.version.as_str())
                    .unwrap_or("new");
                let detail = if target_app.is_some() {
                    format!(
                        "Pui {version} is verified and ready. Install & Relaunch will wait for Pui to close, replace the app bundle, clean up temporary files and reopen."
                    )
                } else {
                    format!(
                        "Pui {version} is downloaded and verified. Because this instance isn’t running from a packaged .app bundle, you can reveal the unpacked app in Finder."
                    )
                };
                return ("Update ready to install", detail);
            }
            InstallState::Failed(error) => {
                return ("Couldn’t install update", error.clone());
            }
            InstallState::Idle => {}
        }
        if self.rx.is_some() {
            return (
                "Checking for updates…",
                "Looking for the latest published stable release.".to_owned(),
            );
        }
        match &self.result {
            Some(Ok(release)) if release.newer => (
                "A new version is ready",
                format!("Version {} is available on GitHub.", release.version),
            ),
            Some(Ok(release)) => (
                "You’re up to date",
                format!("Pui {} is the latest available version.", release.version),
            ),
            Some(Err(error)) => ("Couldn’t check for updates", error.clone()),
            None => (
                "Updates on your terms",
                "Check published GitHub releases when you’re ready.".to_owned(),
            ),
        }
    }

    pub(super) fn show(&mut self, ctx: &egui::Context) -> bool {
        let mut auto_check = self.auto_check;
        self.show_with_preference(ctx, &mut auto_check)
    }

    pub(super) fn show_with_preference(
        &mut self,
        ctx: &egui::Context,
        auto_check: &mut bool,
    ) -> bool {
        self.auto_check = *auto_check;
        if !self.open {
            return false;
        }
        let mut retry = false;
        let mut download = false;
        let mut install_now = None;
        let mut reveal_staged = None;
        let mut discard_staged = false;
        let response = prompt_ui::modal(ctx, egui::Id::new("pui-updates"), 360.0, |ui| {
            let mut close = prompt_ui::close(ui);
            prompt_ui::badge(ui, Icon::Refresh, false);
            let (title, description) = self.status(configured());
            prompt_ui::heading(ui, title, "");
            egui::ScrollArea::vertical()
                .id_salt("update-description")
                .max_height((ctx.screen_rect().height() * 0.28).clamp(60.0, 180.0))
                .show(ui, |ui| {
                    ui.add(egui::Label::new(theme::small(ui, &description)).wrap());
                    if let Some(Ok(release)) = &self.result {
                        if release.newer && !release.body.trim().is_empty() {
                            ui.add_space(8.0);
                            ui.add(egui::Label::new(theme::small(ui, release.body.trim())).wrap());
                        }
                    }
                });
            ui.add_space(14.0);
            installed_version(ui);
            ui.add_space(8.0);
            if prompt_ui::compact_toggle_row(ui, auto_check, "Automatically check for updates")
                .changed()
            {
                self.auto_check = *auto_check;
            }
            ui.add_space(6.0);
            ui.add(
                egui::Label::new(theme::small(
                    ui,
                    "Downloads are verified with SHA-256 and codesign. Running agents won’t be interrupted until you choose to relaunch.",
                ))
                .wrap(),
            );
            ui.add_space(18.0);
            prompt_ui::footer(ui, |ui| {
                match &self.install {
                    InstallState::Ready {
                        staged_dir,
                        staged_app,
                        target_app,
                    } => {
                        if let Some(target) = target_app {
                            if prompt_ui::button(
                                ui,
                                "Install & Relaunch",
                                prompt_ui::Button::Primary,
                                true,
                            )
                            .clicked()
                            {
                                install_now =
                                    Some((staged_dir.clone(), staged_app.clone(), target.clone()));
                            }
                        } else if prompt_ui::button(
                            ui,
                            "Reveal in Finder",
                            prompt_ui::Button::Primary,
                            true,
                        )
                        .clicked()
                        {
                            reveal_staged = Some(staged_app.clone());
                        }
                        if prompt_ui::button(
                            ui,
                            "Discard download",
                            prompt_ui::Button::Secondary,
                            true,
                        )
                        .clicked()
                        {
                            discard_staged = true;
                        }
                    }
                    InstallState::Downloading => {}
                    InstallState::Idle | InstallState::Failed(_) => {
                        if let Some(Ok(release)) = &self.result {
                            if release.newer {
                                if release.zip_url.is_some() {
                                    if prompt_ui::button(
                                        ui,
                                        "Download Update",
                                        prompt_ui::Button::Primary,
                                        true,
                                    )
                                    .clicked()
                                    {
                                        download = true;
                                    }
                                    if prompt_ui::button(
                                        ui,
                                        "Release page",
                                        prompt_ui::Button::Secondary,
                                        true,
                                    )
                                    .clicked()
                                    {
                                        ctx.open_url(egui::OpenUrl::new_tab(&release.url));
                                    }
                                } else if prompt_ui::button(
                                    ui,
                                    "Open GitHub release",
                                    prompt_ui::Button::Primary,
                                    true,
                                )
                                .clicked()
                                {
                                    ctx.open_url(egui::OpenUrl::new_tab(&release.url));
                                }
                            }
                        }
                        let release_ready = self.result.as_ref().is_some_and(|result| {
                            result.as_ref().is_ok_and(|release| release.newer)
                        });
                        if configured()
                            && self.rx.is_none()
                            && (!release_ready || matches!(self.install, InstallState::Failed(_)))
                            && prompt_ui::button(
                                ui,
                                if self.checked {
                                    "Check again"
                                } else {
                                    "Check for updates"
                                },
                                prompt_ui::Button::Secondary,
                                true,
                            )
                            .clicked()
                        {
                            retry = true;
                        }
                    }
                }
                close |=
                    prompt_ui::button(ui, "Close", prompt_ui::Button::Secondary, true).clicked();
            });
            close
        });
        if response.should_close() || response.inner {
            self.open = false;
        }
        if discard_staged {
            self.cancel_staged();
        }
        if let Some(app_path) = reveal_staged {
            let _ = std::process::Command::new("/usr/bin/open")
                .arg("-R")
                .arg(app_path)
                .spawn();
        }
        if let Some((staged_dir, staged_app, target_app)) = install_now {
            match spawn_installer_and_relaunch(&staged_dir, &staged_app, &target_app) {
                Ok(()) => {
                    self.open = false;
                    ctx.input_mut(|i| {
                        i.raw
                            .viewports
                            .entry(egui::ViewportId::ROOT)
                            .or_default()
                            .events
                            .push(egui::ViewportEvent::Close)
                    });
                    ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                    return true;
                }
                Err(error) => {
                    self.install = InstallState::Failed(error);
                }
            }
        }
        if download {
            self.start_download(ctx);
        }
        if retry {
            self.install = InstallState::Idle;
            self.start(ctx);
        }
        false
    }
}

// Allocate the dot and label in the same centered row; never top-align the dot.
fn installed_version(ui: &mut egui::Ui) {
    egui::Frame::new()
        .fill(prompt_ui::subtle(ui.ctx()))
        .corner_radius(7)
        .inner_margin(12)
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.horizontal(|ui| {
                let color = theme::palette(ui.ctx()).accent;
                let (rect, _) = ui.allocate_exact_size(Vec2::splat(10.0), Sense::hover());
                theme::status_dot(ui, rect.center(), color, 2.5);
                ui.label(theme::small(
                    ui,
                    format!("Installed version {}", env!("CARGO_PKG_VERSION")),
                ));
            });
        });
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn github_config_and_versions_are_validated_without_trusting_response_urls() {
        for repo in [
            "",
            "owner",
            "a/b/c",
            "../repo",
            "owner/r?token=x",
            "owner/repo;cmd",
        ] {
            assert!(!validate_repository(repo));
        }
        assert!(validate_repository("owner/pui-desktop"));
        assert!(configured());
        let data = json!({
            "tag_name": "v1.10.0",
            "draft": false,
            "prerelease": false,
            "html_url": "https://untrusted.invalid",
            "body": "### Added\n- Self-contained update installer",
            "assets": [
                {
                    "name": "Pui-1.10.0-macos-arm64-adhoc.zip",
                    "browser_download_url": "https://github.com/owner/pui/releases/download/v1.10.0/Pui-1.10.0-macos-arm64-adhoc.zip"
                },
                {
                    "name": "SHA256SUMS",
                    "browser_download_url": "https://github.com/owner/pui/releases/download/v1.10.0/SHA256SUMS"
                },
                {
                    "name": "Pui-1.10.0-macos-arm64.zip",
                    "browser_download_url": "https://untrusted.invalid/Pui-1.10.0-macos-arm64.zip"
                }
            ]
        });
        let r = release("owner/pui", "1.9.0", &data).unwrap();
        assert!(r.newer);
        assert_eq!(r.url, "https://github.com/owner/pui/releases/latest");
        assert_eq!(
            r.zip_url.as_deref(),
            Some(
                "https://github.com/owner/pui/releases/download/v1.10.0/Pui-1.10.0-macos-arm64-adhoc.zip"
            )
        );
        assert_eq!(
            r.sums_url.as_deref(),
            Some("https://github.com/owner/pui/releases/download/v1.10.0/SHA256SUMS")
        );
        assert!(!release("owner/pui", "2.0.0", &data).unwrap().newer);
        assert!(release("owner/pui", "1.0.0", &json!({"tag_name":"v2.0.0-beta.1"})).is_err());
        assert!(
            release(
                "owner/pui",
                "1.0.0",
                &json!({"tag_name":"arbitrary","prerelease":true})
            )
            .is_err()
        );
    }

    #[test]
    fn checksum_parser_and_bundle_detection_are_strict() {
        let sums = "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855  Pui-0.1.0-macos-arm64-adhoc.zip\n";
        assert_eq!(
            parse_expected_sha256(sums, "Pui-0.1.0-macos-arm64-adhoc.zip").as_deref(),
            Some("e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855")
        );
        assert!(parse_expected_sha256(sums, "other.zip").is_none());
        assert!(parse_expected_sha256("not-a-sha  Pui.zip", "Pui.zip").is_none());

        assert_eq!(
            bundle_from_executable(Path::new("/Applications/Pui.app/Contents/MacOS/pui")),
            Some(PathBuf::from("/Applications/Pui.app"))
        );
        assert!(
            bundle_from_executable(Path::new("/Users/user/git/pui/target/debug/pui")).is_none()
        );
    }

    #[test]
    fn all_update_states_have_honest_copy_without_starting_work() {
        let mut updates = Updates::default();
        assert_eq!(updates.status(false).0, "Updates aren’t configured");
        assert_eq!(updates.status(true).0, "Updates on your terms");
        let (_tx, rx) = mpsc::channel();
        updates.rx = Some(rx);
        assert_eq!(updates.status(true).0, "Checking for updates…");
        updates.rx = None;
        for newer in [false, true] {
            updates.result = Some(Ok(Release {
                version: "0.2.0".into(),
                url: "https://github.com/example/pui/releases/latest".into(),
                body: String::new(),
                zip_url: Some("https://github.com/example/pui/releases/download/v0.2.0/Pui-0.2.0-macos-arm64-adhoc.zip".into()),
                sums_url: None,
                newer,
            }));
            assert_eq!(
                updates.status(true).0,
                if newer {
                    "A new version is ready"
                } else {
                    "You’re up to date"
                }
            );
        }
        updates.install = InstallState::Downloading;
        assert_eq!(updates.status(true).0, "Downloading update…");
        updates.install = InstallState::Ready {
            staged_dir: PathBuf::from("/tmp/pui-update-test"),
            staged_app: PathBuf::from("/tmp/pui-update-test/unpacked/Pui.app"),
            target_app: Some(PathBuf::from("/Applications/Pui.app")),
        };
        assert_eq!(updates.status(true).0, "Update ready to install");
        updates.install = InstallState::Failed("Checksum mismatch".into());
        assert_eq!(
            updates.status(true),
            ("Couldn’t install update", "Checksum mismatch".into())
        );
        updates.install = InstallState::Idle;
        updates.result = Some(Err("Connection unavailable".into()));
        assert_eq!(
            updates.status(true),
            (
                "Couldn’t check for updates",
                "Connection unavailable".into()
            )
        );
        assert!(updates.rx.is_none());
    }

    #[test]
    fn installed_version_dot_is_centered_on_the_label_in_both_themes() {
        fn collect(
            shape: &egui::Shape,
            dots: &mut Vec<egui::Pos2>,
            label: &mut Option<egui::Rect>,
        ) {
            match shape {
                egui::Shape::Circle(circle) => dots.push(circle.center),
                egui::Shape::Text(text)
                    if text.galley.job.text.starts_with("Installed version") =>
                {
                    *label = Some(egui::Rect::from_min_size(text.pos, text.galley.size()));
                }
                egui::Shape::Vec(shapes) => {
                    for shape in shapes {
                        collect(shape, dots, label);
                    }
                }
                _ => {}
            }
        }
        for mode in [theme::Theme::Light, theme::Theme::Dark] {
            let ctx = egui::Context::default();
            theme::install(&ctx);
            mode.apply(&ctx);
            let output = ctx.run(Default::default(), |ctx| {
                egui::CentralPanel::default().show(ctx, |ui| {
                    installed_version(ui);
                });
            });
            let mut dots = vec![];
            let mut label = None;
            for shape in output.shapes {
                collect(&shape.shape, &mut dots, &mut label);
            }
            let label = label.expect("version label");
            assert!(
                dots.iter()
                    .any(|dot| (dot.y - label.center().y).abs() < 1.0)
            );
        }
    }

    #[test]
    fn unconfigured_update_check_starts_no_worker() {
        if repository().is_some() {
            return;
        }
        let mut updates = Updates::default();
        updates.start(&egui::Context::default());
        assert!(updates.open);
        assert!(updates.rx.is_none());
    }
}
