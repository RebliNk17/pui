//! Sidebar projection only: one row per session, explicit ownership, no model calls
//! or filesystem scans. Task workers open their supervisor, never a second Pi owner.
use super::*;
use std::collections::{BTreeMap, BTreeSet};
use std::time::SystemTime;

#[derive(Clone, Debug, PartialEq)]
pub(super) enum Target {
    Session(usize),
    Agent(String),
    Task(String, Option<String>),
    Saved(PathBuf),
}
#[derive(Clone)]
pub(super) struct Branch {
    pub key: String,
    pub target: Target,
    pub path: Option<PathBuf>,
    pub title: String,
    pub status: String,
    pub cwd: PathBuf,
    pub project: PathBuf,
    pub modified: SystemTime,
    pub children: Vec<Branch>,
    pub pinned: bool,
    pub archived: bool,
    pub hidden: bool,
}
impl Branch {
    pub fn matches(&self, query: &str) -> bool {
        format!(
            "{} {} {}",
            self.title,
            self.project.display(),
            self.cwd.display()
        )
        .to_lowercase()
        .contains(query)
            || self.children.iter().any(|c| c.matches(query))
    }
    pub fn active(&self, app: &Pui) -> bool {
        match &self.target {
            Target::Task(task, run) => {
                app.tasks.open
                    && app.tasks.selected.as_ref() == Some(task)
                    && app.tasks.selected_run == *run
            }
            Target::Agent(id) => !app.tasks.open && app.agents.active == *id,
            _ => {
                !app.tasks.open
                    && self.path.as_ref().is_some_and(|p| {
                        app.state
                            .session_file
                            .as_deref()
                            .is_some_and(|s| path_key(std::path::Path::new(s)) == path_key(p))
                    })
            }
        }
    }
}
// These are the same macOS directory, without canonicalizing on every frame.
fn path_key(path: &std::path::Path) -> PathBuf {
    #[cfg(target_os = "macos")]
    if let Ok(suffix) = path.strip_prefix("/private/var") {
        return PathBuf::from("/var").join(suffix);
    }
    path.to_path_buf()
}
fn session_key(path: &std::path::Path) -> String {
    format!("session:{}", path_key(path).display())
}
fn agent_key(app: &Pui, id: &str) -> Option<String> {
    let node = app.agents.nodes.get(id)?;
    Some(
        node.path
            .as_deref()
            .map(session_key)
            .unwrap_or_else(|| format!("agent:{id}")),
    )
}
fn empty(
    key: String,
    target: Target,
    path: Option<PathBuf>,
    title: String,
    cwd: PathBuf,
    project: PathBuf,
) -> Branch {
    Branch {
        key,
        target,
        path,
        title,
        status: String::new(),
        cwd,
        project,
        modified: SystemTime::UNIX_EPOCH,
        children: Vec::new(),
        pinned: false,
        archived: false,
        hidden: false,
    }
}
fn preferences(app: &Pui, branch: &mut Branch) {
    if let Some(meta) = branch.path.as_ref().and_then(|p| app.prefs.chats.get(p)) {
        if let Some(title) = &meta.title {
            branch.title = title.clone();
        }
        if let Some(project) = &meta.project {
            branch.project = project.clone();
        }
        branch.pinned = meta.pinned;
        branch.hidden = meta.hidden;
        branch.archived = meta.archived;
    }
}
pub(super) fn forest(app: &Pui) -> Vec<Branch> {
    let mut rows = BTreeMap::<String, Branch>::new();
    let mut parents = BTreeMap::<String, String>::new();
    let mut order = Vec::new();
    for (index, s) in app.sessions.iter().enumerate() {
        let key = session_key(&s.path);
        if rows.contains_key(&key) {
            continue;
        }
        let mut row = empty(
            key.clone(),
            Target::Session(index),
            Some(s.path.clone()),
            s.title.clone(),
            s.cwd.clone(),
            s.project.clone(),
        );
        row.modified = s.modified;
        preferences(app, &mut row);
        order.push(key.clone());
        rows.insert(key, row);
    }
    // Reuse discovered session rows for known agents rather than duplicating a
    // second "agent workflows" list. Unsaved/queued workers still get a row.
    for node in app.agents.nodes.values() {
        let key = agent_key(app, &node.id).unwrap();
        if !rows.contains_key(&key) {
            let mut row = empty(
                key.clone(),
                Target::Agent(node.id.clone()),
                node.path.clone(),
                node.name.clone(),
                node.cwd.clone(),
                node.cwd.clone(),
            );
            preferences(app, &mut row);
            rows.insert(key.clone(), row);
            order.push(key.clone());
        }
        let row = rows.get_mut(&key).unwrap();
        row.status = node.status.clone();
        if node.parent.is_some() {
            row.target = Target::Agent(node.id.clone());
            if row
                .path
                .as_ref()
                .and_then(|path| app.prefs.chats.get(path))
                .is_none_or(|meta| meta.title.is_none())
            {
                row.title = node.name.clone();
            }
        }
        if let Some(parent) = node
            .parent
            .as_deref()
            .and_then(|p| agent_key(app, p))
            .filter(|p| *p != key)
        {
            parents.insert(key, parent);
        }
    }
    for task in &app.tasks.records {
        let key = format!("task:{}", task.id);
        let title = if task.objective.trim().is_empty() {
            format!("Task {}", &task.id[..task.id.len().min(8)])
        } else {
            format!("Task · {}", task.objective)
        };
        let mut row = empty(
            key.clone(),
            Target::Task(task.id.clone(), None),
            None,
            title,
            task.source.clone(),
            task.source.clone(),
        );
        row.status = task.phase.label().into();
        row.modified = SystemTime::UNIX_EPOCH
            .checked_add(Duration::from_secs(task.updated))
            .unwrap_or(SystemTime::UNIX_EPOCH);
        let parent = task
            .parent_session
            .as_deref()
            .map(|path| {
                let parent = session_key(path);
                if !rows.contains_key(&parent) {
                    let mut row = empty(
                        parent.clone(),
                        Target::Saved(path.to_path_buf()),
                        Some(path.to_path_buf()),
                        "Parent chat".into(),
                        task.source.clone(),
                        task.source.clone(),
                    );
                    preferences(app, &mut row);
                    rows.insert(parent.clone(), row);
                    order.push(parent.clone());
                }
                parent
            })
            .or_else(|| {
                app.tasks
                    .parents
                    .get(&task.id)
                    .and_then(|id| agent_key(app, id))
            });
        if let Some(parent) = parent.filter(|p| rows.contains_key(p)) {
            parents.insert(key.clone(), parent);
        }
        order.push(key.clone());
        rows.insert(key.clone(), row);
        for run in &task.runs {
            let run_key = run
                .session
                .as_deref()
                .map(session_key)
                .unwrap_or_else(|| format!("run:{}:{}", task.id, run.id));
            let label = format!(
                "{}{} · {}",
                if run.routing_for.is_some() {
                    "Routing: "
                } else {
                    ""
                },
                run.role.label(),
                run.attempt + 1
            );
            let mut row = empty(
                run_key.clone(),
                Target::Task(task.id.clone(), Some(run.id.clone())),
                run.session.clone(),
                label,
                task.workspace.as_ref().unwrap_or(&task.source).clone(),
                task.source.clone(),
            );
            preferences(app, &mut row);
            row.status = run.status.clone();
            row.modified = SystemTime::UNIX_EPOCH
                .checked_add(Duration::from_secs(task.updated))
                .unwrap_or(SystemTime::UNIX_EPOCH);
            // Worker rows always navigate to the existing task process/evidence,
            // including historical sessions that are also in the recent scan.
            if !rows.contains_key(&run_key) {
                order.push(run_key.clone());
            }
            rows.insert(run_key.clone(), row);
            parents.insert(run_key, key.clone());
        }
    }
    // Corrupt/restored cycles must never recurse forever or hide every row.
    let edges: Vec<_> = parents.keys().cloned().collect();
    for child in edges {
        let mut seen = BTreeSet::from([child.clone()]);
        let mut next = parents.get(&child);
        while let Some(parent) = next {
            if !seen.insert(parent.clone()) {
                parents.remove(&child);
                break;
            }
            next = parents.get(parent);
        }
    }
    let mut children = BTreeMap::<String, Vec<String>>::new();
    let mut roots = Vec::new();
    let mut seen = BTreeSet::new();
    for key in order {
        if !seen.insert(key.clone()) {
            continue;
        }
        if let Some(parent) = parents.get(&key).filter(|p| rows.contains_key(*p)) {
            children.entry(parent.clone()).or_default().push(key);
        } else {
            roots.push(key);
        }
    }
    fn take(
        key: &str,
        rows: &mut BTreeMap<String, Branch>,
        children: &mut BTreeMap<String, Vec<String>>,
        depth: usize,
    ) -> Option<Branch> {
        if depth > 128 {
            return None;
        }
        let mut row = rows.remove(key)?;
        for child in children.remove(key).unwrap_or_default() {
            if let Some(child) = take(&child, rows, children, depth + 1) {
                row.modified = row.modified.max(child.modified);
                row.children.push(child);
            }
        }
        Some(row)
    }
    let mut forest: Vec<_> = roots
        .into_iter()
        .filter_map(|key| take(&key, &mut rows, &mut children, 0))
        .collect();
    forest.sort_by_key(|r| (!r.pinned, std::cmp::Reverse(r.modified)));
    forest
}

#[cfg(test)]
#[path = "sidebar_tree_tests.rs"]
mod tests;
