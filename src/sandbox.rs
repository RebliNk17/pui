//! OS-level local shell sandbox using macOS Seatbelt (`sandbox-exec`).
//! On non-macOS platforms, sandboxing is a safe no-op and `is_supported()` returns `false`.

use std::path::{Component, Path, PathBuf};
use std::process::Command;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Mode {
    #[default]
    Off,
    Standard,
    Strict,
}

impl Mode {
    pub fn from_flags(local_sandbox: bool, strict_sandbox: bool) -> Self {
        if !local_sandbox {
            Self::Off
        } else if strict_sandbox {
            Self::Strict
        } else {
            Self::Standard
        }
    }

    pub fn is_active(self) -> bool {
        !matches!(self, Self::Off)
    }
}

/// OS-level shell sandboxing uses macOS Seatbelt (`sandbox-exec`).
pub fn is_supported() -> bool {
    cfg!(target_os = "macos")
}

/// Escape a string literal for inclusion in a Seatbelt SBPL double-quoted string.
pub fn escape_string(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    for ch in value.chars() {
        match ch {
            '\\' => out.push_str("\\\\"),
            '"' => out.push_str("\\\""),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            '\0' => {}
            other => out.push(other),
        }
    }
    out
}

/// Best-effort canonicalization: resolves symlinks (e.g. `/var` -> `/private/var` on macOS)
/// even when the leaf path does not exist yet by canonicalizing the deepest existing ancestor.
pub fn canonicalize_path(path: &Path) -> PathBuf {
    let resolved = if let Ok(canon) = std::fs::canonicalize(path) {
        canon
    } else {
        let mut remainder = Vec::new();
        let mut cursor = path;
        let mut base = None;
        while let Some(parent) = cursor.parent() {
            if let Some(name) = cursor.file_name() {
                remainder.push(name.to_os_string());
            }
            if let Ok(canon) = std::fs::canonicalize(parent) {
                base = Some(canon);
                break;
            }
            if parent == cursor {
                break;
            }
            cursor = parent;
        }
        if let Some(mut canon) = base {
            for part in remainder.into_iter().rev() {
                canon.push(part);
            }
            canon
        } else {
            path.to_path_buf()
        }
    };
    strip_trailing_slash(&resolved)
}

fn strip_trailing_slash(path: &Path) -> PathBuf {
    let mut normalized = PathBuf::new();
    for component in path.components() {
        match component {
            Component::CurDir => {}
            other => normalized.push(other.as_os_str()),
        }
    }
    if normalized.as_os_str().is_empty() {
        path.to_path_buf()
    } else {
        normalized
    }
}

fn sb_path(path: &Path) -> String {
    escape_string(&path.to_string_lossy())
}

const SENSITIVE_HOME_DIRS: &[&str] = &[".ssh", ".aws", ".gnupg", ".kube"];
const SENSITIVE_HOME_FILES: &[&str] = &[".netrc", ".git-credentials"];

/// Generate a macOS Seatbelt (`.sb`) profile for `workspace` under `mode` with explicit
/// `tmpdir` and `home` directories (used by both production and hermetic tests).
pub fn build_profile(
    workspace: &Path,
    mode: Mode,
    tmpdir: Option<&Path>,
    home: Option<&Path>,
) -> Option<String> {
    if !mode.is_active() {
        return None;
    }

    let canon_workspace = canonicalize_path(workspace);
    let canon_tmpdir = tmpdir.map(canonicalize_path);

    let mut out = String::new();
    out.push_str("(version 1)\n");
    out.push_str("(allow default)\n");
    out.push_str("(deny file-write*)\n");
    out.push_str("(allow file-write*\n");
    out.push_str(&format!("  (subpath \"{}\")\n", sb_path(&canon_workspace)));
    out.push_str("  (subpath \"/tmp\")\n");
    out.push_str("  (subpath \"/private/tmp\")\n");
    if let Some(tmp) = canon_tmpdir.as_ref() {
        let tmp_str = tmp.to_string_lossy();
        if tmp_str != "/tmp" && tmp_str != "/private/tmp" && *tmp != canon_workspace {
            out.push_str(&format!("  (subpath \"{}\")\n", sb_path(tmp)));
        }
    }
    out.push_str("  (literal \"/dev/null\")\n");
    out.push_str("  (subpath \"/dev/fd\")\n");
    out.push_str(")\n");

    if let Some(home_dir) = home {
        let canon_home = canonicalize_path(home_dir);
        out.push_str("(deny file-read*\n");
        for dir in SENSITIVE_HOME_DIRS {
            let target = canonicalize_path(&canon_home.join(dir));
            out.push_str(&format!("  (subpath \"{}\")\n", sb_path(&target)));
        }
        for file in SENSITIVE_HOME_FILES {
            let target = canonicalize_path(&canon_home.join(file));
            out.push_str(&format!("  (literal \"{}\")\n", sb_path(&target)));
        }
        out.push_str(")\n");
    }

    if mode == Mode::Strict {
        // Restrict outbound network to local loopback and Unix domain sockets only.
        // Note: Seatbelt requires `(remote ip "localhost:*")` (not `local ip`, which
        // matches unbound sockets destined for any external host).
        out.push_str("(deny network-outbound)\n");
        out.push_str("(allow network-outbound (remote ip \"localhost:*\"))\n");
        out.push_str("(allow network* (remote unix-socket))\n");
    }

    Some(out)
}

