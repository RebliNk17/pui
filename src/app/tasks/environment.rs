//! Bounded, read-only prerequisite evidence. This is not an environment health check.
//! Never executes project code, installs packages, follows dependency links, or reads credentials.
use super::*;
use std::{fs, path::Path};

#[cfg(test)]
#[path = "environment_tests.rs"]
mod tests;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Stage {
    Proposal,
    Implementation,
    Checks,
    Repair(u8),
}
#[derive(Debug, Default)]
pub(super) struct Report {
    pub evidence: Vec<String>,
    pub blockers: Vec<String>,
}
impl Report {
    pub fn note(&self) -> String {
        let mut parts = vec!["Read-only prerequisite inspection only; dependency completeness and environment health are not established. No setup commands were run.".to_owned()];
        parts.extend(self.evidence.iter().cloned());
        parts.extend(self.blockers.iter().cloned());
        if !self.blockers.is_empty() {
            parts.push("Automatic dependency/bootstrap setup is not supported. A separate user-reviewed setup plan can propose supported frozen-lockfile commands or accept explicit project-specific commands for this retained isolated workspace. Reviewing a plan runs nothing. Setup requires separate task/workspace/fingerprint-bound confirmation and normal exact-command shell permission approval; no install, network access, lifecycle script, credential/environment copying, or dependency symlink is authorized by workflow consent. Setup never silently resumes implementation or checks; use an explicit recovery action after refreshing prerequisite evidence. Revalidation only compares existing candidate evidence; it does not rerun commands.".into());
        }
        parts.join("\n")
    }
}

/// Inspect only requirements grounded in a supported, direct approved command.
/// Other command forms are explicitly unknown, not guessed from incidental manifests.
pub(super) fn inspect(source: &Path, workspace: &Path, contract: Option<&Contract>) -> Report {
    let mut report = Report::default();
    if !workspace.is_dir() {
        report
            .blockers
            .push("Workspace directory is unavailable.".into());
        return report;
    }
    let Some(contract) = contract else {
        report.evidence.push("No acceptance commands exist yet. Project-specific prerequisites will be inspected after checks are established, before implementation.".into());
        return report;
    };
    for criterion in contract
        .criteria
        .iter()
        .filter(|c| !c.command.trim().is_empty())
    {
        let words: Vec<_> = criterion.command.split_whitespace().collect();
        let Some(tool) = words.first().copied() else {
            continue;
        };
        if criterion
            .command
            .contains(['\n', ';', '|', '&', '$', '`', '<', '>'])
        {
            report.evidence.push(format!("{}: compound/custom command prerequisites are unknown; normal command permission remains required.", criterion.id));
            continue;
        }
        if matches!(tool, "npm" | "pnpm" | "yarn" | "bun" | "npx") {
            if !available(tool) {
                report.blockers.push(format!("{}: approved command requires `{tool}`, but no executable was found on the host PATH.", criterion.id));
            }
            // Prefix/workspace flags may select another manifest. Do not invent its requirements.
            if words
                .iter()
                .any(|w| matches!(*w, "--prefix" | "--cwd" | "-C" | "--workspace" | "-w"))
            {
                report.evidence.push(format!("{}: alternate package roots are not inspected; prerequisite coverage is unknown.", criterion.id));
                continue;
            }
            if tool != "npx"
                && !words.get(1).is_some_and(|verb| {
                    matches!(
                        *verb,
                        "test" | "t" | "run" | "run-script" | "exec" | "lint" | "build"
                    )
                })
            {
                report.evidence.push(format!("{}: package-manager command requirements are not inferred for this subcommand.", criterion.id));
                continue;
            }
            match package_dependencies(workspace) {
                Ok(Some(true)) => match fs::symlink_metadata(workspace.join("node_modules")) {
                    Ok(meta) if meta.is_dir() => report.evidence.push(format!("{}: package.json declares dependencies and a local node_modules directory exists; contents and usability are unverified.", criterion.id)),
                    Ok(meta) if meta.file_type().is_symlink() => report.blockers.push(format!("{}: local node_modules is a symlink; dependency isolation is unverified. The link was not followed or copied.", criterion.id)),
                    _ => report.blockers.push(format!("{}: package.json declares dependencies but local node_modules is absent or not a directory at {}. Ignored dependencies are not included in isolated snapshots; this is an environment blocker, not an implementation failure.", criterion.id, workspace.display())),
                },
                Ok(Some(false)) => report.evidence.push(format!("{}: package.json declares no package dependencies; script/tool prerequisites remain unverified.", criterion.id)),
                Ok(None) => report.evidence.push(format!("{}: no direct package.json; package-root prerequisites are unknown.", criterion.id)),
                Err(error) => report.blockers.push(format!("{}: {error}", criterion.id)),
            }
        } else if matches!(
            tool,
            "cargo" | "rustc" | "python" | "python3" | "pytest" | "uv" | "go" | "make" | "cmake"
        ) {
            if !available(tool) {
                report.blockers.push(format!("{}: approved command requires `{tool}`, but no executable was found on the host PATH.", criterion.id));
            } else {
                report.evidence.push(format!("{}: `{tool}` found on PATH; SDKs, caches, dependencies and project-specific prerequisites remain unverified.", criterion.id));
            }
        } else if matches!(
            tool,
            ".venv/bin/python"
                | ".venv/bin/python3"
                | ".venv/bin/pytest"
                | "./.venv/bin/python"
                | "./.venv/bin/pytest"
        ) {
            let relative = tool.trim_start_matches("./");
            let components = [".venv", ".venv/bin", relative];
            let safe = components.iter().all(|p| {
                fs::symlink_metadata(workspace.join(p)).is_ok_and(|m| !m.file_type().is_symlink())
            });
            if !safe || !workspace.join(relative).is_file() {
                report.blockers.push(format!("{}: approved command requires local `{relative}`, which is absent or symlinked. Virtual environments are not copied from {}.", criterion.id, source.display()));
            }
        } else {
            report.evidence.push(format!("{}: custom command prerequisites are unsupported by read-only preflight; no environment-health claim is made.", criterion.id));
        }
    }
    report
}

