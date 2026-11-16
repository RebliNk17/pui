use super::*;
use std::{
    io::{Read, Write},
    process::{Command, Stdio},
    time::Instant,
};

pub(super) fn run_helper(cwd: &Path, request: &Value) -> Result<Value, String> {
    let sdk = crate::usage::sdk()
        .filter(|p| p.is_file())
        .ok_or("Pi SDK not found. Set PUI_PI_SDK to its dist/index.js.")?;
    let mut command = Command::new(crate::runtime::node_binary());
    command
        .args([
            "--input-type=module",
            "--eval",
            include_str!("../../../../scripts/resources.mjs"),
        ])
        .arg(sdk)
        .env("PUI_RESOURCES_HELPER", "1")
        .current_dir(cwd)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null());
    crate::runtime::configure(&mut command);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(0x08000000);
    }
    let mut child = command
        .spawn()
        .map_err(|_| "Cannot start Node for resource management.")?;
    let stdout = child
        .stdout
        .take()
        .ok_or("Cannot read resource response.")?;
    let reader = std::thread::spawn(move || {
        let mut bytes = Vec::new();
        stdout
            .take(4 * 1024 * 1024 + 1)
            .read_to_end(&mut bytes)
            .map(|_| bytes)
    });
    let input = serde_json::to_vec(request).map_err(|_| "Invalid resource request.")?;
    if child
        .stdin
        .take()
        .is_none_or(|mut stdin| stdin.write_all(&input).is_err())
    {
        let _ = child.kill();
        let _ = child.wait();
        return Err("Could not send resource request.".into());
    }
    let timeout = if request["action"] == "install"
        || request["action"] == "remove"
        || request["action"] == "apply"
    {
        300
    } else {
        30
    };
    let start = Instant::now();
    loop {
        match child.try_wait() {
            Ok(Some(_)) => break,
            Ok(None) if start.elapsed() < Duration::from_secs(timeout) => {
                std::thread::sleep(Duration::from_millis(100))
            }
            _ => {
                let _ = child.kill();
                let _ = child.wait();
                return Err("Resource operation timed out or stopped. It may have partially completed; refresh before retrying.".into());
            }
        }
    }
    let bytes = reader
        .join()
        .map_err(|_| "Resource reader stopped.")?
        .map_err(|_| "Cannot read resource response.")?;
    if bytes.len() > 4 * 1024 * 1024 {
        return Err("Resource catalog exceeds 4 MiB. Narrow your configuration.".into());
    }
    let record: Value = serde_json::from_slice(&bytes)
        .map_err(|_| "Pi returned an invalid resource response. Check SDK compatibility.")?;
    if let Some(error) = record["error"].as_str() {
        return Err(error.into());
    }
    Ok(record["data"].clone())
}
pub(super) fn open_config(file: PathBuf) {
    // Fixed JSON config paths only, never execute a selected extension/script.
    std::thread::spawn(move || {
        #[cfg(target_os = "macos")]
        let result = Command::new("open").arg("-t").arg(&file).status();
        #[cfg(target_os = "windows")]
        let result = Command::new("notepad.exe").arg(&file).status();
        #[cfg(not(any(target_os = "macos", target_os = "windows")))]
        let result = Command::new("xdg-open").arg(&file).status();
        let _ = result;
    });
}
