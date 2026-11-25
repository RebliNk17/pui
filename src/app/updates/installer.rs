//! The helper owns an exclusive app-lifetime lock while replacing the bundle.
//! It runs before GUI/runtime setup and never kills or closes another process.
use super::*;
use fs2::FileExt;
use std::io::{BufRead, Write};

pub(super) fn spawn(staged_dir: &Path, staged_app: &Path, target_app: &Path) -> Result<(), String> {
    require_distribution_bundle(target_app)?;
    require_distribution_bundle(staged_app)?;
    validate_paths(staged_dir, staged_app, target_app)?;
    let mut child = std::process::Command::new(
        std::env::current_exe().map_err(|_| "Cannot locate update helper.")?,
    )
    .arg("--install-staged")
    .arg(std::process::id().to_string())
    .arg(staged_dir)
    .arg(staged_app)
    .arg(target_app)
    .stdin(std::process::Stdio::null())
    .stdout(std::process::Stdio::piped())
    .stderr(std::process::Stdio::null())
    .spawn()
    .map_err(|_| "Could not start the background update installer.")?;
    let stdout = child
        .stdout
        .take()
        .ok_or("Cannot receive installer acknowledgement.")?;
    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || {
        let mut line = String::new();
        let _ = std::io::BufReader::new(stdout.take(128)).read_line(&mut line);
        let _ = tx.send(line);
    });
    if rx.recv_timeout(Duration::from_secs(5)).ok().as_deref() != Some("READY\n") {
        let _ = child.kill();
        let _ = child.wait();
        return Err("Update helper could not initialize; Pui was kept open.".into());
    }
    // Reap on normal exits too; the helper itself can safely outlive this process.
    std::thread::spawn(move || {
        let _ = child.wait();
    });
    Ok(())
}

fn validate_paths(staged_dir: &Path, staged_app: &Path, target: &Path) -> Result<(), String> {
    if !staged_dir.is_absolute()
        || !staged_app.is_absolute()
        || !target.is_absolute()
        || staged_app != staged_dir.join("unpacked/Pui.app")
        || !staged_dir
            .file_name()
            .and_then(|n| n.to_str())
            .is_some_and(|n| n.starts_with("pui-update-"))
        || target.extension().and_then(|e| e.to_str()) != Some("app")
    {
        return Err("Invalid staged update paths.".into());
    }
    let dir = staged_dir
        .canonicalize()
        .map_err(|_| "Staged update is unavailable.")?;
    let app = staged_app
        .canonicalize()
        .map_err(|_| "Staged app is unavailable.")?;
    let target = target
        .canonicalize()
        .map_err(|_| "Installed app is unavailable.")?;
    if !app.starts_with(&dir) || target.starts_with(&dir) || dir.starts_with(&target) {
        return Err("Update paths overlap or escape staging.".into());
    }
    Ok(())
}

pub(crate) fn run() -> i32 {
    match run_checked() {
        Ok(()) => 0,
        Err(error) => {
            eprintln!("{error}");
            1
        }
    }
}

