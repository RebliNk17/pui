//! Read-only, bounded Git file navigation. One requested body; no patch-list cache.
use super::*;
use std::{
    path::Path,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
};
use workspace::{MAX_FILES, MAX_PATH_BYTES, read_only::Request};

#[derive(Clone)]
struct List {
    root: PathBuf,
    branch: String,
    files: Vec<workspace::FileChange>,
}
impl List {
    fn new(workspace: &Workspace) -> Option<Self> {
        if !workspace.is_git
            || workspace.root.as_os_str().len() > 8192
            || workspace.branch.len() > 1024
            || workspace
                .files
                .iter()
                .take(MAX_FILES + 1)
                .any(|file| file.status.len() > 8)
            || workspace.files.len() > MAX_FILES
            || workspace.files.is_empty()
            || workspace
                .files
                .iter()
                .map(|file| file.path.len())
                .sum::<usize>()
                > MAX_PATH_BYTES
            || workspace.files.iter().any(|file| !valid_path(&file.path))
        {
            return None;
        }
        Some(Self {
            root: workspace.root.clone(),
            branch: workspace.branch.clone(),
            files: workspace
                .files
                .iter()
                .map(|file| workspace::FileChange {
                    path: file.path.clone(),
                    status: file.status.clone(),
                    ..Default::default()
                })
                .collect(),
        })
    }
    fn matches(&self, workspace: &Workspace) -> bool {
        workspace.is_git
            && self.root == workspace.root
            && self.branch == workspace.branch
            && self.files.len() == workspace.files.len()
            && self
                .files
                .iter()
                .zip(&workspace.files)
                .all(|(old, now)| old.path == now.path && old.status == now.status)
    }
}
use workspace::valid_diff_path as valid_path;

pub(in crate::app) struct Loaded {
    root: PathBuf,
    file: workspace::FileChange,
    list: Option<List>,
    result: Result<String, String>,
}
impl Loaded {
    pub(in crate::app) fn finish(
        self,
        workspace: &Workspace,
    ) -> Option<Result<(String, String), String>> {
        if self.root != workspace.root
            || !workspace.is_git
            || !workspace
                .files
                .iter()
                .any(|file| file.path == self.file.path && file.status == self.file.status)
            || self
                .list
                .as_ref()
                .is_some_and(|list| !list.matches(workspace))
        {
            return None;
        }
        Some(self.result.map(|diff| (self.file.path, diff)))
    }
}
struct Pending {
    rx: Request<Loaded>,
    target: usize,
    owner: egui::Id,
}
#[derive(Default)]
pub(super) struct Git {
    list: Option<List>,
    position: Option<usize>,
    bound: bool,
    pub(super) stale: bool,
    pending: Option<Pending>,
    error: Option<String>,
}

pub(in crate::app) fn start(
    ctx: &egui::Context,
    workspace: &Workspace,
    file: workspace::FileChange,
) -> Request<Loaded> {
    let loaded = Loaded {
        root: workspace.root.clone(),
        file,
        list: List::new(workspace),
        result: Err(
            "Git review is busy. Try the file action again when the existing read finishes.".into(),
        ),
    };
    let Some(permit) = permit(ctx) else {
        let (tx, rx) = mpsc::channel();
        let _ = tx.send(loaded);
        ctx.request_repaint();
        return Request::new(rx, Arc::new(AtomicBool::new(false)));
    };
    spawn_read(ctx, permit, move |cancel| {
        let mut loaded = loaded;
        loaded.result = workspace::diff_cancellable(&loaded.root, &loaded.file, cancel);
        loaded
    })
}

