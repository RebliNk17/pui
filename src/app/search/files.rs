//! Explicit filename-only Quick Open. Workers never read file bodies or leave the workspace.
use super::*;

type Found = Result<(Vec<PathBuf>, bool), String>;

#[derive(Default)]
pub(super) struct Files {
    requested: Option<(PathBuf, String)>,
    rx: Option<Receiver<Found>>,
    cancel: Arc<AtomicBool>,
    paths: Vec<PathBuf>,
    displayed: Option<(PathBuf, String)>,
    limited: bool,
    error: Option<String>,
}
impl Files {
    pub fn cancel(&mut self) {
        self.cancel.store(true, Ordering::Relaxed);
        self.rx = None;
        self.requested = None;
        self.paths.clear();
        self.displayed = None;
        self.error = None;
        self.limited = false;
    }
    pub fn pending(&self) -> bool {
        self.rx.is_some()
    }
    pub fn note(&self) -> Option<&str> {
        self.error.as_deref().or(self.limited.then_some(
            "File discovery reached a limit or skipped generated folders. Refine the path.",
        ))
    }
}
impl Drop for Files {
    fn drop(&mut self) {
        self.cancel.store(true, Ordering::Relaxed);
    }
}

/// A single positive :line suffix; colons elsewhere remain literal filename characters.
pub(super) fn query(value: &str) -> Result<(&str, Option<usize>), &'static str> {
    let value = value.trim();
    if value.chars().take(257).count() > 256 || value.chars().any(char::is_control) {
        return Err("Use a workspace filename, optionally followed by :line.");
    }
    if let Some((path, suffix)) = value.rsplit_once(':')
        && !suffix.is_empty()
        && suffix.bytes().all(|byte| byte.is_ascii_digit())
    {
        let line = suffix
            .parse::<usize>()
            .ok()
            .filter(|line| *line > 0 && *line <= 1_000_000)
            .ok_or("Line must be between 1 and 1,000,000.")?;
        if path.is_empty() {
            return Err("Enter a filename before :line.");
        }
        return Ok((path, Some(line)));
    }
    Ok((value, None))
}

impl Pui {
    pub(in crate::app) fn open_quick_files(&mut self) {
        if self.search_blocked() {
            return;
        }
        self.search.clear();
        self.spotlight.scope = Scope::Files;
        self.open_search();
    }
    pub(super) fn refresh_quick_files(&mut self, ctx: &egui::Context) {
        let parsed = query(&self.search);
        let path = parsed
            .as_ref()
            .map(|(path, _)| path.to_lowercase())
            .unwrap_or_default();
        let key = (self.prefs.cwd.clone(), path.clone());
        let files = &mut self.spotlight.files;
        if parsed.is_err() {
            files.cancel();
            files.error = parsed.err().map(str::to_owned);
        } else if files.requested.as_ref() != Some(&key) {
            files.cancel();
            if ctx.input(|input| input.time) - self.spotlight.changed_at >= 0.15 {
                files.requested = Some(key);
                files.cancel = Arc::new(AtomicBool::new(false));
                let cancel = files.cancel.clone();
                let root = self.prefs.cwd.clone();
                let ctx = ctx.clone();
                let (tx, rx) = mpsc::channel();
                files.rx = Some(rx);
                std::thread::spawn(move || {
                    let found = crate::app::files::mention_paths(&root, &path, &cancel).map(
                        |(mut paths, limited)| {
                            // Prefer the exact workspace-relative path over an
                            // identically named file in a nested checkout.
                            paths.sort_by_cached_key(|candidate| {
                                let relative = candidate
                                    .strip_prefix(&root)
                                    .unwrap_or(candidate)
                                    .to_string_lossy()
                                    .to_lowercase();
                                (relative != path, relative)
                            });
                            (paths, limited)
                        },
                    );
                    if !cancel.load(Ordering::Relaxed) {
                        let _ = tx.send(found);
                        ctx.request_repaint();
                    }
                });
            } else {
                ctx.request_repaint_after(Duration::from_millis(160));
            }
        }
        if let Some(found) = files.rx.as_ref().map(|rx| rx.try_recv()) {
            match found {
                Ok(result) => {
                    files.rx = None;
                    files.displayed = None;
                    match result {
                        Ok((paths, limited)) => {
                            files.paths = paths;
                            files.limited = limited;
                        }
                        Err(error) => files.error = Some(error),
                    }
                }
                Err(mpsc::TryRecvError::Disconnected) => {
                    files.rx = None;
                    files.error = Some(
                        "File discovery stopped. Close and reopen Quick Open to retry.".into(),
                    );
                }
                Err(mpsc::TryRecvError::Empty) => {}
            }
        }
        let display_key = (self.prefs.cwd.clone(), self.search.clone());
        if files.displayed.as_ref() == Some(&display_key) {
            return;
        }
        files.displayed = Some(display_key);
        let line = parsed.ok().and_then(|(_, line)| line);
        self.spotlight.hits = files
            .paths
            .iter()
            .map(|path| {
                let relative = path
                    .strip_prefix(&self.prefs.cwd)
                    .unwrap_or(path)
                    .display()
                    .to_string();
                Hit {
                    candidate: Candidate {
                        title: format!(
                            "{relative}{}",
                            line.map(|line| format!(":{line}")).unwrap_or_default()
                        ),
                        project: self.prefs.cwd.clone(),
                        cwd: self.prefs.cwd.clone(),
                        path: Some(path.clone()),
                        target: sidebar_tree::Target::Saved(PathBuf::new()),
                    },
                    detail: format!(
                        "Workspace file{} · {}",
                        line.map(|line| format!(" · line {line}"))
                            .unwrap_or_default(),
                        path.display()
                    ),
                    kind: Kind::File,
                    message: None,
                }
            })
            .collect();
        self.spotlight.selected = self
            .spotlight
            .selected
            .min(self.spotlight.hits.len().saturating_sub(1));
    }

    pub(super) fn activate_quick_file(&mut self, hit: &Hit) {
        if self.search_blocked() {
            return;
        }
        let Some(path) = hit.candidate.path.as_deref() else {
            return;
        };
        let line = match query(&self.search) {
            Ok((_, line)) => line,
            Err(_) => return,
        };
        let expected_title = format!(
            "{}{}",
            path.strip_prefix(&self.prefs.cwd).unwrap_or(path).display(),
            line.map(|line| format!(":{line}")).unwrap_or_default()
        );
        let valid = hit.candidate.title == expected_title
            && self.spotlight.scope == Scope::Files
            && hit.candidate.cwd == self.prefs.cwd
            && self
                .spotlight
                .files
                .requested
                .as_ref()
                .is_some_and(|(root, requested)| {
                    root == &self.prefs.cwd
                        && query(&self.search)
                            .is_ok_and(|(path, _)| path.to_lowercase() == *requested)
                })
            && self
                .spotlight
                .files
                .paths
                .iter()
                .any(|candidate| candidate == path);
        if !valid {
            self.spotlight.notice =
                Some("Workspace or query changed. Choose a current file result.".into());
            return;
        }
        match self
            .files
            .open_workspace_source(&self.ctx, &self.prefs.cwd, path, line)
        {
            Ok(()) => {
                self.settings_open = false;
                self.kanban.open = false;
                self.automation.open = false;
                self.tasks.open = false;
                self.inspector_panel = false;
                self.close_search();
                self.focus_composer = false;
            }
            Err(error) => self.spotlight.notice = Some(error),
        }
    }
}

#[cfg(test)]
mod resource_tests;
#[cfg(test)]
mod tests;
#[cfg(test)]
mod ui_tests;
