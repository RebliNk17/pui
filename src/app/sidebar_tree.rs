//! Sidebar projection only: one row per session, explicit ownership, no model calls
//! or filesystem scans. Task workers open their supervisor, never a second Pi owner.
use super::*;
use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::hash::{Hash, Hasher};
use std::time::SystemTime;

#[derive(Default)]
pub(super) struct ListingCache {
    stamp: Option<u64>,
    pinned: Vec<Branch>,
    branches: Vec<Branch>,
}

pub(super) fn same_path(a: &std::path::Path, b: &std::path::Path) -> bool {
    a.as_os_str() == b.as_os_str() || (a.file_name() == b.file_name() && a == b)
}

fn path_key_eq(a: &std::path::Path, b: &std::path::Path) -> bool {
    a.as_os_str() == b.as_os_str() || (a.file_name() == b.file_name() && path_key(a) == path_key(b))
}

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
    pub created: Option<SystemTime>,
    pub modified: SystemTime,
    /// This row's own observed activity, never its descendants' sorting time.
    pub activity: Option<SystemTime>,
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
        if app.kanban.open {
            return false;
        }
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
                            .is_some_and(|s| path_key_eq(std::path::Path::new(s), p))
                    })
            }
        }
    }
}
pub(super) fn cached_listing(app: &mut Pui) -> (Vec<Branch>, Vec<Branch>) {
    let current = stamp(app);
    if app.sidebar_cache.stamp == Some(current) {
        return (
            app.sidebar_cache.pinned.clone(),
            app.sidebar_cache.branches.clone(),
        );
    }
    let (pinned, branches) = listing(app);
    app.sidebar_cache = ListingCache {
        stamp: Some(current),
        pinned: pinned.clone(),
        branches: branches.clone(),
    };
    (pinned, branches)
}
fn stamp(app: &Pui) -> u64 {
    let mut h = std::collections::hash_map::DefaultHasher::new();
    let hash_path = |p: &std::path::Path, h: &mut std::collections::hash_map::DefaultHasher| {
        p.as_os_str().as_encoded_bytes().hash(h);
    };
    let hash_state = |s: &State, h: &mut std::collections::hash_map::DefaultHasher| {
        s.session_file.hash(h);
        s.assigned_title().hash(h);
        s.title_request.is_some().hash(h);
        s.session_name.hash(h);
        s.messages.last().map(|m| m.timestamp).hash(h);
    };
    app.show_hidden.hash(&mut h);
    app.sessions.len().hash(&mut h);
    for s in &app.sessions {
        hash_path(&s.path, &mut h);
        hash_path(&s.cwd, &mut h);
        hash_path(&s.project, &mut h);
        s.title.hash(&mut h);
        s.created.hash(&mut h);
        s.modified.hash(&mut h);
    }
    app.prefs.chats.len().hash(&mut h);
    for (path, meta) in &app.prefs.chats {
        hash_path(path, &mut h);
        meta.title.hash(&mut h);
        meta.initial_title.hash(&mut h);
        meta.project
            .as_deref()
            .map(|p| p.as_os_str().as_encoded_bytes())
            .hash(&mut h);
        meta.pinned.hash(&mut h);
        meta.hidden.hash(&mut h);
        meta.archived.hash(&mut h);
    }
    app.agents.active.hash(&mut h);
    hash_state(&app.state, &mut h);
    app.agents.nodes.len().hash(&mut h);
    for node in app.agents.nodes.values() {
        node.id.hash(&mut h);
        node.parent.hash(&mut h);
        node.name.hash(&mut h);
        node.status.hash(&mut h);
        hash_path(&node.cwd, &mut h);
        node.path
            .as_deref()
            .map(|p| p.as_os_str().as_encoded_bytes())
            .hash(&mut h);
        node.created.hash(&mut h);
        node.last_activity.hash(&mut h);
    }
    app.agents.parked.len().hash(&mut h);
    for (id, chat) in &app.agents.parked {
        id.hash(&mut h);
        hash_state(&chat.state, &mut h);
    }
    app.tasks.records.len().hash(&mut h);
    for task in &app.tasks.records {
        task.id.hash(&mut h);
        task.objective.hash(&mut h);
        task.phase.label().hash(&mut h);
        task.updated.hash(&mut h);
        hash_path(&task.source, &mut h);
        task.workspace
            .as_deref()
            .map(|p| p.as_os_str().as_encoded_bytes())
            .hash(&mut h);
        task.parent_session
            .as_deref()
            .map(|p| p.as_os_str().as_encoded_bytes())
            .hash(&mut h);
        task.runs.len().hash(&mut h);
        for run in &task.runs {
            run.id.hash(&mut h);
            run.routing_for.is_some().hash(&mut h);
            run.role.label().hash(&mut h);
            run.attempt.hash(&mut h);
            run.status.hash(&mut h);
            run.session
                .as_deref()
                .map(|p| p.as_os_str().as_encoded_bytes())
                .hash(&mut h);
        }
    }
    app.tasks.parents.hash(&mut h);
    h.finish()
}
/// Pull pinned descendants into a dedicated section and expose hidden descendants
/// without requiring their visible parent to be shown. No duplicated row IDs.
pub(super) fn listing(app: &Pui) -> (Vec<Branch>, Vec<Branch>) {
    fn project(mut branch: Branch, show_hidden: bool, pinned: &mut Vec<Branch>) -> Vec<Branch> {
        branch.children = branch
            .children
            .into_iter()
            .flat_map(|child| project(child, show_hidden, pinned))
            .collect();
        if branch.pinned {
            pinned.push(branch);
            Vec::new()
        } else if branch.hidden != show_hidden {
            branch.children
        } else {
            vec![branch]
        }
    }
    let mut pinned = Vec::new();
    let rows = forest(app)
        .into_iter()
        .flat_map(|branch| project(branch, app.show_hidden, &mut pinned))
        .collect();
    (pinned, rows)
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
        created: None,
        modified: SystemTime::UNIX_EPOCH,
        activity: None,
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
    let mut live_titles = HashMap::with_capacity(app.agents.parked.len() + 1);
    for chat in app.agents.parked.values() {
        if let Some(path) = chat.state.session_file.as_deref() {
            live_titles.insert(std::path::Path::new(path), chat.state.chat_title());
        }
    }
    if let Some(path) = app.state.session_file.as_deref() {
        live_titles.insert(std::path::Path::new(path), app.state.chat_title());
    }
    for (index, s) in app.sessions.iter().enumerate() {
        let key = session_key(&s.path);
        if rows.contains_key(&key) {
            continue;
        }
        let meta = app.prefs.chats.get(&s.path);
        let title = meta
            .and_then(|m| m.title.clone().or_else(|| m.initial_title.clone()))
            .or_else(|| live_titles.get(s.path.as_path()).cloned().flatten())
            .unwrap_or_else(|| s.title.clone());
        let mut row = empty(
            key.clone(),
            Target::Session(index),
            Some(s.path.clone()),
            title,
            s.cwd.clone(),
            s.project.clone(),
        );
        row.created = s.created;
        row.modified = s.modified;
        row.activity = (s.modified > SystemTime::UNIX_EPOCH).then_some(s.modified);
        if let Some(meta) = meta {
            if let Some(project) = &meta.project {
                row.project = project.clone();
            }
            row.pinned = meta.pinned;
            row.hidden = meta.hidden;
            row.archived = meta.archived;
        }
        order.push(key.clone());
        rows.insert(key.clone(), row);
        if let Some(parent_path) = app
            .prefs
            .chats
            .get(&s.path)
            .and_then(|meta| meta.parent_session.as_ref())
        {
            let parent_key = session_key(parent_path);
            if parent_key != key {
                parents.insert(key, parent_key);
            }
        }
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
        let state = if node.id == app.agents.active {
            Some(&app.state)
        } else {
            app.agents.parked.get(&node.id).map(|chat| &chat.state)
        };
        // History is already loaded; inspecting its last message is O(1), not
        // a transcript scan or file read. Events supply fresh tool/stream time.
        let message_time = state
            .and_then(|state| state.messages.last())
            .and_then(|message| u64::try_from(message.timestamp).ok())
            .filter(|timestamp| *timestamp > 0)
            .and_then(|timestamp| {
                SystemTime::UNIX_EPOCH.checked_add(Duration::from_millis(timestamp))
            });
        row.created = row.created.or(node.created);
        row.activity = row
            .activity
            .max(node.created)
            .max(node.last_activity)
            .max(message_time);
        row.modified = row
            .modified
            .max(row.activity.unwrap_or(SystemTime::UNIX_EPOCH));
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
            format!("Workflow {}", &task.id[..task.id.len().min(8)])
        } else {
            format!("Workflow · {}", task.objective)
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
        row.activity = (task.updated > 0).then_some(row.modified);
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
            // A workflow update is not a per-worker activity timestamp. Keep
            // the run's own discovered session date only, if one is available.
            row.activity = rows.get(&run_key).and_then(|existing| existing.activity);
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