// Inline expansions and full review share a single per-window read permit.
// A busy inline row waits for the active reader's repaint, not another thread.
struct Permit(Arc<AtomicBool>);
impl Drop for Permit {
    fn drop(&mut self) {
        self.0.store(false, Ordering::Release);
    }
}
fn permit(ctx: &egui::Context) -> Option<Permit> {
    named_permit(ctx, "git-review-worker")
}
fn named_permit(ctx: &egui::Context, id: &str) -> Option<Permit> {
    let gate = ctx.data_mut(|data| {
        data.get_temp_mut_or_default::<Arc<AtomicBool>>(egui::Id::new(id))
            .clone()
    });
    gate.compare_exchange(false, true, Ordering::AcqRel, Ordering::Relaxed)
        .ok()
        .map(|_| Permit(gate))
}
fn spawn_read<T: Send + 'static>(
    ctx: &egui::Context,
    permit: Permit,
    read: impl FnOnce(&AtomicBool) -> T + Send + 'static,
) -> Request<T> {
    let (tx, rx) = mpsc::channel();
    let cancel = Arc::new(AtomicBool::new(false));
    let request = Request::new(rx, cancel.clone());
    let repaint = ctx.clone();
    // Failure drops the permit and sender; callers display disconnection.
    if std::thread::Builder::new()
        .name("pui-git-review".into())
        .spawn(move || {
            let result = read(&cancel);
            drop(permit);
            if !cancel.load(Ordering::Acquire) {
                let _ = tx.send(result);
            }
            repaint.request_repaint();
        })
        .is_err()
    {
        ctx.request_repaint();
    }
    request
}
pub(in crate::app) fn start_workspace(ctx: &egui::Context, root: PathBuf) -> Request<Workspace> {
    let Some(permit) = named_permit(ctx, "git-workspace-worker") else {
        let (tx, rx) = mpsc::channel();
        let _ = tx.send(Workspace {
            root,
            error: Some("A Git workspace read is still finishing. Refresh to retry.".into()),
            ..Default::default()
        });
        return Request::new(rx, Arc::new(AtomicBool::new(false)));
    };
    spawn_read(ctx, permit, move |cancel| workspace::read(&root, cancel))
}
pub(super) fn start_inline(
    ctx: &egui::Context,
    root: &Path,
    file: workspace::FileChange,
) -> Option<Request<Result<String, String>>> {
    let permit = permit(ctx)?;
    let root = root.to_path_buf();
    Some(spawn_read(ctx, permit, move |cancel| {
        workspace::diff_cancellable(&root, &file, cancel)
    }))
}

