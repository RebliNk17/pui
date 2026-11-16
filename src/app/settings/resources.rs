//! Native resource manager. Discovery/review/apply use isolated workers, never chat RPC.
use super::*;
use std::{collections::BTreeMap, path::Path};
mod dialogs;
mod engineering;
#[cfg(test)]
mod tests;
mod transport;
mod view;

const KINDS: [(&str, &str, Icon); 7] = [
    ("", "All resources", Icon::Board),
    ("skills", "Skills", Icon::Spark),
    ("mcp", "MCP servers", Icon::Globe),
    ("extensions", "Extensions", Icon::Code),
    ("prompts", "Prompts", Icon::File),
    ("themes", "CLI themes", Icon::Sliders),
    ("packages", "Packages", Icon::Folder),
];
#[derive(Clone, Default, Deserialize)]
#[serde(default, rename_all = "camelCase")]
struct Catalog {
    revision: String,
    trusted: bool,
    items: Vec<Resource>,
    warnings: Vec<String>,
    user_config: String,
    project_config: String,
}
#[derive(Clone, Default, Deserialize)]
#[serde(default, rename_all = "camelCase")]
struct Resource {
    id: String,
    kind: String,
    scope: String,
    name: String,
    description: String,
    path: String,
    source: String,
    config: String,
    enabled: bool,
    removable: bool,
    inherited: bool,
    personal_enabled: Option<bool>,
    origin_scope: String,
    project_state: String,
    source_label: String,
    owner: String,
    installed: bool,
    issue: bool,
    version: String,
    resource_count: usize,
    transport: String,
    exposure: String,
}
impl Resource {
    fn kind_label(&self) -> &str {
        KINDS
            .iter()
            .find(|k| k.0 == self.kind)
            .map(|k| k.1)
            .unwrap_or("Resource")
    }
    fn icon(&self) -> Icon {
        KINDS
            .iter()
            .find(|k| k.0 == self.kind)
            .map(|k| k.2)
            .unwrap_or(Icon::File)
    }
    fn source_label(&self) -> &str {
        if self.source_label.is_empty() {
            &self.source
        } else {
            &self.source_label
        }
    }
    fn project_state(&self) -> &str {
        if self.project_state.is_empty() {
            "inherit"
        } else {
            &self.project_state
        }
    }
}
#[derive(Clone, Copy, Default, PartialEq, Eq, Debug, Hash)]
enum Mode {
    #[default]
    Library,
    Workspace,
    Packages,
}
impl Mode {
    const ALL: [(Self, &'static str); 3] = [
        (Self::Library, "Library"),
        (Self::Workspace, "Workspace"),
        (Self::Packages, "Packages & sources"),
    ];
}
#[derive(Clone)]
struct Draft {
    key: String,
    title: String,
    request: Value,
}
#[derive(Clone, Default, Deserialize)]
struct Review {
    token: String,
    steps: Vec<ReviewStep>,
}
#[derive(Clone, Default, Deserialize)]
#[serde(default)]
struct ReviewStep {
    index: usize,
    title: String,
    file: String,
    operation: String,
    before: String,
    after: String,
    redacted: bool,
    diff: String,
    affected: Vec<String>,
}
#[derive(Clone, Default, Deserialize)]
struct Preview {
    id: String,
    content: String,
    path: String,
}
#[derive(Clone, Deserialize)]
struct Outcome {
    index: usize,
    success: bool,
    message: String,
}
#[derive(Clone, Copy, PartialEq)]
enum JobKind {
    List,
    Inspect,
    Review,
    Apply,
}
struct Job {
    cwd: PathBuf,
    kind: JobKind,
    rx: Receiver<Result<Value, String>>,
}
#[derive(Default)]
struct Form {
    open: bool,
    kind: usize,
    scope: String,
    source: String,
    name: String,
    transport: usize,
    url: String,
    command: String,
    args: String,
    exposure: usize,
    advanced: String,
    edit_id: Option<String>,
    engineering_selection: usize,
    error: Option<String>,
}
#[derive(Default)]
pub(in crate::app) struct Resources {
    cwd: Option<PathBuf>,
    catalog: Option<Catalog>,
    job: Option<Job>,
    error: Option<String>,
    notice: Option<String>,
    mode: Mode,
    kind: usize,
    query: String,
    state_filter: usize,
    origin_filter: usize,
    selected: String,
    source_group: String,
    source_tab: bool,
    drafts: Vec<Draft>,
    draft_revision: String,
    review: Option<Review>,
    review_open: bool,
    outcomes: Vec<Outcome>,
    previews: BTreeMap<String, Preview>,
    form: Form,
    remove: Option<Resource>,
}
impl Resources {
    pub(in crate::app) fn poll(&mut self) {
        let Some(result) = self.job.as_ref().and_then(|j| match j.rx.try_recv() {
            Ok(result) => Some(result),
            Err(mpsc::TryRecvError::Disconnected) => Some(Err(
                "Resource helper stopped. Refresh and review before retrying.".into(),
            )),
            Err(mpsc::TryRecvError::Empty) => None,
        }) else {
            return;
        };
        let job = self.job.take().unwrap();
        if self.cwd.as_ref() != Some(&job.cwd) {
            return;
        }
        match result {
            Err(error) => {
                self.error = Some(error);
                if job.kind == JobKind::Apply {
                    self.review = None;
                    self.catalog = None;
                }
            }
            Ok(value) => match job.kind {
                JobKind::List => match serde_json::from_value(value) {
                    Ok(catalog) => self.catalog = Some(catalog),
                    Err(_) => {
                        self.error =
                            Some("Unrecognized Pi catalog. Check SDK compatibility.".into())
                    }
                },
                JobKind::Inspect => match serde_json::from_value::<Preview>(value) {
                    Ok(p) => {
                        if self.previews.len() >= 8 && !self.previews.contains_key(&p.id) {
                            self.previews.clear();
                        }
                        self.previews.insert(p.id.clone(), p);
                    }
                    Err(_) => self.error = Some("Unrecognized preview response.".into()),
                },
                JobKind::Review => match serde_json::from_value(value) {
                    Ok(review) => {
                        self.review = Some(review);
                        self.review_open = true;
                    }
                    Err(_) => self.error = Some("Unrecognized review response.".into()),
                },
                JobKind::Apply => {
                    let outcomes: Vec<Outcome> =
                        serde_json::from_value(value["results"].clone()).unwrap_or_default();
                    let successful: Vec<_> = outcomes
                        .iter()
                        .filter(|r| r.success)
                        .filter_map(|r| self.drafts.get(r.index).map(|d| d.key.clone()))
                        .collect();
                    self.drafts.retain(|d| !successful.contains(&d.key));
                    self.outcomes = outcomes;
                    self.review = None;
                    self.catalog = None;
                    self.previews.clear();
                    self.notice = Some(if value["complete"] == true { "Configuration saved. In each existing idle chat, use /reload or Reload chat resources… in skill suggestions to load changes. Other chats are unchanged." } else { "Stopped after an unsuccessful operation. Earlier operations may have completed. Refresh and review remaining drafts; nothing is retried automatically." }.into());
                }
            },
        }
    }
    fn start(&mut self, ctx: &egui::Context, kind: JobKind, request: Value) {
        if self.job.is_some() {
            return;
        }
        let cwd = self.cwd.clone().unwrap_or_default();
        let target = cwd.clone();
        let ctx = ctx.clone();
        let (tx, rx) = mpsc::channel();
        std::thread::spawn(move || {
            let _ = tx.send(transport::run_helper(&target, &request));
            ctx.request_repaint();
        });
        self.error = None;
        self.job = Some(Job { cwd, kind, rx });
    }
    fn sync_workspace(&mut self, cwd: &Path, demo: bool) {
        if self.cwd.as_deref() != Some(cwd) {
            let job = self.job.take(); // allow the old worker to finish; discard its receiver/result
            let mode = self.mode;
            *self = Self {
                cwd: Some(cwd.to_path_buf()),
                mode,
                ..Default::default()
            };
            drop(job);
        }
        if demo && self.catalog.is_none() {
            self.catalog = Some(demo_catalog());
        }
    }
    fn stage(&mut self, key: String, title: String, request: Value) {
        if self.drafts.is_empty() {
            self.draft_revision = self
                .catalog
                .as_ref()
                .map(|c| c.revision.clone())
                .unwrap_or_default();
        }
        self.review = None;
        self.outcomes.clear();
        self.drafts.retain(|d| d.key != key);
        if request["action"] == "remove" {
            self.drafts.retain(|d| d.request["id"] != request["id"]);
        } else if request["id"].is_string()
            && self
                .drafts
                .iter()
                .any(|d| d.request["action"] == "remove" && d.request["id"] == request["id"])
        {
            self.error = Some("Remove the staged removal before editing this resource.".into());
            return;
        }
        if self.drafts.len() < 32 {
            self.drafts.push(Draft {
                key,
                title,
                request,
            });
        } else {
            self.error = Some("Apply or discard drafts before adding more than 32 changes.".into());
        }
    }
    fn selection(&self, item: &Resource, scope: &str) -> String {
        self.drafts
            .iter()
            .find(|d| {
                d.request["action"] == "select"
                    && d.request["id"] == item.id
                    && d.request["scope"] == scope
            })
            .and_then(|d| d.request["state"].as_str())
            .map(str::to_owned)
            .unwrap_or_else(|| {
                if scope == "user" {
                    if item.personal_enabled.unwrap_or(item.enabled) {
                        "on"
                    } else {
                        "off"
                    }
                    .into()
                } else {
                    item.project_state().into()
                }
            })
    }
    fn effective(&self, item: &Resource) -> bool {
        if item.issue {
            return false;
        }
        match self.selection(item, "project").as_str() {
            "on" => true,
            "off" => false,
            _ => {
                if item.inherited {
                    self.selection(item, "user") == "on"
                } else {
                    item.enabled
                }
            }
        }
    }
    fn select_state(&mut self, item: &Resource, scope: &str, state: &str) {
        let key = format!("select:{}:{scope}", item.id);
        let original = if scope == "user" {
            if item.personal_enabled.unwrap_or(item.enabled) {
                "on"
            } else {
                "off"
            }
        } else {
            item.project_state()
        };
        self.review = None;
        if state == original {
            self.drafts.retain(|d| d.key != key);
        } else {
            self.stage(
                key,
                format!(
                    "{} · {}",
                    item.name,
                    if scope == "project" {
                        "project selection"
                    } else {
                        "personal default"
                    }
                ),
                json!({"action":"select","id":item.id,"scope":scope,"state":state}),
            );
        }
    }
    fn request_review(&mut self, ctx: &egui::Context, demo: bool) {
        if self.drafts.is_empty() {
            return;
        }
        self.review_open = true;
        if demo {
            self.review = Some(Review {
                token: "demo".into(),
                steps: self
                    .drafts
                    .iter()
                    .enumerate()
                    .map(|(index, d)| ReviewStep {
                        index,
                        title: d.title.clone(),
                        file: if d.request["scope"] == "project" {
                            "./.pi/settings.json"
                        } else {
                            "~/.pi/agent/settings.json"
                        }
                        .into(),
                        before: "Demo configuration".into(),
                        after: serde_json::to_string_pretty(&d.request).unwrap_or_default(),
                        ..Default::default()
                    })
                    .collect(),
            });
        } else {
            self.start(ctx,JobKind::Review,json!({"action":"preview","revision":self.catalog.as_ref().map(|c| c.revision.clone()).unwrap_or_default(),"operations":self.drafts.iter().map(|d| &d.request).collect::<Vec<_>>() }));
        }
    }
    pub(in crate::app) fn navigation_view(&self) -> &'static str {
        match self.mode {
            Mode::Library => "Library",
            Mode::Workspace => "Workspace",
            Mode::Packages => "Packages & sources",
        }
    }
    pub(in crate::app) fn navigation_blocked(&self) -> bool {
        self.form.open || self.remove.is_some() || self.review_open
    }
    pub(in crate::app) fn select_view(&mut self, name: &str) {
        self.mode = match name.to_lowercase().as_str() {
            "workspace" => Mode::Workspace,
            "packages" | "packages & sources" => Mode::Packages,
            _ => Mode::Library,
        };
    }
    pub(super) fn escape(&mut self) -> bool {
        if self.form.open {
            self.form.open = false;
            return true;
        }
        if self.remove.is_some() {
            self.remove = None;
            return true;
        }
        if self.review_open {
            self.review_open = false;
            return true;
        }
        false
    }
    fn draft_for(&self, id: &str) -> bool {
        self.drafts.iter().any(|d| d.request["id"] == id)
    }
}
fn demo_catalog() -> Catalog {
    let fixtures = [
        (
            "skills",
            "code-review",
            "Review changes for correctness, security and maintainability.",
            true,
            "user",
            "developer-tools",
        ),
        (
            "skills",
            "browser-testing",
            "Check local web workflows and capture browser evidence.",
            true,
            "project",
            "local",
        ),
        (
            "mcp",
            "Documentation",
            "Search your configured documentation service.",
            true,
            "user",
            "mcp",
        ),
        (
            "skills",
            "release-notes",
            "Prepare a structured release summary from reviewed changes.",
            false,
            "user",
            "developer-tools",
        ),
        (
            "extensions",
            "workspace-tools",
            "Adds repository commands to Pi.",
            true,
            "user",
            "developer-tools",
        ),
        (
            "prompts",
            "review-pr",
            "A reusable review prompt with a focus argument.",
            true,
            "user",
            "developer-tools",
        ),
        (
            "themes",
            "midnight",
            "A dark theme for the Pi terminal interface, not Pui.",
            true,
            "user",
            "local",
        ),
        (
            "mcp",
            "Filesystem",
            "Local file tools through a configured stdio server.",
            false,
            "project",
            "mcp",
        ),
        (
            "packages",
            "Developer tools",
            "Installed package with four resolved resources.",
            true,
            "user",
            "developer-tools",
        ),
        (
            "packages",
            "Team helpers",
            "Configured source is not installed locally.",
            false,
            "user",
            "missing",
        ),
        (
            "extensions",
            "builtin:codemode",
            "Pi's built-in JavaScript tool coordinator.",
            true,
            "user",
            "builtin",
        ),
    ];
    Catalog {
        revision: "demo".into(),
        trusted: true,
        user_config: "~/.pi/agent/settings.json".into(),
        project_config: "./.pi/settings.json".into(),
        items: fixtures
            .into_iter()
            .enumerate()
            .map(
                |(i, (kind, name, description, enabled, scope, owner))| Resource {
                    id: format!("demo-{i}"),
                    kind: kind.into(),
                    name: name.into(),
                    description: description.into(),
                    enabled,
                    scope: scope.into(),
                    origin_scope: scope.into(),
                    inherited: scope == "user" && kind != "packages",
                    personal_enabled: if scope == "user" { Some(enabled) } else { None },
                    project_state: if scope == "project" {
                        if enabled { "on" } else { "off" }
                    } else if i == 4 {
                        "off"
                    } else {
                        "inherit"
                    }
                    .into(),
                    path: format!("~/.pi/agent/{kind}/{name}"),
                    source: if owner == "developer-tools" {
                        "git:github.com/example/developer-tools@v1.4.2"
                    } else {
                        "Local configuration"
                    }
                    .into(),
                    source_label: match owner {
                        "developer-tools" => "Developer tools",
                        "builtin" => "Built into Pi",
                        "mcp" => "MCP configuration",
                        _ => "Local resource",
                    }
                    .into(),
                    owner: if owner == "developer-tools" {
                        "developer-tools"
                    } else {
                        ""
                    }
                    .into(),
                    config: if kind == "mcp" {
                        "~/.pi/agent/mcp.json"
                    } else {
                        "~/.pi/agent/settings.json"
                    }
                    .into(),
                    removable: owner != "builtin",
                    installed: enabled,
                    issue: owner == "missing",
                    version: if kind == "packages" && enabled {
                        "1.4.2"
                    } else {
                        ""
                    }
                    .into(),
                    resource_count: if owner == "developer-tools" && kind == "packages" {
                        4
                    } else {
                        0
                    },
                    transport: if i == 7 { "stdio" } else { "HTTP" }.into(),
                    exposure: "codemode".into(),
                },
            )
            .collect(),
        ..Default::default()
    }
}