/// Generate a macOS Seatbelt (`.sb`) profile for `workspace` using the process's
/// `TMPDIR` and `HOME`. Returns `None` when `mode == Mode::Off`.
pub fn profile(workspace: &Path, mode: Mode) -> Option<String> {
    let tmp = std::env::temp_dir();
    let home = std::env::var_os("HOME").map(PathBuf::from);
    build_profile(workspace, mode, Some(&tmp), home.as_deref())
}

/// Construct a shell `Command` running `command`, wrapped in macOS `sandbox-exec -p`
/// when `mode` is active on macOS. Uses inline `-p` to avoid temp-file cleanup races.
/// On non-macOS platforms, `mode` is ignored and a normal platform shell command is returned.
pub fn shell_command(command: &str, workspace: &Path, mode: Mode) -> Command {
    #[cfg(target_os = "macos")]
    {
        if let Some(sb_profile) = profile(workspace, mode) {
            let mut cmd = Command::new("/usr/bin/sandbox-exec");
            cmd.args(["-p", &sb_profile, "/bin/sh", "-c", command]);
            return cmd;
        }
        let mut cmd = Command::new("/bin/sh");
        cmd.args(["-c", command]);
        cmd
    }
    #[cfg(all(not(target_os = "macos"), not(windows)))]
    {
        let _ = (workspace, mode);
        let mut cmd = Command::new("/bin/sh");
        cmd.args(["-c", command]);
        cmd
    }
    #[cfg(windows)]
    {
        let _ = (workspace, mode);
        let mut cmd = Command::new("cmd.exe");
        cmd.args(["/D", "/S", "/C", command]);
        cmd
    }
}

