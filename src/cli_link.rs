//! Manage the `~/.local/bin/pui` CLI symlink so `pui` in external shells tracks
//! the active app build: `dist/Pui.app` for dev builds and `/Applications/Pui.app`
//! for the released distribution bundle.
use std::{
    io::Write,
    path::{Path, PathBuf},
};

const DISTRIBUTION_BUNDLE_ID: &str = "dev.pi.pui";
const DEV_BUNDLE_ID: &str = "dev.pi.pui.dev";
const APPLICATIONS_PUI_BIN: &str = "/Applications/Pui.app/Contents/MacOS/pui";

/// Parse `CFBundleIdentifier` from an XML `Info.plist` without spawning PlistBuddy.
pub(crate) fn parse_bundle_identifier(plist_xml: &str) -> Option<String> {
    let key = "<key>CFBundleIdentifier</key>";
    let after_key = plist_xml.split_once(key)?.1;
    let after_open = after_key.split_once("<string>")?.1;
    let value = after_open.split_once("</string>")?.0.trim();
    (!value.is_empty()).then(|| value.to_owned())
}

fn read_bundle_identifier(app_dir: &Path) -> Option<String> {
    let plist = std::fs::read_to_string(app_dir.join("Contents/Info.plist")).ok()?;
    parse_bundle_identifier(&plist)
}

/// Resolve the `.app` directory if `exe` is `<App>.app/Contents/MacOS/<bin>`.
pub(crate) fn app_bundle_for_exe(exe: &Path) -> Option<PathBuf> {
    let macos = exe.parent()?;
    if macos.file_name()? != "MacOS" {
        return None;
    }
    let contents = macos.parent()?;
    if contents.file_name()? != "Contents" {
        return None;
    }
    let app = contents.parent()?;
    (app.extension()?.to_str()? == "app").then(|| app.to_path_buf())
}

/// If `exe` is a Cargo build binary (`<repo>/target/{debug,release}/pui`),
/// look for `<repo>/dist/Pui.app/Contents/MacOS/pui`.
fn dev_dist_binary_for_cargo_exe(exe: &Path) -> Option<PathBuf> {
    let profile_dir = exe.parent()?;
    let target_dir = profile_dir.parent()?;
    if target_dir.file_name()? != "target" {
        return None;
    }
    let repo_root = target_dir.parent()?;
    let dist_bin = repo_root.join("dist/Pui.app/Contents/MacOS/pui");
    dist_bin.is_file().then_some(dist_bin)
}

/// Choose the binary that `~/.local/bin/pui` should point to for this app instance:
/// - Released distribution bundle (`dev.pi.pui`): `/Applications/Pui.app/Contents/MacOS/pui`
///   (if present, otherwise `exe`).
/// - Dev bundle (`dev.pi.pui.dev`) or Cargo dev binary: the `dist/Pui.app/Contents/MacOS/pui`
///   bundle binary.
pub(crate) fn preferred_cli_target_with(
    exe: &Path,
    bundle_id: Option<&str>,
    applications_bin: &Path,
) -> Option<PathBuf> {
    if app_bundle_for_exe(exe).is_some() {
        return match bundle_id {
            Some(DISTRIBUTION_BUNDLE_ID) => {
                if applications_bin.is_file() {
                    Some(applications_bin.to_path_buf())
                } else {
                    Some(exe.to_path_buf())
                }
            }
            Some(DEV_BUNDLE_ID) => Some(exe.to_path_buf()),
            _ => Some(exe.to_path_buf()),
        };
    }
    dev_dist_binary_for_cargo_exe(exe)
}

pub(crate) fn preferred_cli_target() -> Option<PathBuf> {
    let raw_exe = std::env::current_exe().ok()?;
    let exe = raw_exe.canonicalize().unwrap_or(raw_exe);
    let bundle_id = app_bundle_for_exe(&exe).and_then(|app| read_bundle_identifier(&app));
    preferred_cli_target_with(&exe, bundle_id.as_deref(), Path::new(APPLICATIONS_PUI_BIN))
}

pub(crate) fn default_link_dir() -> Option<PathBuf> {
    dirs::home_dir().map(|home| home.join(".local/bin"))
}