fn run_checked() -> Result<(), String> {
    let args: Vec<_> = std::env::args_os().skip(2).collect();
    if args.len() != 4 {
        return Err("Invalid update helper arguments.".into());
    }
    let parent: i32 = args[0]
        .to_str()
        .and_then(|p| p.parse().ok())
        .filter(|pid| *pid > 1)
        .ok_or("Invalid update parent.")?;
    let (dir, app, target) = (
        Path::new(&args[1]),
        Path::new(&args[2]),
        Path::new(&args[3]),
    );
    validate_paths(dir, app, target)?;
    require_distribution_bundle(target)?;
    require_distribution_bundle(app)?;
    let lock = crate::update_gate::open()?;
    // Fail before acknowledging if replacing the app cannot create a sibling.
    // This is only a capability probe, never treated as proof of recovery save.
    let probe = target.with_file_name(format!(".pui-update-probe-{}", uuid::Uuid::new_v4()));
    let file = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&probe)
        .map_err(|_| "Installed app directory is not writable; Pui was kept open.")?;
    drop(file);
    std::fs::remove_file(probe).map_err(|_| "Cannot clean installer probe.")?;
    // Acknowledge initialization before waiting for the parent's shared guard.
    std::io::stdout()
        .write_all(b"READY\n")
        .and_then(|_| std::io::stdout().flush())
        .map_err(|_| "Cannot acknowledge installer.")?;
    let deadline = std::time::Instant::now() + Duration::from_secs(60);
    loop {
        match lock.try_lock_exclusive() {
            Ok(()) => break,
            Err(error)
                if error.kind() == std::io::ErrorKind::WouldBlock
                    && std::time::Instant::now() < deadline =>
            {
                std::thread::sleep(Duration::from_millis(100))
            }
            Err(_) => return Err("Other Pui windows are still open; update deferred.".into()),
        }
    }
    #[cfg(unix)]
    loop {
        if unsafe { libc::kill(parent, 0) } != 0
            && std::io::Error::last_os_error().raw_os_error() == Some(libc::ESRCH)
        {
            break;
        }
        // The lifetime guard drops just before main returns: allow that small
        // shutdown gap, retaining exclusive ownership while waiting for the PID.
        if std::time::Instant::now() >= deadline {
            return Err("Original Pui process has not exited; update deferred.".into());
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    // Preparation failures after the parent exits must reopen the unchanged
    // original, rather than leave the user with a closed application.
    struct ReopenOriginal<'a> {
        target: &'a Path,
        needed: bool,
    }
    impl Drop for ReopenOriginal<'_> {
        fn drop(&mut self) {
            if self.needed && self.target.is_dir() {
                let _ = std::process::Command::new("/usr/bin/open")
                    .arg(self.target)
                    .spawn();
            }
        }
    }
    let mut reopen = ReopenOriginal {
        target,
        needed: true,
    };
    // Recheck after taking exclusive ownership. No process can open the bundle
    // through the new startup path until replacement/rollback has completed.
    validate_paths(dir, app, target)?;
    require_distribution_bundle(target)?;
    require_distribution_bundle(app)?;
    let verified = std::process::Command::new("/usr/bin/codesign")
        .args(["--verify", "--deep", "--strict"])
        .arg(app)
        .status()
        .map_err(|_| "Cannot reverify staged signature.")?;
    if !verified.success() {
        return Err("Staged update signature is no longer valid.".into());
    }
    // Stage beside the target first: temp downloads and an installed app can
    // reside on different volumes, where rename(staged, target) is not atomic.
    let candidate = target.with_file_name(format!(".pui-update-{}.app", uuid::Uuid::new_v4()));
    let copied = std::process::Command::new("/usr/bin/ditto")
        .arg(app)
        .arg(&candidate)
        .status()
        .map_err(|_| "Cannot stage update beside installed app.")?;
    if !copied.success() {
        let _ = std::fs::remove_dir_all(&candidate);
        return Err("Cannot stage update beside installed app; original app unchanged.".into());
    }
    let copied_signature = std::process::Command::new("/usr/bin/codesign")
        .args(["--verify", "--deep", "--strict"])
        .arg(&candidate)
        .status()
        .map_err(|_| "Cannot verify copied update.")?;
    if !copied_signature.success() {
        let _ = std::fs::remove_dir_all(&candidate);
        return Err("Copied update signature is invalid; original app unchanged.".into());
    }
    let backup = replace(&candidate, target)?;
    // Replacement is complete before releasing the lock or launching the app.
    let _ = std::fs::remove_dir_all(&backup);
    let _ = std::fs::remove_dir_all(dir);
    drop(lock);
    reopen.needed = false;
    std::process::Command::new("/usr/bin/open")
        .arg(target)
        .spawn()
        .map_err(|_| "Updated app is installed; open it manually.")?;
    Ok(())
}

/// Both paths are siblings after candidate preparation, so each rename stays on
/// one filesystem. A failed second rename must restore the original bundle.
fn replace(candidate: &Path, target: &Path) -> Result<PathBuf, String> {
    let backup = target.with_file_name(format!(
        "{}.previous.{}",
        target
            .file_name()
            .ok_or("Missing update target filename.")?
            .to_string_lossy(),
        uuid::Uuid::new_v4()
    ));
    std::fs::rename(target, &backup).map_err(|_| "Cannot move installed app aside.")?;
    if std::fs::rename(candidate, target).is_err() {
        std::fs::rename(&backup, target).map_err(
            |_| "Installation and rollback failed; previous app retained beside target.",
        )?;
        return Err("Cannot install staged app; original restored.".into());
    }
    Ok(backup)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn replacement_keeps_original_and_rolls_back_failed_candidate_move() {
        let home =
            std::env::temp_dir().join(format!("pui-replacement-test-{}", uuid::Uuid::new_v4()));
        let target = home.join("Pui.app");
        let candidate = home.join("candidate.app");
        std::fs::create_dir_all(&target).unwrap();
        std::fs::write(target.join("version"), "old").unwrap();
        assert!(replace(&candidate, &target).is_err());
        assert_eq!(
            std::fs::read_to_string(target.join("version")).unwrap(),
            "old"
        );
        std::fs::create_dir(&candidate).unwrap();
        std::fs::write(candidate.join("version"), "new").unwrap();
        let backup = replace(&candidate, &target).unwrap();
        assert_eq!(
            std::fs::read_to_string(target.join("version")).unwrap(),
            "new"
        );
        assert_eq!(
            std::fs::read_to_string(backup.join("version")).unwrap(),
            "old"
        );
    }

    #[test]
    fn rejects_overlapping_relative_or_unexpected_staging_paths() {
        for (dir, app, target) in [
            (
                "relative",
                "relative/unpacked/Pui.app",
                "/Applications/Pui.app",
            ),
            (
                "/tmp/not-staging",
                "/tmp/not-staging/unpacked/Pui.app",
                "/Applications/Pui.app",
            ),
            (
                "/tmp/pui-update-example",
                "/tmp/outside/Pui.app",
                "/Applications/Pui.app",
            ),
            (
                "/tmp/pui-update-example",
                "/tmp/pui-update-example/unpacked/Pui.app",
                "/tmp/target",
            ),
        ] {
            assert!(validate_paths(Path::new(dir), Path::new(app), Path::new(target)).is_err());
        }
    }
}
