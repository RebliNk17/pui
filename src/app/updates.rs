//! GitHub release checks, background auto-check, ZIP download/verification,
//! staged installation, and post-restart temporary cleanup.
use super::*;
mod idle;
mod installer;
pub(super) use idle::Idle;
pub(crate) use installer::run as install_staged_update;
use sha2::{Digest, Sha256};
use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU8, AtomicU64, Ordering};

const DEFAULT_REPOSITORY: &str = "RebliNk17/pui";
const AUTO_CHECK_INTERVAL: Duration = Duration::from_secs(6 * 60 * 60);

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct Release {
    pub version: String,
    pub url: String,
    pub body: String,
    pub zip_url: Option<String>,
    pub sums_url: Option<String>,
    /// Archive size advertised by GitHub; used as the progress total.
    pub zip_size: Option<u64>,
    pub newer: bool,
}

/// Shared between the UI and the download worker. Lock-free so painting never
/// waits on the worker.
#[derive(Default)]
pub(super) struct Progress {
    downloaded: AtomicU64,
    total: AtomicU64,
    phase: AtomicU8,
    cancel: AtomicBool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Phase {
    Downloading,
    Verifying,
    Extracting,
    CheckingSignature,
}

impl Progress {
    fn set_phase(&self, phase: Phase) {
        self.phase.store(phase as u8, Ordering::Relaxed);
    }

    pub(super) fn phase(&self) -> Phase {
        match self.phase.load(Ordering::Relaxed) {
            1 => Phase::Verifying,
            2 => Phase::Extracting,
            3 => Phase::CheckingSignature,
            _ => Phase::Downloading,
        }
    }

    /// Downloaded bytes and known total, if any.
    pub(super) fn bytes(&self) -> (u64, Option<u64>) {
        let total = self.total.load(Ordering::Relaxed);
        (
            self.downloaded.load(Ordering::Relaxed),
            (total > 0).then_some(total),
        )
    }