impl Changes {
    pub(in crate::app) fn bind_git_files(&mut self, workspace: &Workspace) {
        if self.git_files.bound {
            return;
        }
        self.git_files.bound = true;
        self.git_files.list = List::new(workspace);
        if let Some(doc) = &mut self.review {
            doc.2.set_untracked(
                !doc.3
                    && workspace
                        .files
                        .iter()
                        .any(|file| file.path == doc.0 && file.status == "??"),
            );
        }
        self.git_files.position = self.review.as_ref().and_then(|doc| {
            self.git_files
                .list
                .as_ref()?
                .files
                .iter()
                .position(|file| file.path == doc.0)
        });
    }
    pub(in crate::app) fn cancel_git_read(&mut self) {
        self.git_files.pending = None;
    }
    pub(super) fn poll_git_files(&mut self, ctx: &egui::Context, workspace: &Workspace) {
        if self.review.as_ref().is_none_or(|doc| doc.3) {
            self.git_files = Default::default();
            return;
        }
        self.bind_git_files(workspace);
        if self
            .git_files
            .list
            .as_ref()
            .is_some_and(|list| !list.matches(workspace))
        {
            self.git_files.stale = true;
            self.cancel_git_read();
        }
        // Frozen menus/modal forms retain their body/keyboard ownership until
        // dismissed, even when a worker has completed in the meantime.
        if egui::Popup::is_any_open(ctx) || ctx.memory(|memory| memory.top_modal_layer().is_some())
        {
            return;
        }
        let result =
            self.git_files
                .pending
                .as_ref()
                .and_then(|pending| match pending.rx.try_recv() {
                    Ok(result) => Some(Ok(result)),
                    Err(mpsc::TryRecvError::Disconnected) => {
                        Some(Err("Git review worker ended without a result.".to_owned()))
                    }
                    Err(mpsc::TryRecvError::Empty) => None,
                });
        let Some(result) = result else {
            return;
        };
        let pending = self.git_files.pending.take().unwrap();
        if self.git_files.stale
            || self
                .review
                .as_ref()
                .is_none_or(|doc| doc.2.focus_id() != pending.owner)
        {
            return;
        }
        match result.and_then(|loaded| {
            loaded
                .finish(workspace)
                .unwrap_or_else(|| Err("Git file list changed; reopen review to navigate.".into()))
        }) {
            Ok((path, diff)) => {
                let mut document: diff_viewer::Document = (path, diff).into();
                clear_selection(ctx);
                // Completing a background read must not steal composer focus.
                if self
                    .review
                    .as_ref()
                    .is_some_and(|doc| doc.2.owns_focus(ctx))
                {
                    document.2.focus(ctx);
                }
                document.3 = false;
                document.2.set_untracked(
                    workspace
                        .files
                        .iter()
                        .any(|file| file.path == document.0 && file.status == "??"),
                );
                self.review = Some(document);
                self.git_files.position = Some(pending.target);
                self.git_files.error = None;
            }
            Err(error) => self.git_files.error = Some(error),
        }
    }
    fn git_available(&self) -> bool {
        !self.git_files.stale
            && self.git_files.list.is_some()
            && self.git_files.position.is_some()
            && self.git_files.pending.is_none()
    }
    fn select_git_file(&mut self, ctx: &egui::Context, workspace: &Workspace, target: usize) {
        if !self.git_available() {
            return;
        }
        let Some(file) = self
            .git_files
            .list
            .as_ref()
            .and_then(|list| list.files.get(target))
            .cloned()
        else {
            return;
        };
        clear_selection(ctx);
        self.git_files.error = None;
        self.git_files.pending = Some(Pending {
            rx: start(ctx, workspace, file),
            target,
            owner: self.review.as_ref().unwrap().2.focus_id(),
        });
    }
    pub(super) fn git_file_inputs(&mut self, ctx: &egui::Context, workspace: &Workspace) {
        let Some(doc) = &self.review else {
            return;
        };
        if doc.3
            || !doc.2.owns_focus(ctx)
            || egui::Popup::is_any_open(ctx)
            || ctx.memory(|m| m.top_modal_layer().is_some())
        {
            return;
        }
        let mut target = self.git_files.position;
        let mut requested = None;
        let count = self
            .git_files
            .list
            .as_ref()
            .map_or(0, |list| list.files.len());
        let enabled = self.git_available();
        ctx.input_mut(|input| {
            input.events.retain(|event| {
                let egui::Event::Key {
                    key,
                    modifiers,
                    pressed: true,
                    ..
                } = event
                else {
                    return true;
                };
                if !modifiers.matches_exact(egui::Modifiers::ALT)
                    || !matches!(key, egui::Key::ArrowLeft | egui::Key::ArrowRight)
                {
                    return true;
                }
                if enabled {
                    let next = if *key == egui::Key::ArrowLeft {
                        target.and_then(|n| n.checked_sub(1))
                    } else {
                        target.map(|n| n + 1).filter(|n| *n < count)
                    };
                    if let Some(n) = next {
                        target = Some(n);
                        requested = Some(n);
                    }
                }
                false
            })
        });
        if let Some(target) = requested {
            self.select_git_file(ctx, workspace, target);
        }
    }
    pub(super) fn git_file_navigation(&self, ctx: &egui::Context) -> diff_viewer::FileNavigation {
        let count = self
            .git_files
            .list
            .as_ref()
            .map_or(0, |list| list.files.len());
        let position = self.git_files.position;
        let enabled = self.git_available()
            && ctx.memory(|memory| memory.top_modal_layer().is_none())
            && !egui::Popup::is_any_open(ctx);
        let status = if self.git_files.stale {
            "Git file list changed · reopen review to navigate".into()
        } else if let Some(pending) = &self.git_files.pending {
            format!("Loading Git file {} of {count}…", pending.target + 1)
        } else if let Some(position) = position {
            format!("Current Git file {} of {count}", position + 1)
        } else {
            "Git file navigation unavailable · requires ≤256 files / 64 KiB paths".into()
        };
        diff_viewer::FileNavigation {
            previous: enabled && position.is_some_and(|n| n > 0),
            next: enabled && position.is_some_and(|n| n + 1 < count),
            position: position.map(|position| (position, count)),
            warning: if self.git_files.stale || position.is_none() {
                Some(status.clone())
            } else {
                self.git_files.error.clone()
            },
            status,
            git: true,
            loading: self.git_files.pending.is_some(),
        }
    }
    pub(super) fn step_git_file(&mut self, ctx: &egui::Context, workspace: &Workspace, next: bool) {
        let target = self.git_files.position.and_then(|position| {
            if next {
                Some(position + 1)
            } else {
                position.checked_sub(1)
            }
        });
        if let Some(target) = target {
            self.select_git_file(ctx, workspace, target);
        }
    }
}
fn clear_selection(ctx: &egui::Context) {
    context_capture::clear_source(ctx);
    let mut selection = egui::text_selection::LabelSelectionState::load(ctx);
    selection.clear_selection();
    selection.store(ctx);
}
#[cfg(test)]
mod cancellation_tests;
#[cfg(test)]
mod lifecycle_tests;
#[cfg(test)]
mod tests;
#[cfg(test)]
mod ui_tests;