fn available(tool: &str) -> bool {
    std::env::var_os("PATH").is_some_and(|path| {
        std::env::split_paths(&path).any(|dir| {
            let path = dir.join(tool);
            let Ok(meta) = fs::metadata(path) else {
                return false;
            };
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                meta.is_file() && meta.permissions().mode() & 0o111 != 0
            }
            #[cfg(not(unix))]
            {
                meta.is_file()
            }
        })
    })
}
fn package_dependencies(root: &Path) -> Result<Option<bool>, String> {
    let path = root.join("package.json");
    let meta = match fs::symlink_metadata(&path) {
        Ok(meta) => meta,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(_) => return Err("Cannot inspect package.json metadata.".into()),
    };
    if !meta.is_file() || meta.len() > 256 * 1024 {
        return Err("package.json must be a regular bounded file; symlinks and oversized manifests are not read.".into());
    }
    // O_NOFOLLOW closes replacement-with-symlink races; bounded reads close growth races.
    let mut options = fs::OpenOptions::new();
    options.read(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK);
    }
    let file = options
        .open(path)
        .map_err(|_| "Cannot safely read package.json.")?;
    if !file
        .metadata()
        .map_err(|_| "Cannot inspect opened package.json.")?
        .is_file()
    {
        return Err("package.json is not a regular file.".into());
    }
    use std::io::Read;
    let mut bytes = vec![];
    file.take(256 * 1024 + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| "Cannot read package.json.")?;
    if bytes.len() > 256 * 1024 {
        return Err("package.json exceeds the read-only inspection limit.".into());
    }
    let manifest: Value = serde_json::from_slice(&bytes)
        .map_err(|_| "package.json cannot be parsed; package requirements are unknown.")?;
    Ok(Some(
        ["dependencies", "devDependencies", "optionalDependencies"]
            .iter()
            .any(|key| {
                manifest[*key]
                    .as_object()
                    .is_some_and(|deps| !deps.is_empty())
            }),
    ))
}

/// Ground classification in actual, unsuccessful command evidence. Never call an
/// ordinary assertion/type error an environment failure; unknowns stay unknown.
pub(super) fn failed_requirement(check: &Check) -> Option<String> {
    if check.passed() || check.cancelled || check.exit_code.is_none() {
        return None;
    }
    let text = check.output.to_ascii_lowercase();
    let recognized = [
        "cannot find module",
        "modulenotfounderror:",
        "no module named ",
        "command not found",
        "no matching package named",
        "failed to get `",
        "could not resolve host",
        "network is unreachable",
    ];
    let cargo_offline_cache = text.contains("error: failed to download `")
        && text.contains("attempting to make an http request, but --offline was specified");
    let missing_linker = text.lines().any(|line| {
        line.trim_start().starts_with("error: linker `") && line.trim_end().ends_with("` not found")
    });
    (recognized.iter().any(|s| text.contains(s)) || cargo_offline_cache || missing_linker).then(|| format!("Check {} reported an unavailable module, tool, package, or network prerequisite. Inspect its exact retained command/output; environment health is unknown. No verifier or automatic repair was started. Automatic setup is unsupported; inspect a separate user-reviewed setup plan with exact-command permission, then explicitly choose recovery. Never replay uncertain shell effects.", check.criterion))
}