fn is_managed_pui_target(target: &Path) -> bool {
    let text = target.to_string_lossy();
    let is_app_bin = target.file_name().is_some_and(|n| n == "pui")
        && target
            .parent()
            .and_then(|p| p.file_name())
            .is_some_and(|n| n == "MacOS")
        && target
            .parent()
            .and_then(|p| p.parent())
            .and_then(|p| p.file_name())
            .is_some_and(|n| n == "Contents");
    let is_cargo_bin = target.file_name().is_some_and(|n| n == "pui")
        && target
            .parent()
            .and_then(|p| p.parent())
            .and_then(|p| p.file_name())
            .is_some_and(|n| n == "target");
    is_app_bin || is_cargo_bin || text.ends_with("/Pui.app/Contents/MacOS/pui")
}

#[derive(Debug, PartialEq, Eq)]
pub(crate) enum SyncOutcome {
    UpToDate(PathBuf, PathBuf),
    Updated(PathBuf, PathBuf),
    SkippedNonPui(PathBuf),
}

#[cfg(unix)]
pub(crate) fn sync_symlink_at(
    link_dir: &Path,
    target: &Path,
    force: bool,
) -> Result<SyncOutcome, String> {
    use std::os::unix::fs::symlink;

    let link_path = link_dir.join("pui");
    match std::fs::symlink_metadata(&link_path) {
        Ok(meta) => {
            if !meta.file_type().is_symlink() {
                if !force {
                    return Ok(SyncOutcome::SkippedNonPui(link_path));
                }
                return Err(format!(
                    "Refusing to overwrite non-symlink file at {}. Remove it manually first.",
                    link_path.display()
                ));
            }
            if let Ok(existing) = std::fs::read_link(&link_path) {
                if existing == target {
                    return Ok(SyncOutcome::UpToDate(link_path, target.to_path_buf()));
                }
                if !force && !is_managed_pui_target(&existing) {
                    return Ok(SyncOutcome::SkippedNonPui(link_path));
                }
            }
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => {
            return Err(format!("Cannot inspect {}: {error}", link_path.display()));
        }
    }

    std::fs::create_dir_all(link_dir)
        .map_err(|e| format!("Cannot create {}: {e}", link_dir.display()))?;
    let temp = link_dir.join(format!(".pui-link-{}.tmp", uuid::Uuid::new_v4()));
    symlink(target, &temp).map_err(|e| format!("Cannot create symlink {}: {e}", temp.display()))?;
    if let Err(error) = std::fs::rename(&temp, &link_path) {
        let _ = std::fs::remove_file(&temp);
        return Err(format!(
            "Cannot install CLI symlink at {}: {error}",
            link_path.display()
        ));
    }
    Ok(SyncOutcome::Updated(link_path, target.to_path_buf()))
}

#[cfg(not(unix))]
pub(crate) fn sync_symlink_at(
    _link_dir: &Path,
    _target: &Path,
    _force: bool,
) -> Result<SyncOutcome, String> {
    Err("CLI symlink installation is currently supported on macOS and Unix.".into())
}

/// Best-effort sync on GUI startup so `~/.local/bin/pui` tracks the active build.
pub(crate) fn sync_on_startup() {
    #[cfg(unix)]
    {
        let Some(link_dir) = default_link_dir() else {
            return;
        };
        let Some(target) = preferred_cli_target() else {
            return;
        };
        let _ = sync_symlink_at(&link_dir, &target, false);
    }
}

pub(crate) fn run_install_from_env() -> i32 {
    let args: Vec<String> = std::env::args().skip(2).collect();
    let mut out = std::io::stdout();
    let mut err = std::io::stderr();
    run_install(&args, &mut out, &mut err)
}

fn run_install(args: &[String], out: &mut dyn Write, err: &mut dyn Write) -> i32 {
    let mut dir = default_link_dir();
    let mut target_override: Option<PathBuf> = None;
    let mut force = false;
    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "-h" | "--help" => {
                let _ = writeln!(
                    out,
                    "Install or update the `pui` CLI symlink on PATH.\n\n\
                     Usage: pui install-cli [options]\n\n\
                     Options:\n  \
                       --dir <path>      Directory for the `pui` symlink (default: ~/.local/bin)\n  \
                       --target <path>   Explicit Pui binary path (default: auto-detect dev dist vs /Applications/Pui.app)\n  \
                       --force           Replace an existing symlink even if it points elsewhere\n  \
                       -h, --help        Show this help"
                );
                return 0;
            }
            "--force" => force = true,
            "--dir" => {
                i += 1;
                let Some(val) = args.get(i) else {
                    let _ = writeln!(err, "--dir requires a directory path.");
                    return 1;
                };
                dir = Some(PathBuf::from(val));
            }
            "--target" => {
                i += 1;
                let Some(val) = args.get(i) else {
                    let _ = writeln!(err, "--target requires a binary path.");
                    return 1;
                };
                target_override = Some(PathBuf::from(val));
            }
            other => {
                let _ = writeln!(
                    err,
                    "Unknown option: {other}. Use `pui install-cli --help`."
                );
                return 1;
            }
        }
        i += 1;
    }

    let Some(link_dir) = dir else {
        let _ = writeln!(
            err,
            "Could not determine ~/.local/bin directory. Pass --dir <path>."
        );
        return 1;
    };
    let target = target_override.or_else(preferred_cli_target).or_else(|| {
        std::env::current_exe()
            .ok()
            .map(|p| p.canonicalize().unwrap_or(p))
    });
    let Some(target) = target else {
        let _ = writeln!(err, "Could not locate the Pui binary.");
        return 1;
    };
    if !target.is_file() {
        let _ = writeln!(err, "Target binary does not exist: {}", target.display());
        return 1;
    }

    match sync_symlink_at(&link_dir, &target, force) {
        Ok(SyncOutcome::Updated(link, dest)) => {
            let _ = writeln!(
                out,
                "Installed CLI symlink: {} -> {}",
                link.display(),
                dest.display()
            );
            check_path_hint(&link_dir, out);
            0
        }
        Ok(SyncOutcome::UpToDate(link, dest)) => {
            let _ = writeln!(
                out,
                "CLI symlink already up to date: {} -> {}",
                link.display(),
                dest.display()
            );
            check_path_hint(&link_dir, out);
            0
        }
        Ok(SyncOutcome::SkippedNonPui(link)) => {
            let _ = writeln!(
                err,
                "Existing {} does not appear to be managed by Pui. Pass --force if it is a symlink you want to replace.",
                link.display()
            );
            1
        }
        Err(message) => {
            let _ = writeln!(err, "{message}");
            1
        }
    }
}

