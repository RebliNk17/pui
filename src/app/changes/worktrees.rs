//! Read-only linked Git worktree discovery and inspection for the Changes panel.
use super::*;
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};

#[derive(Clone, PartialEq, Eq)]
pub(super) struct WorktreeEntry {
    pub(super) path: PathBuf,
    pub(super) branch: String,
    pub(super) bare: bool,
}

impl WorktreeEntry {
    pub(super) fn short_name(&self) -> String {
        self.path
            .file_name()
            .and_then(|n| n.to_str())
            .map(|s| s.to_owned())
            .unwrap_or_else(|| self.path.display().to_string())
    }
}

#[derive(Default)]
pub(super) struct Worktrees {
    owner: Option<PathBuf>,
    list_rx: Option<workspace::read_only::Request<Result<Vec<WorktreeEntry>, String>>>,
    pub(super) entries: Vec<WorktreeEntry>,
    pub(super) list_error: Option<String>,
    pub(super) active_path: Option<PathBuf>,
    inspect_rx: Option<workspace::read_only::Request<Workspace>>,
    pub(super) inspected: Option<Workspace>,
}

impl Worktrees {
    pub(super) fn reset(&mut self) {
        *self = Self::default();
    }

    pub(super) fn clear_selection(&mut self) {
        self.active_path = None;
        self.inspect_rx = None;
        self.inspected = None;
    }

    pub(super) fn active_label(&self, workspace: &Workspace) -> String {
        if let Some(active) = &self.active_path
            && let Some(name) = active.file_name().and_then(|n| n.to_str())
        {
            return name.to_owned();
        }
        workspace
            .root
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("main")
            .to_owned()
    }

    pub(super) fn refresh_list(&mut self, ctx: &egui::Context, root: &std::path::Path) {
        self.owner = Some(root.to_path_buf());
        self.list_error = None;
        let (tx, rx) = mpsc::channel();
        let cancel = Arc::new(AtomicBool::new(false));
        self.list_rx = Some(workspace::read_only::Request::new(rx, cancel.clone()));
        let ctx = ctx.clone();
        let root = root.to_path_buf();
        std::thread::spawn(move || {
            let result = list_worktrees(&root);
            if !cancel.load(Ordering::Acquire) {
                let _ = tx.send(result);
                ctx.request_repaint();
            }
        });
    }

    pub(super) fn select(&mut self, ctx: &egui::Context, path: PathBuf) {
        self.active_path = Some(path.clone());
        self.inspected = None;
        self.inspect_rx = Some(git_files::start_workspace(ctx, path));
    }

    pub(super) fn poll(&mut self, ctx: &egui::Context, workspace: &Workspace, open: bool) {
        if workspace.is_git && self.owner.as_deref() != Some(workspace.root.as_path()) {
            self.entries.clear();
            self.list_error = None;
            self.owner = Some(workspace.root.clone());
        }
        if open
            && workspace.is_git
            && self.list_rx.is_none()
            && self.entries.is_empty()
            && self.list_error.is_none()
        {
            self.refresh_list(ctx, &workspace.root);
        }
        if let Some(result) = self.list_rx.as_ref().and_then(|rx| match rx.try_recv() {
            Ok(r) => Some(r),
            Err(mpsc::TryRecvError::Empty) => None,
            Err(_) => Some(Err("Worktree list worker ended.".into())),
        }) {
            self.list_rx = None;
            match result {
                Ok(entries) => {
                    self.entries = entries;
                    self.list_error = None;
                }
                Err(err) => self.list_error = Some(err),
            }
        }
        if let Some(ws) = self.inspect_rx.as_ref().and_then(|rx| match rx.try_recv() {
            Ok(ws) => Some(ws),
            Err(mpsc::TryRecvError::Empty) => None,
            Err(_) => None,
        }) {
            self.inspect_rx = None;
            self.inspected = Some(ws);
        }
    }

    pub(super) fn loading_list(&self) -> bool {
        self.list_rx.is_some()
    }

    pub(super) fn loading_workspace(&self) -> bool {
        self.inspect_rx.is_some()
    }
}

fn list_worktrees(root: &std::path::Path) -> Result<Vec<WorktreeEntry>, String> {
    let stdout = workspace::read_only::worktree_list_porcelain(root)?;
    Ok(parse_porcelain(&stdout))
}

fn parse_porcelain(stdout: &str) -> Vec<WorktreeEntry> {
    let mut entries = Vec::new();
    let mut current_path: Option<PathBuf> = None;
    let mut current_branch = String::new();
    let mut bare = false;

    let flush = |entries: &mut Vec<WorktreeEntry>,
                 path: &mut Option<PathBuf>,
                 branch: &mut String,
                 bare: &mut bool| {
        if let Some(p) = path.take() {
            let label = if *bare {
                "(bare)".into()
            } else if branch.is_empty() {
                "(detached)".into()
            } else {
                branch.clone()
            };
            entries.push(WorktreeEntry {
                path: p,
                branch: label,
                bare: *bare,
            });
        }
        branch.clear();
        *bare = false;
    };

    for line in stdout.lines() {
        let line = line.trim();
        if line.is_empty() {
            flush(&mut entries, &mut current_path, &mut current_branch, &mut bare);
            continue;
        }
        if let Some(path) = line.strip_prefix("worktree ") {
            flush(&mut entries, &mut current_path, &mut current_branch, &mut bare);
            current_path = Some(PathBuf::from(path));
        } else if let Some(branch_ref) = line.strip_prefix("branch ") {
            current_branch = branch_ref
                .strip_prefix("refs/heads/")
                .unwrap_or(branch_ref)
                .to_string();
        } else if line == "bare" {
            bare = true;
        }
    }
    flush(&mut entries, &mut current_path, &mut current_branch, &mut bare);
    entries
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_git_worktree_porcelain_output() {
        let sample = "\
worktree /repo/main
HEAD 1111111
branch refs/heads/master

worktree /repo/wt-1
HEAD 2222222
branch refs/heads/feat/ui

worktree /repo/detached
HEAD 3333333
detached
";
        let entries = parse_porcelain(sample);
        assert_eq!(entries.len(), 3);
        assert_eq!(entries[0].path, PathBuf::from("/repo/main"));
        assert_eq!(entries[0].branch, "master");
        assert_eq!(entries[1].path, PathBuf::from("/repo/wt-1"));
        assert_eq!(entries[1].branch, "feat/ui");
        assert_eq!(entries[2].branch, "(detached)");
    }
}