    fn cancelled(&self) -> bool {
        self.cancel.load(Ordering::Relaxed)
    }
}

const CANCELLED: &str = "Update download cancelled.";

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

type StagedInstall = (PathBuf, PathBuf, Option<PathBuf>);

pub(super) struct Updates {
    pub open: bool,
    pub auto_check: bool,
    result: Option<Result<Release, String>>,
    rx: Option<Receiver<Result<Release, String>>>,
    install: InstallState,
    /// Release belonging to the active download/staged bundle, not a later check.
    install_release: Option<Release>,
    install_rx: Option<Receiver<Result<StagedInstall, String>>>,
    progress: Option<Arc<Progress>>,
    download_started_automatically: bool,
    /// Remember attempts, including explicit downloads, for this app session.
    /// Cancellation, discard and failure must never become an automatic retry.
    attempted_download_versions: std::collections::HashSet<String>,
    install_error: Option<String>,
    last_auto_check: Option<std::time::Instant>,
    checked: bool,
}

impl Default for Updates {
    fn default() -> Self {
        Self {
            open: false,
            auto_check: true,
            result: None,
            rx: None,
            install: InstallState::Idle,
            install_release: None,
            install_rx: None,
            progress: None,
            download_started_automatically: false,
            attempted_download_versions: Default::default(),
            install_error: None,
            last_auto_check: None,
            checked: false,
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
    let mut zip_size = None;
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
                && (zip_url.is_none() || !name.contains("-adhoc."))
            {
                zip_url = Some(download.to_owned());
                zip_size = asset["size"].as_u64().filter(|&size| size > 0);
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
        zip_size,
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

const DISTRIBUTION_BUNDLE_ID: &str = "dev.pi.pui";

fn bundle_identifier(app: &Path) -> Option<String> {
    let output = std::process::Command::new("/usr/libexec/PlistBuddy")
        .args(["-c", "Print :CFBundleIdentifier"])
        .arg(app.join("Contents/Info.plist"))
        .stdin(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .output()
        .ok()?;
    output
        .status
        .success()
        .then(|| String::from_utf8(output.stdout).ok())
        .flatten()
        .map(|id| id.trim().to_owned())
}

fn stable_update_target(app: PathBuf, identifier: Option<&str>) -> Option<PathBuf> {
    (identifier == Some(DISTRIBUTION_BUNDLE_ID)).then_some(app)
}

fn require_distribution_bundle(app: &Path) -> Result<(), String> {
    if bundle_identifier(app).as_deref() != Some(DISTRIBUTION_BUNDLE_ID) {
        return Err("Automatic installation requires the Pui distribution bundle identity. Development or unrecognized bundles cannot be replaced by a stable update.".into());
    }
    Ok(())
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
    Ok(crate::hex::lower(hasher.finalize()))
}

fn download_url_to_file(
    url: &str,
    dest: &Path,
    max_bytes: u64,
    progress: Option<(&Progress, &egui::Context)>,
) -> Result<(), String> {
    let mut child = std::process::Command::new("/usr/bin/curl")
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
        .spawn()
        .map_err(|_| "Cannot start curl to download the update.")?;
    // Poll the partial file's size for progress; curl writes it incrementally.
    let status = loop {
        if let Some(status) = child.try_wait().map_err(|_| "Update download failed.")? {
            break status;
        }
        if let Some((progress, ctx)) = progress {
            if progress.cancelled() {
                let _ = child.kill();
                let _ = child.wait();
                return Err(CANCELLED.into());
            }
            let size = std::fs::metadata(dest).map(|m| m.len()).unwrap_or(0);
            if size != progress.downloaded.swap(size, Ordering::Relaxed) {
                ctx.request_repaint();
            }
        }
        std::thread::sleep(Duration::from_millis(100));
    };
    if let Some((progress, ctx)) = progress {
        if let Ok(meta) = std::fs::metadata(dest) {
            progress.downloaded.store(meta.len(), Ordering::Relaxed);
        }
        ctx.request_repaint();
    }
    if !status.success() {
        return Err("Update download failed. Check your connection and try again.".into());
    }
    Ok(())
}

fn download_and_stage(
    repo: &str,
    zip_url: &str,
    sums_url: Option<&str>,
    progress: &Progress,
    ctx: &egui::Context,
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
        download_url_to_file(zip_url, &zip_path, 600 * 1024 * 1024, Some((progress, ctx)))?;
        let step = |phase| {
            if progress.cancelled() {
                return Err(String::from(CANCELLED));
            }
            progress.set_phase(phase);
            ctx.request_repaint();
            Ok(())
        };
        if let Some(sums_url) = sums_url {
            step(Phase::Verifying)?;
            let sums_path = staging.join("SHA256SUMS");
            download_url_to_file(sums_url, &sums_path, 1024 * 1024, None)?;
            let sums_text =
                std::fs::read_to_string(&sums_path).map_err(|_| "Cannot read update checksums.")?;
            let expected = parse_expected_sha256(&sums_text, filename)
                .ok_or("Release archive is missing from SHA256SUMS.")?;
            let actual = sha256_file(&zip_path)?;
            if actual != expected {
                return Err("Downloaded update failed SHA-256 checksum verification.".into());
            }
        }
        step(Phase::Extracting)?;
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
        require_distribution_bundle(&staged_app)?;
        step(Phase::CheckingSignature)?;
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
        if progress.cancelled() {
            return Err(CANCELLED.into());
        }
        let target_app = current_app_bundle().and_then(|app| {
            let identifier = bundle_identifier(&app);
            stable_update_target(app, identifier.as_deref())
        });
        Ok((staging.clone(), staged_app, target_app))
    })();
    if result.is_err() {
        let _ = std::fs::remove_dir_all(&staging);
    }
    result
}

fn spawn_installer_and_relaunch(
    staged_dir: &Path,
    staged_app: &Path,
    target_app: &Path,
) -> Result<(), String> {
    installer::spawn(staged_dir, staged_app, target_app)
}

impl Updates {
    fn displayed_release(&self) -> Option<&Release> {
        self.install_release
            .as_ref()
            .or_else(|| self.result.as_ref()?.as_ref().ok())
    }

    pub(super) fn available_version(&self) -> Option<&str> {
        self.displayed_release()
            .filter(|release| release.newer)
            .map(|release| release.version.as_str())
    }

    fn can_check(&self) -> bool {
        configured() && self.rx.is_none() && !matches!(self.install, InstallState::Downloading)
    }

    pub(super) fn ready_to_install(&self) -> bool {
        matches!(self.install, InstallState::Ready { .. })
    }

    pub(super) fn maybe_auto_check(&mut self, ctx: &egui::Context, enabled: bool, demo: bool) {
        self.auto_check = enabled;
        // Staging is removed by its owning worker/helper, discard or supersede.
        // Never sweep peers' pui-update-* directories during a live startup.
        if !enabled || demo || cfg!(test) || !self.can_check() {
            return;
        }
        let due = self
            .last_auto_check
            .is_none_or(|last| last.elapsed() >= AUTO_CHECK_INTERVAL);
        if due {
            self.check_quietly(ctx);
        }
    }

    /// Called after polling, without opening the Updates dialog. Automatic
    /// network activity is always suppressed in demo builds and unit tests.
    pub(super) fn maybe_auto_download(
        &mut self,
        ctx: &egui::Context,
        settings: &mut settings::Settings,
        demo: bool,
    ) {
        self.apply_download_preference(settings);
        if demo || cfg!(test) || !self.claim_auto_download(settings.auto_download_updates) {
            return;
        }
        self.begin_download(ctx, true);
    }

    pub(super) fn apply_download_preference(&mut self, settings: &mut settings::Settings) {
        settings.normalize_update_preferences();
        if !settings.auto_download_updates && self.download_started_automatically {
            self.cancel_download();
        }
    }

    /// Claim before starting any worker so every outcome consumes the attempt.
    fn claim_auto_download(&mut self, enabled: bool) -> bool {
        if !enabled
            || !configured()
            || self.rx.is_some()
            || self.install_rx.is_some()
            || matches!(
                self.install,
                InstallState::Downloading | InstallState::Ready { .. }
            )
        {
            return false;
        }
        let Some(Ok(release)) = &self.result else {
            return false;
        };
        release.newer
            && release.zip_url.is_some()
            && self
                .attempted_download_versions
                .insert(release.version.clone())
    }

    pub(super) fn automatic_install_ready(&self) -> bool {
        self.rx.is_none()
            && self.install_rx.is_none()
            && self.install_error.is_none()
            && self
                .install_release
                .as_ref()
                .is_some_and(|release| release.newer)
            && matches!(
                self.install,
                InstallState::Ready {
                    target_app: Some(_),
                    ..
                }
            )
    }

    /// Installation failures (including a failed save) do not discard a
    /// verified bundle. A user can explicitly retry after addressing the error.
    pub(super) fn fail_install(&mut self, error: String) {
        self.install_error = Some(error);
        self.open = true;
    }

    /// The owner calls this only after saving and checking its lifecycle guards.
    /// Renderers merely request installation; they never close the app.
    pub(super) fn attempt_install(&mut self, ctx: &egui::Context) -> bool {
        if self.rx.is_some() || self.install_rx.is_some() {
            return false;
        }
        let InstallState::Ready {
            staged_dir,
            staged_app,
            target_app: Some(target_app),
        } = &self.install
        else {
            return false;
        };
        match spawn_installer_and_relaunch(staged_dir, staged_app, target_app) {
            Ok(()) => {
                self.open = false;
                self.install_error = None;
                ctx.input_mut(|i| {
                    i.raw
                        .viewports
                        .entry(egui::ViewportId::ROOT)
                        .or_default()
                        .events
                        .push(egui::ViewportEvent::Close)
                });
                ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                true
            }
            Err(error) => {
                self.fail_install(error);
                false
            }
        }
    }

    pub(super) fn check_quietly(&mut self, ctx: &egui::Context) {
        if !self.can_check() {
            return;
        }
        let Some(repo) = repository() else {
            return;
        };
        self.checked = true;
        self.last_auto_check = Some(std::time::Instant::now());
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
        self.check_quietly(ctx);
    }

    pub(super) fn start_download(&mut self, ctx: &egui::Context) {
        self.begin_download(ctx, false);
    }

    fn begin_download(&mut self, ctx: &egui::Context, automatic: bool) {
        if self.install_rx.is_some() || self.rx.is_some() {
            return;
        }
        let Some(repo) = repository() else {
            return;
        };
        let Some(Ok(release)) = &self.result else {
            return;
        };
        if !release.newer {
            return;
        }
        self.attempted_download_versions
            .insert(release.version.clone());
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
        self.install_release = Some(release.clone());
        self.install_error = None;
        self.download_started_automatically = automatic;
        self.install = InstallState::Downloading;
        let progress = Arc::new(Progress::default());
        progress
            .total
            .store(release.zip_size.unwrap_or(0), Ordering::Relaxed);
        self.progress = Some(progress.clone());
        let (tx, rx) = mpsc::channel();
        self.install_rx = Some(rx);
        let ctx = ctx.clone();
        std::thread::spawn(move || {
            let _ = tx.send(download_and_stage(
                repo,
                &zip_url,
                sums_url.as_deref(),
                &progress,
                &ctx,
            ));
            ctx.request_repaint();
        });
    }

    /// Ask the worker to stop; it kills curl, removes staging and reports back.
    pub(super) fn cancel_download(&mut self) {
        if let Some(progress) = &self.progress {
            progress.cancel.store(true, Ordering::Relaxed);
        }
    }

    pub(super) fn cancel_staged(&mut self) {
        if let InstallState::Ready { staged_dir, .. } = &self.install {
            let _ = std::fs::remove_dir_all(staged_dir);
        }
        self.install = InstallState::Idle;
        self.install_release = None;
        self.install_error = None;
    }

    pub(super) fn poll(&mut self) {
        if let Some(result) = self.rx.as_ref().and_then(|rx| rx.try_recv().ok()) {
            self.rx = None;
            self.checked = true;
            if let (Ok(latest), Some(staged)) = (&result, &self.install_release) {
                let superseded = semver::Version::parse(&latest.version)
                    .ok()
                    .zip(semver::Version::parse(&staged.version).ok())
                    .is_some_and(|(latest, staged)| latest > staged);
                if superseded && self.ready_to_install() {
                    self.cancel_staged();
                }
            }
            self.result = Some(result);
        }
        if let Some(result) = self.install_rx.as_ref().and_then(|rx| rx.try_recv().ok()) {
            self.install_rx = None;
            self.download_started_automatically = false;
            let cancelled = self.progress.take().is_some_and(|p| p.cancelled());
            self.install = match result {
                Ok((staged_dir, ..)) if cancelled => {
                    let _ = std::fs::remove_dir_all(staged_dir);
                    InstallState::Idle
                }
                Err(_) if cancelled => InstallState::Idle,
                Ok((staged_dir, staged_app, target_app)) => InstallState::Ready {
                    staged_dir,
                    staged_app,
                    target_app,
                },
                Err(error) => InstallState::Failed(error),
            };
            if !self.ready_to_install() {
                self.install_release = None;
            }
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
        if self.rx.is_some() {
            return (
                "Checking for updates…",
                "Looking for the latest published stable release.".to_owned(),
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
                    .displayed_release()
                    .map(|r| r.version.as_str())
                    .unwrap_or("new");
                let mut detail = if target_app.is_some() {
                    format!(
                        "Pui {version} is verified and ready. Install & Relaunch will wait for Pui to close, replace the app bundle, clean up temporary files and reopen."
                    )
                } else {
                    format!(
                        "Pui {version} is downloaded and verified. Because this instance isn’t running from a Pui distribution bundle (it may be Pui Dev or an unpackaged build), automatic replacement is disabled. You can reveal the unpacked app in Finder."
                    )
                };
                if let Some(Err(error)) = &self.result {
                    detail.push_str(&format!(" Latest update check failed: {error}"));
                }
                if let Some(error) = &self.install_error {
                    detail.push_str(&format!(" Couldn’t install: {error}"));
                }
                return ("Update ready to install", detail);
            }
            InstallState::Failed(error) => {
                return ("Couldn’t install update", error.clone());
            }
            InstallState::Idle => {}
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

    #[cfg(test)]
    pub(super) fn show(&mut self, ctx: &egui::Context) -> bool {
        let mut settings = settings::Settings {
            auto_check_updates: self.auto_check,
            ..Default::default()
        };
        self.show_with_preference(ctx, &mut settings)
    }

    pub(super) fn show_with_preference(
        &mut self,
        ctx: &egui::Context,
        settings: &mut settings::Settings,
    ) -> bool {
        self.auto_check = settings.auto_check_updates;
        self.apply_download_preference(settings);
        if !self.open {
            return false;
        }
        let mut retry = false;
        let mut download = false;
        let mut install_now = false;
        let mut reveal_staged = None;
        let mut discard_staged = false;
        let mut cancel_download = false;
        let response = prompt_ui::modal(ctx, egui::Id::new("pui-updates"), 360.0, |ui| {
            let mut close = prompt_ui::close(ui);
            prompt_ui::badge(ui, Icon::Refresh, false);
            let (title, description) = self.status(configured());
            prompt_ui::heading(ui, title, "");
            egui::ScrollArea::vertical()
                .id_salt("update-description")
                // Keep actions visible at narrow sizes and large interface zoom.
                .max_height((ctx.screen_rect().height() - 380.0).clamp(45.0, 380.0))
                .show(ui, |ui| {
                    ui.add(egui::Label::new(theme::small(ui, &description)).wrap());
                    if let Some(release) = self.displayed_release() {
                        if release.newer && !release.body.trim().is_empty() {
                            ui.add_space(8.0);
                            // Release notes are the changelog section: render its Markdown.
                            crate::markdown::show(ui, release.body.trim());
                        }
                    }
                    if let (InstallState::Downloading, Some(progress)) =
                        (&self.install, &self.progress)
                    {
                        ui.add_space(14.0);
                        cancel_download = download_progress(ui, progress);
                    }
                    ui.add_space(14.0);
                    installed_version(ui);
                    ui.add_space(8.0);
                    update_preferences(ui, settings);
                    self.auto_check = settings.auto_check_updates;
                });
            ui.add_space(18.0);
            if !matches!(self.install, InstallState::Downloading) {
                prompt_ui::footer(ui, |ui| {
                    retry = prompt_ui::button(
                        ui,
                        if self.rx.is_some() {
                            "Checking…"
                        } else if self.checked {
                            "Check again"
                        } else {
                            "Check for updates"
                        },
                        prompt_ui::Button::Secondary,
                        self.can_check(),
                    )
                    .clicked();
                });
                ui.add_space(6.0);
            }
            if let InstallState::Ready {
                staged_app,
                target_app,
                ..
            } = &self.install
            {
                prompt_ui::footer(ui, |ui| {
                    if target_app.is_some() {
                        install_now = prompt_ui::button(
                            ui,
                            "Install & Relaunch",
                            prompt_ui::Button::Primary,
                            self.rx.is_none() && self.install_rx.is_none(),
                        )
                        .clicked();
                    } else if prompt_ui::button(
                        ui,
                        "Reveal in Finder",
                        prompt_ui::Button::Primary,
                        self.rx.is_none(),
                    )
                    .clicked()
                    {
                        reveal_staged = Some(staged_app.clone());
                    }
                });
                ui.add_space(6.0);
            } else if matches!(self.install, InstallState::Idle | InstallState::Failed(_))
                && self.result.as_ref().is_some_and(|result| {
                    result
                        .as_ref()
                        .is_ok_and(|release| release.newer && release.zip_url.is_some())
                })
            {
                prompt_ui::footer(ui, |ui| {
                    download = prompt_ui::button(
                        ui,
                        "Download Update",
                        prompt_ui::Button::Primary,
                        self.rx.is_none(),
                    )
                    .clicked();
                });
                ui.add_space(6.0);
            }
            prompt_ui::footer(ui, |ui| {
                match &self.install {
                    InstallState::Ready { .. } => {
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
                    }
                }
                close |=
                    prompt_ui::button(ui, "Close", prompt_ui::Button::Secondary, true).clicked();
            });
            close
        });
        self.apply_download_preference(settings);
        if response.should_close() || response.inner {
            self.open = false;
        }
        if discard_staged {
            self.cancel_staged();
        }
        if cancel_download {
            self.cancel_download();
        }
        if let Some(app_path) = reveal_staged {
            let _ = std::process::Command::new("/usr/bin/open")
                .arg("-R")
                .arg(app_path)
                .spawn();
        }
        if download {
            self.start_download(ctx);
        }
        if retry {
            if matches!(self.install, InstallState::Failed(_)) {
                self.install = InstallState::Idle;
            }
            self.start(ctx);
        }
        install_now
    }
}

/// Shared Quiet Mac controls for Settings and Updates. The dependency is
/// normalized before and after rendering, so re-enabling downloads never
/// silently opts the user back into installation.
pub(super) fn update_preferences(ui: &mut egui::Ui, settings: &mut settings::Settings) {
    settings.normalize_update_preferences();
    let narrow = ui.available_width() < 280.0;
    prompt_ui::compact_toggle_row(
        ui,
        &mut settings.auto_check_updates,
        if narrow {
            "Automatically check"
        } else {
            "Automatically check for updates"
        },
    );
    prompt_ui::compact_toggle_row(
        ui,
        &mut settings.auto_download_updates,
        if narrow {
            "Automatically download"
        } else {
            "Automatically download updates"
        },
    );
    settings.normalize_update_preferences();
    ui.add_enabled_ui(settings.auto_download_updates, |ui| {
        prompt_ui::compact_toggle_row(
            ui,
            &mut settings.auto_install_updates,
            if narrow {
                "Automatically install"
            } else {
                "Automatically install updates"
            },
        );
    });
    ui.add_space(6.0);
    ui.add(egui::Label::new(theme::small(ui,
        "Downloads are verified with SHA-256 and codesign. Optional auto-install relaunches only when all agents are sleeping and you’ve been inactive for 10 minutes, even with Pui in the foreground. Otherwise, use Install & Relaunch.",
    )).wrap());
}

fn format_mb(bytes: u64) -> String {
    let mb = bytes as f64 / (1024.0 * 1024.0);
    if mb < 10.0 {
        format!("{mb:.1} MB")
    } else {
        format!("{mb:.0} MB")
    }
}

/// Label and determinate fraction (None = indeterminate) for the progress row.
fn progress_text(progress: &Progress) -> (String, Option<f32>) {
    if progress.cancelled() {
        return ("Cancelling…".into(), None);
    }
    match progress.phase() {
        Phase::Downloading => match progress.bytes() {
            (done, Some(total)) => {
                let fraction = (done as f64 / total as f64).clamp(0.0, 1.0);
                (
                    format!(
                        "Downloading {} of {} · {}%",
                        format_mb(done.min(total)),
                        format_mb(total),
                        (fraction * 100.0).floor() as u32
                    ),
                    Some(fraction as f32),
                )
            }
            (0, None) => ("Starting download…".into(), None),
            (done, None) => (format!("Downloading {}…", format_mb(done)), None),
        },
        Phase::Verifying => ("Verifying checksum…".into(), None),
        Phase::Extracting => ("Extracting…".into(), None),
        Phase::CheckingSignature => ("Checking code signature…".into(), None),
    }
}

/// Progress label, bar and cancel action. Returns true when cancel is clicked.
fn download_progress(ui: &mut egui::Ui, progress: &Progress) -> bool {
    let (label, fraction) = progress_text(progress);
    let colors = theme::palette(ui.ctx());
    let mut cancel = false;
    ui.horizontal(|ui| {
        ui.vertical(|ui| {
            ui.set_width((ui.available_width() - 38.0).max(80.0));
            ui.label(theme::small(ui, &label).color(colors.muted));
            ui.add_space(6.0);
            let (rect, response) =
                ui.allocate_exact_size(egui::vec2(ui.available_width(), 4.0), Sense::hover());
            response.widget_info(|| {
                egui::WidgetInfo::labeled(egui::WidgetType::ProgressIndicator, true, &label)
            });
            let painter = ui.painter_at(rect);
            painter.rect_filled(rect, 2, prompt_ui::border(ui.ctx()));
            let fill = match fraction {
                Some(fraction) => egui::Rect::from_min_size(
                    rect.min,
                    egui::vec2(rect.width() * fraction, rect.height()),
                ),
                None => {
                    // Sliding segment for phases without a measurable total.
                    let segment = rect.width() * 0.3;
                    let t = (ui.input(|i| i.time) * 0.8).fract() as f32;
                    let x = rect.left() - segment + t * (rect.width() + segment);
                    ui.ctx().request_repaint_after(Duration::from_millis(16));
                    egui::Rect::from_min_size(
                        egui::pos2(x, rect.top()),
                        egui::vec2(segment, rect.height()),
                    )
                }
            };
            painter.rect_filled(fill, 2, colors.accent);
        });
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            cancel =
                prompt_ui::icon_action(ui, Icon::Close, "Cancel download", !progress.cancelled())
                    .clicked();
        });
    });
    cancel
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

    fn test_release(version: &str) -> Release {
        release(
            "owner/pui",
            "1.0.0",
            &json!({
                "tag_name": format!("v{version}"),
                "draft": false,
                "prerelease": false,
                "body": format!("Release notes for {version}"),
            }),
        )
        .unwrap()
    }

    fn staged_update() -> Updates {
        let staged_dir = std::env::temp_dir().join(format!("pui-update-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&staged_dir).unwrap();
        std::fs::write(staged_dir.join("staged-marker"), "1.0.1").unwrap();
        let staged_app = staged_dir.join("unpacked/Pui.app");
        Updates {
            open: true,
            checked: true,
            result: Some(Ok(test_release("1.0.1"))),
            install_release: Some(test_release("1.0.1")),
            install: InstallState::Ready {
                staged_dir,
                staged_app,
                target_app: Some(PathBuf::from("/Applications/Pui.app")),
            },
            ..Default::default()
        }
    }

    fn pending_check(updates: &mut Updates) -> mpsc::Sender<Result<Release, String>> {
        let (tx, rx) = mpsc::channel();
        updates.rx = Some(rx);
        tx
    }

    fn downloadable(version: &str) -> Release {
        Release {
            zip_url: Some(format!(
                "https://github.com/owner/pui/releases/download/v{version}/Pui-macos-arm64.zip"
            )),
            ..test_release(version)
        }
    }

    #[test]
    fn automatic_download_claims_each_version_once_without_opening_dialog() {
        let mut updates = Updates::default();
        let tx = pending_check(&mut updates);
        tx.send(Ok(downloadable("1.0.2"))).unwrap();
        assert!(!updates.claim_auto_download(true), "wait for polling");
        updates.poll();
        assert!(!updates.claim_auto_download(false));
        assert!(updates.claim_auto_download(true));
        assert!(
            !updates.open,
            "background availability must not open a modal"
        );
        // Cancellation, discard, failure and explicit checks must not create a
        // per-frame or preference-toggle retry loop for this release.
        for state in [InstallState::Idle, InstallState::Failed("offline".into())] {
            updates.install = state;
            assert!(!updates.claim_auto_download(true));
            assert!(!updates.claim_auto_download(false));
            assert!(!updates.claim_auto_download(true));
        }
        pending_check(&mut updates)
            .send(Ok(downloadable("1.0.2")))
            .unwrap();
        updates.poll();
        assert!(!updates.claim_auto_download(true));
        pending_check(&mut updates)
            .send(Ok(downloadable("1.0.3")))
            .unwrap();
        updates.poll();
        assert!(updates.claim_auto_download(true));
    }

    #[test]
    fn cancelled_and_discarded_downloads_do_not_restart_automatically() {
        let mut updates = Updates {
            result: Some(Ok(downloadable("1.0.2"))),
            ..Default::default()
        };
        assert!(updates.claim_auto_download(true));
        let (tx, rx) = mpsc::channel();
        let progress = Arc::new(Progress::default());
        updates.install_rx = Some(rx);
        updates.progress = Some(progress.clone());
        updates.install = InstallState::Downloading;
        updates.download_started_automatically = true;
        updates.cancel_download();
        tx.send(Err(CANCELLED.into())).unwrap();
        updates.poll();
        assert!(progress.cancelled());
        assert!(!updates.download_started_automatically);
        assert_eq!(updates.install, InstallState::Idle);
        assert!(!updates.claim_auto_download(true));

        let mut updates = staged_update();
        updates.attempted_download_versions.insert("1.0.1".into());
        updates.result = Some(Ok(downloadable("1.0.1")));
        updates.cancel_staged();
        assert!(!updates.claim_auto_download(true));
    }

    #[test]
    fn automatic_download_policy_waits_for_workers_and_valid_newer_assets() {
        let mut updates = Updates::default();
        assert!(!updates.claim_auto_download(true));
        updates.result = Some(Err("offline".into()));
        assert!(!updates.claim_auto_download(true));
        updates.result = Some(Ok(test_release("1.0.2")));
        assert!(!updates.claim_auto_download(true), "no ZIP");
        updates.result = Some(Ok(downloadable("1.0.0")));
        assert!(!updates.claim_auto_download(true), "not newer");
        updates.result = Some(Ok(downloadable("1.0.2")));
        let _tx = pending_check(&mut updates);
        assert!(!updates.claim_auto_download(true));
        updates.rx = None;
        let (_tx, rx) = mpsc::channel();
        updates.install_rx = Some(rx);
        assert!(!updates.claim_auto_download(true));
        updates.install_rx = None;
        updates.install = InstallState::Downloading;
        assert!(!updates.claim_auto_download(true));
        updates.install = InstallState::Ready {
            staged_dir: PathBuf::new(),
            staged_app: PathBuf::new(),
            target_app: None,
        };
        assert!(!updates.claim_auto_download(true));
        updates.install = InstallState::Idle;
        assert!(updates.claim_auto_download(true));
    }

    #[test]
    fn explicit_download_attempts_suppress_auto_retry_but_not_explicit_retry() {
        let mut updates = Updates {
            result: Some(Ok(test_release("1.0.2"))),
            ..Default::default()
        };
        // A missing ZIP fails synchronously: no worker or real network.
        updates.start_download(&egui::Context::default());
        assert!(matches!(updates.install, InstallState::Failed(_)));
        updates.result = Some(Ok(downloadable("1.0.2")));
        assert!(!updates.claim_auto_download(true));
        // Explicit retries are independent of the automatic attempt claim.
        updates.result = Some(Ok(test_release("1.0.2")));
        updates.install = InstallState::Idle;
        updates.start_download(&egui::Context::default());
        assert!(matches!(updates.install, InstallState::Failed(_)));
        updates.result = Some(Ok(downloadable("1.0.0")));
        updates.install = InstallState::Idle;
        updates.start_download(&egui::Context::default());
        assert_eq!(
            updates.install,
            InstallState::Idle,
            "never download a non-newer release"
        );
        assert!(updates.install_rx.is_none());
    }

    #[test]
    fn automatic_network_is_suppressed_in_tests_and_demo() {
        for demo in [false, true] {
            let mut updates = Updates {
                result: Some(Ok(downloadable("1.0.2"))),
                ..Default::default()
            };
            let mut settings = settings::Settings::default();
            let ctx = egui::Context::default();
            updates.maybe_auto_check(&ctx, true, demo);
            updates.maybe_auto_download(&ctx, &mut settings, demo);
            assert!(updates.rx.is_none());
            assert!(updates.install_rx.is_none());
            assert!(updates.attempted_download_versions.is_empty());
            assert_eq!(updates.install, InstallState::Idle);
            assert!(!updates.open);
        }
    }

    #[test]
    fn disabling_downloads_cancels_only_automatic_workers_and_revokes_install() {
        for automatic in [false, true] {
            let progress = Arc::new(Progress::default());
            let mut updates = Updates {
                install: InstallState::Downloading,
                progress: Some(progress.clone()),
                download_started_automatically: automatic,
                ..Default::default()
            };
            let mut settings = settings::Settings {
                auto_download_updates: false,
                auto_install_updates: true,
                ..Default::default()
            };
            updates.apply_download_preference(&mut settings);
            assert_eq!(progress.cancelled(), automatic);
            assert!(!settings.auto_install_updates);
            settings.auto_download_updates = true;
            updates.apply_download_preference(&mut settings);
            assert!(!settings.auto_install_updates);
            assert_eq!(progress.cancelled(), automatic, "never undo cancellation");
        }
    }

    #[test]
    fn install_failures_preserve_verified_bundle_and_block_automatic_retry() {
        let mut updates = staged_update();
        let expected = updates.install.clone();
        assert!(updates.automatic_install_ready());
        let _tx = pending_check(&mut updates);
        assert!(!updates.automatic_install_ready());
        assert!(!updates.attempt_install(&egui::Context::default()));
        updates.rx = None;
        updates.fail_install("Could not save preferences".into());
        assert!(!updates.automatic_install_ready());
        assert!(updates.open);
        assert_eq!(updates.install, expected);
        assert_eq!(updates.install_release.as_ref().unwrap().version, "1.0.1");
        if let InstallState::Ready { staged_dir, .. } = &updates.install {
            assert!(staged_dir.join("staged-marker").exists());
        }
        assert!(
            updates
                .status(true)
                .1
                .contains("Could not save preferences")
        );
        updates.cancel_staged();
        assert!(updates.install_error.is_none());
    }

    #[test]
    fn newer_check_replaces_stale_staged_offer_and_removes_only_its_download() {
        let mut updates = staged_update();
        let staged_dir = match &updates.install {
            InstallState::Ready { staged_dir, .. } => staged_dir.clone(),
            _ => unreachable!(),
        };
        assert!(updates.can_check());
        let tx = pending_check(&mut updates);
        assert!(!updates.can_check());
        assert_eq!(updates.status(true).0, "Checking for updates…");
        // Opening again while checking reuses the pending worker.
        updates.start(&egui::Context::default());
        tx.send(Ok(test_release("1.0.2"))).unwrap();
        updates.poll();
        assert!(!staged_dir.exists());
        assert_eq!(updates.install, InstallState::Idle);
        assert!(updates.install_release.is_none());
        assert_eq!(updates.available_version(), Some("1.0.2"));
        assert_eq!(
            updates.displayed_release().unwrap().body,
            "Release notes for 1.0.2"
        );
        assert_eq!(updates.status(true).0, "A new version is ready");
        assert!(updates.can_check());
    }

    #[test]
    fn failed_equal_or_older_checks_preserve_the_staged_version_and_notes() {
        for result in [
            Err("Connection unavailable".into()),
            Ok(test_release("1.0.1")),
            Ok(test_release("1.0.0")),
        ] {
            let mut updates = staged_update();
            let expected = updates.install.clone();
            let failed = result.is_err();
            pending_check(&mut updates).send(result).unwrap();
            updates.poll();
            assert_eq!(updates.install, expected);
            assert_eq!(updates.available_version(), Some("1.0.1"));
            assert_eq!(
                updates.displayed_release().unwrap().body,
                "Release notes for 1.0.1"
            );
            let (title, detail) = updates.status(true);
            assert_eq!(title, "Update ready to install");
            assert!(detail.contains("Pui 1.0.1"));
            assert_eq!(detail.contains("Latest update check failed"), failed);
            updates.cancel_staged();
            assert!(updates.install_release.is_none());
        }
    }

    #[test]
    fn checks_and_downloads_do_not_overlap_and_cancellation_clears_version() {
        let mut updates = Updates {
            install: InstallState::Downloading,
            install_release: Some(test_release("1.0.1")),
            ..Default::default()
        };
        assert!(!updates.can_check());
        updates.start(&egui::Context::default());
        assert!(updates.rx.is_none());
        let (tx, rx) = mpsc::channel();
        updates.install_rx = Some(rx);
        let progress = Arc::new(Progress::default());
        progress.cancel.store(true, Ordering::Relaxed);
        updates.progress = Some(progress);
        tx.send(Err(CANCELLED.into())).unwrap();
        updates.poll();
        assert_eq!(updates.install, InstallState::Idle);
        assert!(updates.install_release.is_none());
        assert!(updates.can_check());
        let _tx = pending_check(&mut updates);
        updates.start_download(&egui::Context::default());
        assert!(updates.install_rx.is_none());
    }

    fn text_rects(shape: &egui::Shape, out: &mut Vec<(String, egui::Rect)>) {
        match shape {
            egui::Shape::Text(text) => out.push((
                text.galley.job.text.clone(),
                egui::Rect::from_min_size(text.pos, text.galley.size()),
            )),
            egui::Shape::Vec(shapes) => shapes.iter().for_each(|s| text_rects(s, out)),
            _ => {}
        }
    }

    fn pointer_event(pos: egui::Pos2, pressed: bool) -> Vec<egui::Event> {
        vec![
            egui::Event::PointerMoved(pos),
            egui::Event::PointerButton {
                pos,
                button: egui::PointerButton::Primary,
                pressed,
                modifiers: egui::Modifiers::NONE,
            },
        ]
    }

    #[test]
    fn shared_update_controls_fit_and_disabled_install_cannot_restore_consent() {
        for mode in [theme::Theme::Light, theme::Theme::Dark] {
            for zoom in [0.8, 1.0, 1.4] {
                let ctx = egui::Context::default();
                theme::install(&ctx);
                mode.apply(&ctx);
                ctx.set_zoom_factor(zoom);
                let mut settings = settings::Settings {
                    auto_download_updates: false,
                    auto_install_updates: true,
                    ..Default::default()
                };
                let render = |settings: &mut settings::Settings, events| {
                    ctx.run(
                        egui::RawInput {
                            screen_rect: Some(egui::Rect::from_min_size(
                                egui::Pos2::ZERO,
                                Vec2::new(320.0, 500.0),
                            )),
                            events,
                            ..Default::default()
                        },
                        |ctx| {
                            egui::CentralPanel::default()
                                .show(ctx, |ui| update_preferences(ui, settings));
                        },
                    )
                };
                let mut labels = vec![];
                for _ in 0..4 {
                    let output = render(&mut settings, vec![]);
                    labels.clear();
                    for shape in output.shapes {
                        text_rects(&shape.shape, &mut labels);
                    }
                }
                let install = labels
                    .iter()
                    .find(|(text, _)| text.starts_with("Automatically install"))
                    .unwrap()
                    .1;
                for (_, rect) in &labels {
                    assert!(ctx.screen_rect().contains_rect(*rect), "{labels:?}");
                }
                assert!(!settings.auto_install_updates);
                for pressed in [true, false] {
                    render(&mut settings, pointer_event(install.center(), pressed));
                }
                assert!(
                    !settings.auto_install_updates,
                    "disabled label must not toggle"
                );
                settings.auto_download_updates = true;
                render(&mut settings, vec![]);
                assert!(
                    !settings.auto_install_updates,
                    "download enable must not restore install"
                );
                for pressed in [true, false] {
                    render(&mut settings, pointer_event(install.center(), pressed));
                }
                assert!(
                    settings.auto_install_updates,
                    "explicit enabled opt-in works"
                );
            }
        }
    }

    #[test]
    fn manual_install_is_only_a_request_and_modal_cancellation_preserves_staging() {
        for mode in [theme::Theme::Light, theme::Theme::Dark] {
            for zoom in [0.8, 1.0, 1.4] {
                let ctx = egui::Context::default();
                theme::install(&ctx);
                mode.apply(&ctx);
                ctx.set_zoom_factor(zoom);
                let mut updates = staged_update();
                let mut settings = settings::Settings::default();
                let mut requested = false;
                let mut render =
                    |updates: &mut Updates, settings: &mut settings::Settings, events| {
                        ctx.run(
                            egui::RawInput {
                                screen_rect: Some(egui::Rect::from_min_size(
                                    egui::Pos2::ZERO,
                                    Vec2::new(420.0, 620.0),
                                )),
                                events,
                                ..Default::default()
                            },
                            |ctx| {
                                requested |= updates.show_with_preference(ctx, settings);
                            },
                        )
                    };
                let mut labels = vec![];
                for _ in 0..4 {
                    let output = render(&mut updates, &mut settings, vec![]);
                    labels.clear();
                    for shape in output.shapes {
                        text_rects(&shape.shape, &mut labels);
                    }
                }
                let modal = ctx
                    .read_response(egui::Id::new("pui-updates"))
                    .unwrap()
                    .rect;
                assert!(ctx.screen_rect().contains_rect(modal), "{modal:?}");
                let install = labels
                    .iter()
                    .find(|(text, _)| text == "Install & Relaunch")
                    .unwrap()
                    .1;
                assert!(ctx.screen_rect().contains_rect(install));
                for pressed in [true, false] {
                    let output = render(
                        &mut updates,
                        &mut settings,
                        pointer_event(install.center(), pressed),
                    );
                    assert!(!output.viewport_output.values().any(|v| {
                        v.commands
                            .iter()
                            .any(|c| matches!(c, egui::ViewportCommand::Close))
                    }));
                }
                assert!(
                    updates.open,
                    "request must not close the modal or spawn installer"
                );
                assert!(updates.ready_to_install());
                // Backdrop cancellation is a close-only operation, never an install.
                let outside = egui::pos2(2.0, 2.0);
                for pressed in [true, false] {
                    render(&mut updates, &mut settings, pointer_event(outside, pressed));
                }
                assert!(!updates.open);
                assert!(updates.ready_to_install());
                assert!(requested);
                updates.cancel_staged();
            }
        }
    }

    #[test]
    fn staged_refresh_actions_fit_themes_zoom_and_disable_install_during_check() {
        fn collect(shape: &egui::Shape, out: &mut Vec<(String, egui::Rect)>) {
            match shape {
                egui::Shape::Text(text) => out.push((
                    text.galley.job.text.clone(),
                    egui::Rect::from_min_size(text.pos, text.galley.size()),
                )),
                egui::Shape::Vec(shapes) => shapes.iter().for_each(|s| collect(s, out)),
                _ => {}
            }
        }
        for mode in [theme::Theme::Light, theme::Theme::Dark] {
            for zoom in [0.8, 1.0, 1.4] {
                let ctx = egui::Context::default();
                theme::install(&ctx);
                mode.apply(&ctx);
                ctx.set_zoom_factor(zoom);
                let mut updates = staged_update();
                let render = |updates: &mut Updates, events| {
                    ctx.run(
                        egui::RawInput {
                            screen_rect: Some(egui::Rect::from_min_size(
                                egui::Pos2::ZERO,
                                Vec2::new(420.0, 700.0),
                            )),
                            events,
                            ..Default::default()
                        },
                        |ctx| {
                            updates.show(ctx);
                        },
                    )
                };
                for checking in [false, true] {
                    let _tx = checking.then(|| pending_check(&mut updates));
                    let mut labels = vec![];
                    for _ in 0..4 {
                        let output = render(&mut updates, vec![]);
                        labels.clear();
                        for shape in output.shapes {
                            collect(&shape.shape, &mut labels);
                        }
                    }
                    for label in [
                        "Install & Relaunch",
                        "Discard download",
                        "Close",
                        if checking {
                            "Checking…"
                        } else {
                            "Check again"
                        },
                    ] {
                        let (_, rect) = labels
                            .iter()
                            .find(|(text, _)| text == label)
                            .unwrap_or_else(|| panic!("missing {label}: {labels:?}"));
                        assert!(ctx.screen_rect().contains_rect(*rect), "{label}: {rect:?}");
                    }
                    if checking {
                        let pos = labels
                            .iter()
                            .find(|(text, _)| text == "Install & Relaunch")
                            .unwrap()
                            .1
                            .center();
                        for pressed in [true, false] {
                            render(
                                &mut updates,
                                vec![
                                    egui::Event::PointerMoved(pos),
                                    egui::Event::PointerButton {
                                        pos,
                                        button: egui::PointerButton::Primary,
                                        pressed,
                                        modifiers: egui::Modifiers::NONE,
                                    },
                                ],
                            );
                        }
                        assert!(updates.open, "disabled install must not close/relaunch");
                        assert!(updates.ready_to_install());
                    }
                }
                render(
                    &mut updates,
                    vec![egui::Event::Key {
                        key: egui::Key::Escape,
                        physical_key: None,
                        pressed: true,
                        repeat: false,
                        modifiers: egui::Modifiers::NONE,
                    }],
                );
                assert!(!updates.open);
                assert!(updates.ready_to_install());
                updates.cancel_staged();
            }
        }
    }

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
    fn stable_updates_only_replace_distribution_identity_regardless_of_bundle_path() {
        for path in ["/Applications/Pui.app", "/work/dist/Pui.app"] {
            let app = PathBuf::from(path);
            assert_eq!(
                stable_update_target(app.clone(), Some("dev.pi.pui")),
                Some(app.clone())
            );
            for id in [None, Some(""), Some("dev.pi.pui.dev"), Some("other.app")] {
                assert_eq!(stable_update_target(app.clone(), id), None);
            }
        }
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn installer_rejects_dev_and_unreadable_bundle_identities_before_launching() {
        let root = std::env::temp_dir().join(format!("pui-bundle-id-{}", uuid::Uuid::new_v4()));
        let stable = root.join("stable/Pui.app");
        let dev = root.join("dev/Pui.app");
        for (app, id) in [(&stable, "dev.pi.pui"), (&dev, "dev.pi.pui.dev")] {
            std::fs::create_dir_all(app.join("Contents")).unwrap();
            std::fs::write(
                app.join("Contents/Info.plist"),
                format!("<?xml version=\"1.0\"?><plist version=\"1.0\"><dict><key>CFBundleIdentifier</key><string>{id}</string></dict></plist>"),
            )
            .unwrap();
        }
        assert!(require_distribution_bundle(&stable).is_ok());
        assert!(require_distribution_bundle(&dev).is_err());
        assert!(spawn_installer_and_relaunch(&root, &stable, &dev).is_err());
        assert!(spawn_installer_and_relaunch(&root, &dev, &stable).is_err());
        assert!(spawn_installer_and_relaunch(&root, &stable, &root.join("missing.app")).is_err());
        std::fs::write(dev.join("Contents/Info.plist"), "not a plist").unwrap();
        assert!(require_distribution_bundle(&dev).is_err());
        std::fs::remove_dir_all(root).unwrap();
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
                zip_size: None,
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
        let progress = Progress::default();
        assert_eq!(
            progress_text(&progress),
            ("Starting download…".into(), None)
        );
        progress.total.store(129 * 1024 * 1024, Ordering::Relaxed);
        progress
            .downloaded
            .store(114 * 1024 * 1024, Ordering::Relaxed);
        let (label, fraction) = progress_text(&progress);
        assert_eq!(label, "Downloading 114 MB of 129 MB · 88%");
        assert!((fraction.unwrap() - 114.0 / 129.0).abs() < 1e-4);
        progress.set_phase(Phase::CheckingSignature);
        assert_eq!(progress_text(&progress).1, None);
        progress.cancel.store(true, Ordering::Relaxed);
        assert_eq!(progress_text(&progress).0, "Cancelling…");
        updates.install = InstallState::Ready {
            staged_dir: PathBuf::from("/tmp/pui-update-test"),
            staged_app: PathBuf::from("/tmp/pui-update-test/unpacked/Pui.app"),
            target_app: Some(PathBuf::from("/Applications/Pui.app")),
        };
        assert_eq!(updates.status(true).0, "Update ready to install");
        if let InstallState::Ready { target_app, .. } = &mut updates.install {
            *target_app = None;
        }
        let detail = updates.status(true).1;
        assert!(detail.contains("Pui Dev"));
        assert!(detail.contains("automatic replacement is disabled"));
        assert!(detail.contains("Finder"));
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
    fn release_notes_render_markdown_not_raw_markers() {
        let ctx = egui::Context::default();
        theme::install(&ctx);
        let mut updates = Updates {
            open: true,
            ..Default::default()
        };
        updates.result = Some(Ok(Release {
            version: "0.2.0".into(),
            url: "https://github.com/example/pui/releases/tag/v0.2.0".into(),
            body: "### Added\n\n- **Bold** feature with `code`\n".into(),
            zip_url: None,
            sums_url: None,
            zip_size: None,
            newer: true,
        }));
        let mut text = Vec::new();
        for _ in 0..2 {
            let output = ctx.run(Default::default(), |ctx| {
                updates.show(ctx);
            });
            text.clear();
            fn walk(shape: &egui::Shape, out: &mut Vec<String>) {
                match shape {
                    egui::Shape::Text(t) => out.push(t.galley.job.text.clone()),
                    egui::Shape::Vec(v) => v.iter().for_each(|s| walk(s, out)),
                    _ => {}
                }
            }
            for shape in output.shapes {
                walk(&shape.shape, &mut text);
            }
        }
        let all = text.join("\n");
        assert!(all.contains("Added"), "{all}");
        assert!(all.contains("Bold"), "{all}");
        assert!(!all.contains("###"), "heading markers rendered: {all}");
        assert!(!all.contains("**"), "emphasis markers rendered: {all}");
    }

    #[test]
    fn download_progress_row_renders_label_and_bar_in_both_themes() {
        for mode in [theme::Theme::Light, theme::Theme::Dark] {
            let ctx = egui::Context::default();
            theme::install(&ctx);
            mode.apply(&ctx);
            let mut updates = Updates {
                open: true,
                install: InstallState::Downloading,
                ..Default::default()
            };
            let progress = Arc::new(Progress::default());
            progress.total.store(100 * 1024 * 1024, Ordering::Relaxed);
            progress
                .downloaded
                .store(50 * 1024 * 1024, Ordering::Relaxed);
            updates.progress = Some(progress.clone());
            let mut text = Vec::new();
            for _ in 0..2 {
                let output = ctx.run(Default::default(), |ctx| {
                    updates.show(ctx);
                });
                text.clear();
                fn walk(shape: &egui::Shape, out: &mut Vec<String>) {
                    match shape {
                        egui::Shape::Text(t) => out.push(t.galley.job.text.clone()),
                        egui::Shape::Vec(v) => v.iter().for_each(|s| walk(s, out)),
                        _ => {}
                    }
                }
                for shape in output.shapes {
                    walk(&shape.shape, &mut text);
                }
            }
            let all = text.join("\n");
            assert!(all.contains("Downloading 50 MB of 100 MB · 50%"), "{all}");
            updates.cancel_download();
            assert!(progress.cancelled());
        }
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