fn check_path_hint(link_dir: &Path, out: &mut dyn Write) {
    let on_path = std::env::var_os("PATH")
        .map(|path| std::env::split_paths(&path).any(|entry| entry == link_dir))
        .unwrap_or(false);
    if !on_path {
        let _ = writeln!(
            out,
            "Note: {} is not in your current PATH. Add it to your shell profile (e.g. export PATH=\"{}:$PATH\").",
            link_dir.display(),
            link_dir.display()
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_bundle_identifier_from_inline_and_multiline_plist() {
        let inline = "<dict><key>CFBundleIdentifier</key><string>dev.pi.pui</string></dict>";
        assert_eq!(
            parse_bundle_identifier(inline).as_deref(),
            Some("dev.pi.pui")
        );

        let multiline =
            "<dict>\n\t<key>CFBundleIdentifier</key>\n\t<string>dev.pi.pui.dev</string>\n</dict>";
        assert_eq!(
            parse_bundle_identifier(multiline).as_deref(),
            Some("dev.pi.pui.dev")
        );
    }

    #[test]
    fn preferred_target_distinguishes_dev_dist_and_released_applications_app() {
        let temp = std::env::temp_dir().join(format!("pui-cli-link-{}", uuid::Uuid::new_v4()));
        let dist_bin = temp.join("repo/dist/Pui.app/Contents/MacOS/pui");
        let apps_bin = temp.join("Applications/Pui.app/Contents/MacOS/pui");
        let cargo_bin = temp.join("repo/target/debug/pui");
        let other_release = temp.join("Downloads/Pui.app/Contents/MacOS/pui");
        for path in [&dist_bin, &apps_bin, &cargo_bin, &other_release] {
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(path, b"bin").unwrap();
        }

        // Dev bundle points to dist/Pui.app.
        assert_eq!(
            preferred_cli_target_with(&dist_bin, Some(DEV_BUNDLE_ID), &apps_bin),
            Some(dist_bin.clone())
        );
        // Released bundle points to /Applications/Pui.app when present.
        assert_eq!(
            preferred_cli_target_with(&other_release, Some(DISTRIBUTION_BUNDLE_ID), &apps_bin),
            Some(apps_bin.clone())
        );
        // Released bundle falls back to itself if /Applications/Pui.app is absent.
        let missing_apps = temp.join("Applications/Missing.app/Contents/MacOS/pui");
        assert_eq!(
            preferred_cli_target_with(&other_release, Some(DISTRIBUTION_BUNDLE_ID), &missing_apps),
            Some(other_release.clone())
        );
        // Cargo dev binary points to dist/Pui.app when built.
        assert_eq!(
            preferred_cli_target_with(&cargo_bin, None, &apps_bin),
            Some(dist_bin.clone())
        );

        let _ = std::fs::remove_dir_all(temp);
    }

    #[cfg(unix)]
    #[test]
    fn symlink_sync_switches_between_dev_and_released_and_refuses_unrelated_files() {
        use std::os::unix::fs::symlink;

        let temp = std::env::temp_dir().join(format!("pui-cli-sync-{}", uuid::Uuid::new_v4()));
        let link_dir = temp.join("bin");
        let dist_bin = temp.join("repo/dist/Pui.app/Contents/MacOS/pui");
        let apps_bin = temp.join("Applications/Pui.app/Contents/MacOS/pui");
        for path in [&dist_bin, &apps_bin] {
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(path, b"bin").unwrap();
        }

        // Initial install -> dev dist.
        assert_eq!(
            sync_symlink_at(&link_dir, &dist_bin, false).unwrap(),
            SyncOutcome::Updated(link_dir.join("pui"), dist_bin.clone())
        );
        // Second call is a no-op.
        assert_eq!(
            sync_symlink_at(&link_dir, &dist_bin, false).unwrap(),
            SyncOutcome::UpToDate(link_dir.join("pui"), dist_bin.clone())
        );
        // Launching released app switches symlink to /Applications/Pui.app.
        assert_eq!(
            sync_symlink_at(&link_dir, &apps_bin, false).unwrap(),
            SyncOutcome::Updated(link_dir.join("pui"), apps_bin.clone())
        );
        assert_eq!(std::fs::read_link(link_dir.join("pui")).unwrap(), apps_bin);

        // Switching back to dev app updates symlink back to dist.
        assert_eq!(
            sync_symlink_at(&link_dir, &dist_bin, false).unwrap(),
            SyncOutcome::Updated(link_dir.join("pui"), dist_bin.clone())
        );
        assert_eq!(std::fs::read_link(link_dir.join("pui")).unwrap(), dist_bin);

        // Unrelated symlink is left alone without --force.
        let other_dir = temp.join("other_bin");
        std::fs::create_dir_all(&other_dir).unwrap();
        symlink("/usr/bin/true", other_dir.join("pui")).unwrap();
        assert_eq!(
            sync_symlink_at(&other_dir, &dist_bin, false).unwrap(),
            SyncOutcome::SkippedNonPui(other_dir.join("pui"))
        );
        // Regular file is never overwritten, even with --force.
        let file_dir = temp.join("file_bin");
        std::fs::create_dir_all(&file_dir).unwrap();
        std::fs::write(file_dir.join("pui"), b"custom").unwrap();
        assert_eq!(
            sync_symlink_at(&file_dir, &dist_bin, false).unwrap(),
            SyncOutcome::SkippedNonPui(file_dir.join("pui"))
        );
        assert!(sync_symlink_at(&file_dir, &dist_bin, true).is_err());

        let _ = std::fs::remove_dir_all(temp);
    }
}
