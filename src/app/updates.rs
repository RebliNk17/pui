//! Manual GitHub release checks only. No startup polling, downloads or self-update.
use super::*;
use std::io::Read;

#[derive(Default)]
pub(super) struct Updates {
    pub open: bool,
    result: Option<Result<Release, String>>,
    rx: Option<Receiver<Result<Release, String>>>,
    checked: bool,
}
struct Release {
    version: String,
    url: String,
    newer: bool,
}
fn repository() -> Option<&'static str> {
    option_env!("PUI_GITHUB_REPOSITORY").filter(|s| !s.trim().is_empty())
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
    // Ignore URLs from API data. Only open the configured repository's page.
    Ok(Release {
        version: version.to_string(),
        url: format!("https://github.com/{repo}/releases/latest"),
        newer: version > current,
    })
}
fn check(repo: &str) -> Result<Release, String> {
    if !validate_repository(repo) {
        return Err("Invalid GitHub release repository.".into());
    }
    // Node is an existing Pi prerequisite; fetch is bounded and receives no Pi
    // config, auth or tokens. No new HTTP runtime or idle worker is required.
    let script = r#"
const response = await fetch(process.argv.at(-1), {headers:{Accept:'application/vnd.github+json','User-Agent':'Pui-release-check'},redirect:'error',signal:AbortSignal.timeout(12000)});
if(!response.ok){process.stdout.write(JSON.stringify({status:response.status}));process.exit(0);}
const reader=response.body.getReader();const chunks=[];let bytes=0;
while(true){const {value,done}=await reader.read();if(done)break;bytes+=value.byteLength;if(bytes>1048576){await reader.cancel();throw Error('response too large');}chunks.push(value);}
process.stdout.write(Buffer.concat(chunks).toString('utf8'));
"#;
    let mut command = std::process::Command::new("node");
    command
        .args(["--input-type=module", "--eval", script])
        .arg(format!(
            "https://api.github.com/repos/{repo}/releases/latest"
        ))
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::null());
    if let Some(path) = rpc::child_path() {
        command.env("PATH", path);
    }
    let mut child = command
        .spawn()
        .map_err(|_| "Cannot start the release checker. Install the Node runtime used by Pi.")?;
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
        return Err(match status {404=>"No published release was found. The repository may be private or not published yet.".into(),403|429=>"GitHub rate limit reached. Try again later.".into(),_=>format!("GitHub release check failed (HTTP {status}).")});
    }
    release(repo, env!("CARGO_PKG_VERSION"), &data)
}
impl Updates {
    pub(super) fn start(&mut self, ctx: &egui::Context) {
        self.open = true;
        if self.rx.is_some() {
            return;
        }
        self.checked = true;
        self.result = None;
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
    pub(super) fn poll(&mut self) {
        if let Some(result) = self.rx.as_ref().and_then(|rx| rx.try_recv().ok()) {
            self.rx = None;
            self.result = Some(result);
        }
    }
    pub(super) fn show(&mut self, ctx: &egui::Context) {
        if !self.open {
            return;
        }
        let mut retry = false;
        egui::Window::new("Pui updates").open(&mut self.open).resizable(false).collapsible(false).default_width(390.0).show(ctx,|ui|{
            ui.heading(format!("Pui {}",env!("CARGO_PKG_VERSION")));
            ui.add_space(8.0);
            if repository().is_none(){
                ui.label("GitHub updates aren’t configured yet.");
                ui.add(egui::Label::new(theme::small(ui,"When releases are ready, build with PUI_GITHUB_REPOSITORY=OWNER/REPO. Checks will use published stable GitHub Releases. No network request was made.")).wrap());
            }else if self.rx.is_some(){ui.label("Checking GitHub Releases…");}
            else if let Some(result)=&self.result {match result {
                Ok(release)=>{
                    ui.label(if release.newer {format!("Version {} is available.",release.version)}else{format!("You’re up to date. Latest release: {}.",release.version)});
                    if ui.button("Open GitHub release").clicked(){ctx.open_url(egui::OpenUrl::new_tab(&release.url));}
                }
                Err(error)=>{ui.add(egui::Label::new(error).wrap());}
            }}else if !self.checked {ui.label("Updates are checked only when you ask.");}
            ui.add_space(8.0);ui.add(egui::Label::new(theme::small(ui,"Pui never downloads, installs or restarts automatically. Existing agents are not interrupted.")).wrap());
            if repository().is_some() && self.rx.is_none() && ui.button("Check again").clicked(){retry=true;}
        });
        if retry {
            self.start(ctx);
        }
    }
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
        let data = json!({"tag_name":"v1.10.0","draft":false,"prerelease":false,"html_url":"https://untrusted.invalid"});
        let r = release("owner/pui", "1.9.0", &data).unwrap();
        assert!(r.newer);
        assert_eq!(r.url, "https://github.com/owner/pui/releases/latest");
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