/// Wrap an existing `Command` with macOS `sandbox-exec -p` when `mode` is active on macOS.
/// Preserves program, args, current_dir, and explicit environment overrides.
/// On non-macOS (or when `mode == Mode::Off`), returns `cmd` unmodified.
#[allow(dead_code)]
pub fn wrap_command(cmd: Command, workspace: &Path, mode: Mode) -> Command {
    #[cfg(target_os = "macos")]
    {
        let Some(sb_profile) = profile(workspace, mode) else {
            return cmd;
        };
        let mut wrapped = Command::new("/usr/bin/sandbox-exec");
        wrapped.arg("-p").arg(&sb_profile).arg(cmd.get_program());
        for arg in cmd.get_args() {
            wrapped.arg(arg);
        }
        if let Some(dir) = cmd.get_current_dir() {
            wrapped.current_dir(dir);
        }
        for (key, value) in cmd.get_envs() {
            match value {
                Some(val) => {
                    wrapped.env(key, val);
                }
                None => {
                    wrapped.env_remove(key);
                }
            }
        }
        wrapped
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = (workspace, mode);
        cmd
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mode_from_flags_and_platform_support() {
        assert_eq!(Mode::from_flags(false, false), Mode::Off);
        assert_eq!(Mode::from_flags(false, true), Mode::Off);
        assert_eq!(Mode::from_flags(true, false), Mode::Standard);
        assert_eq!(Mode::from_flags(true, true), Mode::Strict);
        assert!(!Mode::Off.is_active());
        assert!(Mode::Standard.is_active());
        assert!(Mode::Strict.is_active());
        assert_eq!(is_supported(), cfg!(target_os = "macos"));
    }

    #[test]
    fn escape_string_handles_quotes_backslashes_and_control_chars() {
        assert_eq!(
            escape_string(r#"/tmp/my "quoted" \dir"#),
            r#"/tmp/my \"quoted\" \\dir"#
        );
        assert_eq!(escape_string("a\nb\rc\td\0e"), "a\\nb\\rc\\tde");
    }

    #[test]
    fn profile_generation_standard_and_strict() {
        let ws = Path::new("/Users/dev/project");
        let tmp = Path::new("/private/var/folders/ab/cd/T");
        let home = Path::new("/Users/dev");

        assert!(build_profile(ws, Mode::Off, Some(tmp), Some(home)).is_none());

        let standard = build_profile(ws, Mode::Standard, Some(tmp), Some(home)).unwrap();
        assert!(standard.starts_with("(version 1)\n(allow default)\n(deny file-write*)\n"));
        assert!(standard.contains("(subpath \"/Users/dev/project\")"));
        assert!(standard.contains("(subpath \"/tmp\")"));
        assert!(standard.contains("(subpath \"/private/tmp\")"));
        assert!(standard.contains("(subpath \"/private/var/folders/ab/cd/T\")"));
        assert!(standard.contains("(literal \"/dev/null\")"));
        assert!(standard.contains("(subpath \"/dev/fd\")"));
        for dir in [".ssh", ".aws", ".gnupg", ".kube"] {
            assert!(
                standard.contains(&format!("(subpath \"/Users/dev/{dir}\")")),
                "missing deny for {dir}: {standard}"
            );
        }
        for file in [".netrc", ".git-credentials"] {
            assert!(
                standard.contains(&format!("(literal \"/Users/dev/{file}\")")),
                "missing deny for {file}: {standard}"
            );
        }
        assert!(!standard.contains("(deny network-outbound)"));

        let strict = build_profile(ws, Mode::Strict, Some(tmp), Some(home)).unwrap();
        assert!(strict.contains("(deny network-outbound)"));
        assert!(strict.contains("(allow network-outbound (remote ip \"localhost:*\"))"));
        assert!(strict.contains("(allow network* (remote unix-socket))"));
        assert!(
            !strict.contains("(local ip \"localhost:*\")"),
            "local ip would permit arbitrary remote destinations"
        );
    }

    #[test]
    fn profile_escapes_quotes_and_backslashes_in_paths() {
        let ws = Path::new(r#"/Users/dev/my "quoted" \workspace"#);
        let prof = build_profile(ws, Mode::Standard, None, None).unwrap();
        assert!(prof.contains(r#"(subpath "/Users/dev/my \"quoted\" \\workspace")"#));
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn seatbelt_enforces_workspace_writes_credential_reads_and_strict_network() {
        use std::net::TcpListener;
        use std::process::Stdio;
        use std::time::{SystemTime, UNIX_EPOCH};

        // Pick a scratch root guaranteed to be OUTSIDE /tmp, /private/tmp, and $TMPDIR
        // so the sibling directory write test genuinely exercises the kernel denial.
        let canon_tmpdir = canonicalize_path(&std::env::temp_dir());
        let var_tmp = canonicalize_path(Path::new("/var/tmp"));
        let scratch_parent = if !var_tmp.starts_with(&canon_tmpdir) {
            PathBuf::from("/var/tmp")
        } else {
            std::env::current_exe()
                .unwrap()
                .parent()
                .unwrap()
                .to_path_buf()
        };
        let unique = format!(
            "pui-sandbox-test-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        );
        let root = scratch_parent.join(unique);
        let workspace = root.join("workspace");
        let sibling = root.join("sibling");
        let fake_home = root.join("home");
        std::fs::create_dir_all(&workspace).unwrap();
        std::fs::create_dir_all(&sibling).unwrap();
        std::fs::create_dir_all(fake_home.join(".ssh")).unwrap();
        std::fs::write(fake_home.join(".ssh/id_rsa"), "secret-ssh-key").unwrap();
        std::fs::write(fake_home.join(".netrc"), "secret-netrc").unwrap();

        struct Cleanup(PathBuf);
        impl Drop for Cleanup {
            fn drop(&mut self) {
                let _ = std::fs::remove_dir_all(&self.0);
            }
        }
        let _cleanup = Cleanup(root.clone());

        // (a) Writing a file inside the workspace SUCCEEDS under Standard mode.
        let allowed_file = workspace.join("ok.txt");
        let out_ok = shell_command(
            &format!("printf 'hello' > '{}'", allowed_file.display()),
            &workspace,
            Mode::Standard,
        )
        .current_dir(&workspace)
        .output()
        .expect("run sandbox-exec for workspace write");
        assert!(
            out_ok.status.success(),
            "expected workspace write to succeed, stderr: {}",
            String::from_utf8_lossy(&out_ok.stderr)
        );
        assert_eq!(std::fs::read_to_string(&allowed_file).unwrap(), "hello");

        // (b) Writing a file inside a sibling dir OUTSIDE workspace is DENIED by the OS kernel.
        let denied_file = sibling.join("nope.txt");
        let out_denied = shell_command(
            &format!("printf 'nope' > '{}'", denied_file.display()),
            &workspace,
            Mode::Standard,
        )
        .current_dir(&workspace)
        .output()
        .expect("run sandbox-exec for sibling write");
        assert!(
            !out_denied.status.success(),
            "expected sibling write outside workspace to be denied"
        );
        assert!(
            !denied_file.exists(),
            "denied sibling file must not be created"
        );

        // (b2) Verify wrap_command enforces the same restriction.
        let mut raw_cmd = Command::new("/bin/sh");
        raw_cmd
            .args([
                "-c",
                &format!("printf 'nope' > '{}'", denied_file.display()),
            ])
            .current_dir(&workspace);
        let wrapped_out = wrap_command(raw_cmd, &workspace, Mode::Standard)
            .output()
            .expect("run wrap_command");
        assert!(!wrapped_out.status.success());
        assert!(!denied_file.exists());

        // (c) Credential directory reads are denied by the OS kernel.
        let cred_profile = build_profile(
            &workspace,
            Mode::Standard,
            Some(&std::env::temp_dir()),
            Some(&fake_home),
        )
        .unwrap();
        let read_ssh = Command::new("/usr/bin/sandbox-exec")
            .args([
                "-p",
                &cred_profile,
                "/bin/sh",
                "-c",
                &format!("cat '{}'", fake_home.join(".ssh/id_rsa").display()),
            ])
            .output()
            .expect("run sandbox-exec for ssh read");
        assert!(
            !read_ssh.status.success(),
            "reading ~/.ssh/id_rsa must be denied"
        );
        assert!(read_ssh.stdout.is_empty());

        let read_netrc = Command::new("/usr/bin/sandbox-exec")
            .args([
                "-p",
                &cred_profile,
                "/bin/sh",
                "-c",
                &format!("cat '{}'", fake_home.join(".netrc").display()),
            ])
            .output()
            .expect("run sandbox-exec for netrc read");
        assert!(
            !read_netrc.status.success(),
            "reading ~/.netrc must be denied"
        );
        assert!(read_netrc.stdout.is_empty());

        // (d) Strict mode contains (deny network-outbound), allows loopback, and blocks external IPs.
        let strict_profile = profile(&workspace, Mode::Strict).unwrap();
        assert!(strict_profile.contains("(deny network-outbound)"));

        // Local loopback connection should still succeed.
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let accept = std::thread::spawn(move || {
            let _ = listener.accept();
        });
        let local_net = shell_command(
            &format!(
                "python3 -c \"import socket; s=socket.create_connection(('127.0.0.1', {port}), 2); s.close()\""
            ),
            &workspace,
            Mode::Strict,
        )
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output()
        .expect("run strict sandbox local connect");
        let _ = accept.join();
        assert!(
            local_net.status.success(),
            "loopback connect should work in strict mode, stderr: {}",
            String::from_utf8_lossy(&local_net.stderr)
        );

        // External IP connection must be immediately rejected with EPERM by the kernel.
        let ext_net = shell_command(
            "python3 -c \"import socket; s=socket.socket(); s.settimeout(2); s.connect(('192.0.2.1', 80))\"",
            &workspace,
            Mode::Strict,
        )
        .output()
        .expect("run strict sandbox external connect");
        assert!(
            !ext_net.status.success(),
            "external outbound connect must be denied in strict mode"
        );
        let stderr = String::from_utf8_lossy(&ext_net.stderr);
        assert!(
            stderr.contains("Operation not permitted") || stderr.contains("PermissionError"),
            "expected kernel EPERM denial, got: {stderr}"
        );
    }
}
