//! Settings resource browser. Helpers never share the chat RPC or interrupt agents.
use super::page::{card, note, row, section, separator, toggle};
use super::*;
use std::{
    io::{Read, Write},
    path::Path,
    process::{Command, Stdio},
    time::Instant,
};

const KINDS: [(&str, &str); 6] = [
    ("skills", "Skills"),
    ("mcp", "MCP servers"),
    ("extensions", "Extensions"),
    ("prompts", "Prompts"),
    ("themes", "Themes"),
    ("packages", "Packages"),
];
#[derive(Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Catalog {
    revision: String,
    trusted: bool,
    items: Vec<Resource>,
    warnings: Vec<String>,
    user_config: String,
    project_config: String,
}
#[derive(Clone, Default, Deserialize)]
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
}
struct Pending {
    request: Value,
    title: String,
    description: String,
}
struct Job {
    cwd: PathBuf,
    mutation: bool,
    rx: Receiver<Result<Value, String>>,
}
#[derive(Default)]
pub(in crate::app) struct Resources {
    cwd: Option<PathBuf>,
    catalog: Option<Catalog>,
    job: Option<Job>,
    error: Option<String>,
    notice: Option<String>,
    pending: Option<Pending>,
    kind: usize,
    query: String,
    scope: String,
    adding: bool,
    source: String,
    name: String,
    definition: String,
    edit_id: Option<String>,
    expanded: std::collections::BTreeSet<String>,
}
impl Resources {
    pub(in crate::app) fn poll(&mut self) {
        let Some(result) = self.job.as_ref().and_then(|job| match job.rx.try_recv() {
            Ok(result) => Some(result),
            Err(mpsc::TryRecvError::Disconnected) => Some(Err(
                "Resource helper stopped. Refresh before retrying.".into(),
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
            Ok(_) if job.mutation => {
                self.catalog = None;
                self.adding = false;
                self.source.clear();
                self.name.clear();
                self.definition.clear();
                self.edit_id = None;
                self.notice = Some("Configuration saved. Start a new chat / reopen Pui to load changes. Running agents are unchanged.".into());
            }
            Ok(data) => match serde_json::from_value(data) {
                Ok(catalog) => self.catalog = Some(catalog),
                Err(_) => {
                    self.error =
                        Some("Unrecognized Pi resource catalog. Check SDK compatibility.".into())
                }
            },
            Err(error) => self.error = Some(error),
        }
    }
    fn start(&mut self, ui: &egui::Ui, request: Value) {
        if self.job.is_some() {
            return;
        }
        let cwd = self.cwd.clone().unwrap_or_default();
        let mutation = request["action"]
            .as_str()
            .is_some_and(|action| action != "list");
        let (tx, rx) = mpsc::channel();
        let ctx = ui.ctx().clone();
        let target = cwd.clone();
        std::thread::spawn(move || {
            let result = run_helper(&target, &request);
            let _ = tx.send(result);
            ctx.request_repaint();
        });
        self.error = None;
        self.job = Some(Job { cwd, mutation, rx });
    }
    fn stage(&mut self, mut request: Value, title: String, description: String) {
        request["revision"] = self
            .catalog
            .as_ref()
            .map(|c| c.revision.clone())
            .unwrap_or_default()
            .into();
        self.pending = Some(Pending {
            request,
            title,
            description,
        });
    }
    pub(super) fn show(&mut self, ui: &mut egui::Ui, cwd: &Path, demo: bool) {
        if self.cwd.as_deref() != Some(cwd) {
            self.cwd = Some(cwd.to_path_buf());
            self.catalog = None;
            self.error = None;
            self.notice = None;
            self.pending = None;
            self.adding = false;
            self.edit_id = None;
            self.definition.clear();
            self.source.clear();
            self.name.clear();
            self.scope = "user".into();
            self.expanded.clear();
        }
        if demo && self.catalog.is_none() {
            self.catalog = Some(demo_catalog());
        }
        if !demo && self.catalog.is_none() && self.job.is_none() && self.error.is_none() {
            self.start(ui, json!({"action":"list"}));
        }
        let colors = theme::palette(ui.ctx());
        section(
            ui,
            "Configuration",
            "Changes apply to new chats. Running agents are left unchanged.",
        );
        card(ui, |ui| {
            row(
                ui,
                "Pi configuration",
                if demo {
                    "Demo fixtures · no configuration changes"
                } else {
                    "Configured resources, not live connection status."
                },
                |ui| {
                    if ui
                        .add_enabled(
                            self.job.is_none() && self.pending.is_none(),
                            egui::Button::new("Refresh"),
                        )
                        .clicked()
                    {
                        self.catalog = if demo { Some(demo_catalog()) } else { None };
                        self.error = None;
                    }
                },
            );
            separator(ui);
            egui::Frame::new()
                .inner_margin(egui::Margin::symmetric(18, 14))
                .show(ui, |ui| {
                    ui.spacing_mut().item_spacing.y = 10.0;
                    ui.label(theme::caption(ui, "WORKSPACE"));
                    ui.add(
                        egui::Label::new(theme::small(ui, cwd.display().to_string()))
                            .truncate()
                            .selectable(true),
                    )
                    .on_hover_text(cwd.display().to_string());
                    if let Some(catalog) = &self.catalog {
                        ui.horizontal_wrapped(|ui| {
                            for (label, file) in [
                                ("Personal config", &catalog.user_config),
                                ("Project config", &catalog.project_config),
                            ] {
                                if ui
                                    .add_enabled(
                                        !demo && Path::new(file).is_file(),
                                        egui::Button::new(label),
                                    )
                                    .on_hover_text(file)
                                    .clicked()
                                {
                                    open_config(PathBuf::from(file));
                                }
                            }
                        });
                        for warning in &catalog.warnings {
                            ui.add(
                                egui::Label::new(
                                    RichText::new(warning).size(11.5).color(colors.amber),
                                )
                                .wrap(),
                            );
                        }
                    }
                });
        });
        if let Some(notice) = &self.notice {
            ui.label(RichText::new(notice).size(12.0).color(colors.accent));
        }
        if let Some(error) = &self.error {
            ui.label(RichText::new(error).size(12.0).color(colors.red));
        }
        if let Some(job) = &self.job {
            ui.horizontal(|ui| {
                ui.spinner();
                ui.label(theme::small(
                    ui,
                    if job.mutation {
                        "Applying resource change…"
                    } else {
                        "Discovering configured resources…"
                    },
                ));
            });
        }
        if self.pending.is_some() {
            self.confirmation(ui, demo);
        }
        let locked = self.job.is_some() || self.pending.is_some();
        section(
            ui,
            "Resource library",
            "Browse by type, then search by name, source or scope.",
        );
        ui.add_enabled_ui(!locked, |ui| {
            ui.horizontal_wrapped(|ui| {
                ui.spacing_mut().item_spacing = egui::vec2(4.0, 6.0);
                for (index, (kind, title)) in KINDS.iter().enumerate() {
                    let count = self.catalog.as_ref().map(|c| c.items.iter().filter(|r| r.kind == *kind).count()).unwrap_or(0);
                    if theme::choice(ui, self.kind == index, format!("{title}  {count}")).clicked() {
                        self.kind = index;
                        self.adding = false;
                        self.edit_id = None;
                        self.definition.clear();
                    }
                }
            });
            ui.add_space(12.0);
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                if ui.add_enabled(self.catalog.is_some(), egui::Button::new(if KINDS[self.kind].0 == "packages" { "+ Install package" } else { "+ Add resource" })).clicked() {
                    self.adding = !self.adding;
                    self.edit_id = None;
                    self.definition = "{\n  \"url\": \"https://example.com/mcp\"\n}".into();
                }
                ui.add(theme::text_field(&mut self.query, "Search resources").desired_width(ui.available_width()));
            });
            if self.adding { self.form(ui, demo); }
            let query = self.query.trim().to_lowercase();
            let items: Vec<_> = self.catalog.as_ref().map(|c| c.items.iter().filter(|r| r.kind == KINDS[self.kind].0 && format!("{} {} {} {}", r.name, r.description, r.source, r.scope).to_lowercase().contains(&query)).cloned().collect()).unwrap_or_default();
            ui.add_space(12.0);
            if items.is_empty() && self.catalog.is_some() {
                card(ui, |ui| row(ui, if query.is_empty() { "No configured resources" } else { "No matching resources" }, "Add a local resource, install a Pi package, or refresh after editing configuration.", |_| {}));
            }
            if !items.is_empty() {
                ui.label(theme::caption(ui, format!("{} · {}", KINDS[self.kind].1.to_uppercase(), items.len())));
                card(ui, |ui| {
                    for (index, item) in items.into_iter().enumerate() {
                        if index > 0 { separator(ui); }
                        self.resource(ui, item, demo);
                    }
                });
            }
        });
        section(ui, "How changes apply", "");
        note(
            ui,
            "Configuration, not a sandbox",
            "Browse without loading extensions or connecting to MCP servers. Installs may run dependency code. Local source files are never deleted; Remove excludes a local resource. Package removal uninstalls its managed copy. MCP removal keeps OAuth credentials. New chats / restarted Pui processes load changes; existing agents are not reloaded. Use Pi /mcp for connection diagnostics and OAuth sign-in. Pi themes affect the CLI, not Pui's appearance.",
        );
    }
    fn confirmation(&mut self, ui: &mut egui::Ui, demo: bool) {
        let pending = self.pending.as_ref().unwrap();
        let mut confirm = false;
        let mut cancel = false;
        prompt_ui::inline(ui.ctx()).show(ui, |ui| {
            prompt_ui::style(ui);
            prompt_ui::badge(ui, Icon::Folder, pending.request["action"] == "remove");
            prompt_ui::heading(ui, &pending.title, &pending.description);
            ui.add_space(14.0);
            {
                let scope = pending.request["scope"].as_str().unwrap_or("user");
                ui.label(theme::small(
                    ui,
                    format!(
                        "{} configuration · {}",
                        if scope == "project" {
                            "Project"
                        } else {
                            "Personal"
                        },
                        self.cwd.as_ref().unwrap().display()
                    ),
                ));
                if let Some(source) = pending.request["source"].as_str() {
                    ui.add(
                        egui::Label::new(RichText::new(source).monospace())
                            .wrap()
                            .selectable(true),
                    );
                }
                if let Some(definition) = pending.request["definition"].as_str() {
                    egui::ScrollArea::vertical()
                        .id_salt("resource-definition-confirm")
                        .max_height(160.0)
                        .show(ui, |ui| {
                            ui.add(
                                egui::Label::new(RichText::new(definition).monospace())
                                    .wrap()
                                    .selectable(true),
                            );
                        });
                }
                ui.add_space(10.0);
                match confirmations::actions(
                    ui,
                    "Cancel",
                    match pending.request["action"].as_str().unwrap_or("") {
                        "install" => "Install package",
                        "remove" => "Remove resource",
                        "toggle" if pending.request["enabled"] == true => "Enable resource",
                        "toggle" => "Disable resource",
                        "update" => "Update resource",
                        _ => "Save resource",
                    },
                    pending.request["action"] == "remove",
                    !demo,
                    false,
                ) {
                    Some(confirmations::Choice::Cancel) => cancel = true,
                    Some(confirmations::Choice::Confirm) => confirm = true,
                    None => {}
                }
            }
        });
        if cancel {
            self.pending = None;
        } else if confirm {
            let request = self.pending.take().unwrap().request;
            self.start(ui, request);
        }
    }
    fn form(&mut self, ui: &mut egui::Ui, demo: bool) {
        let kind = KINDS[self.kind].0;
        section(
            ui,
            if self.edit_id.is_some() {
                "Edit MCP definition"
            } else {
                "Add resource"
            },
            "",
        );
        prompt_ui::inline(ui.ctx()).show(ui, |ui| {
            prompt_ui::style(ui);
            egui::Frame::new().show(ui, |ui| {
                ui.spacing_mut().item_spacing.y = 8.0;
                if self.scope.is_empty() { self.scope = "user".into(); }
                ui.horizontal(|ui| {
                    ui.label(theme::small(ui, "Save to"));
                    ui.add_enabled_ui(self.edit_id.is_none(), |ui| {
                        if theme::choice(ui, self.scope == "user", "Personal").clicked() { self.scope = "user".into(); }
                        ui.add_enabled_ui(self.catalog.as_ref().is_some_and(|c| c.trusted), |ui| {
                            if theme::choice(ui, self.scope == "project", "Project").clicked() { self.scope = "project".into(); }
                        });
                    });
                });
                if kind == "mcp" {
                    ui.add_enabled(self.edit_id.is_none(), theme::text_field(&mut self.name, "Server name").desired_width(ui.available_width()));
                    ui.label(theme::small(ui, if self.edit_id.is_some() { "JSON patch: supplied fields replace existing fields; omitted fields (including credentials) are preserved. Use environment references, not literal secrets. Open mcp.json to inspect or remove fields." } else { "Server definition JSON (command + args, or url). Prefer ${ENV_VAR} references for credentials." }));
                    ui.add(egui::TextEdit::multiline(&mut self.definition).code_editor().desired_rows(6).desired_width(f32::INFINITY));
                } else {
                    ui.label(theme::small(ui, if kind == "packages" { "npm:@scope/package@version, git:github.com/owner/repo@ref, or a local package path" } else { "Absolute or workspace-relative path. Source files stay in place; Pi registers their path." }));
                    ui.add(theme::text_field(&mut self.source, if kind == "packages" { "Package source" } else { "Local resource path" }).desired_width(ui.available_width()));
                }
                let valid = if kind == "mcp" { !self.name.trim().is_empty() && serde_json::from_str::<Value>(&self.definition).is_ok_and(|v| v.is_object()) } else { !self.source.trim().is_empty() };
                ui.horizontal(|ui| {
                    if ui.button("Cancel").clicked() { self.adding = false; self.edit_id = None; }
                    if ui.add_enabled(valid && !demo, egui::Button::new("Review change…")).clicked() {
                        let action = if kind == "mcp" { if self.edit_id.is_some() { "mcp-edit" } else { "mcp-add" } } else if kind == "packages" { "install" } else { "register" };
                        self.stage(json!({"action":action,"kind":kind,"scope":self.scope,"source":self.source,"name":self.name,"definition":self.definition,"id":self.edit_id}),
                            if self.edit_id.is_some() { format!("Update {}?", self.name) } else { format!("Add {}?", KINDS[self.kind].1.to_lowercase()) },
                            "Only continue if you trust this resource. Packages may execute installer/dependency code; extensions and stdio MCP servers execute with your OS permissions when loaded. Running agents will not be interrupted.".into());
                    }
                });
            });
        });
    }
    fn resource(&mut self, ui: &mut egui::Ui, item: Resource, demo: bool) {
        let colors = theme::palette(ui.ctx());
        ui.push_id(&item.id, |ui| {
            row(ui, &item.name, &format!("{} · {}", if item.scope == "project" { "Project" } else { "Personal" }, if item.kind == "packages" { &item.description } else if item.enabled { "Enabled in config" } else { "Disabled in config" }), |ui| {
                if item.kind != "packages" {
                    let mut enabled = item.enabled;
                    ui.add_enabled_ui(!demo, |ui| toggle(ui, &mut enabled, &item.name));
                    if enabled != item.enabled {
                        self.stage(json!({"action":"toggle","id":item.id,"scope":item.scope,"enabled":enabled}), format!("{} {}?", if enabled { "Enable" } else { "Disable" }, item.name), format!("Updates {}. This affects future Pi processes only, not running agents.", item.config));
                    }
                } else { ui.label(theme::small(ui, "Pi package")); }
                if icons::button(ui, if self.expanded.contains(&item.id) { Icon::Collapse } else { Icon::Details }, "Details & actions").clicked() && !self.expanded.remove(&item.id) {
                    self.expanded.insert(item.id.clone());
                }
            });
            if self.expanded.contains(&item.id) {
                egui::Frame::new().fill(colors.code_bg).inner_margin(18).show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    ui.spacing_mut().item_spacing.y = 10.0;
                    if !item.description.is_empty() { ui.label(theme::small(ui, &item.description)); }
                    for (label, value) in [("Source", &item.source), ("Path", &item.path), ("Configuration", &item.config)] {
                        if !value.is_empty() { ui.add(egui::Label::new(RichText::new(format!("{label}: {value}")).size(11.0).color(colors.muted)).wrap().selectable(true)); }
                    }
                    ui.horizontal_wrapped(|ui| {
                        if ui.add_enabled(!demo && Path::new(&item.config).is_file(), egui::Button::new("Open config")).clicked() { open_config(item.config.clone().into()); }
                        if ui.button("Copy path").clicked() { ui.ctx().copy_text(item.path.clone()); }
                        if item.kind == "mcp" && ui.add_enabled(!demo, egui::Button::new("Edit definition…")).clicked() {
                            self.adding = true;
                            self.edit_id = Some(item.id.clone());
                            self.name = item.name.clone();
                            self.scope = item.scope.clone();
                            self.definition = "{}".into();
                        }
                        if item.removable && ui.add_enabled(!demo, egui::Button::new(if item.kind == "packages" { "Uninstall…" } else { "Remove…" })).clicked() {
                            self.stage(json!({"action":"remove","id":item.id,"scope":item.scope}), format!("Remove {}?", item.name), format!("{} Configuration: {}. Running agents remain unchanged.", match item.kind.as_str() { "packages" => "Uninstalls the managed package copy and removes its declaration. Local package source files are kept.", "mcp" => "Removes this server entry; OAuth credentials are kept. Removing a project override may reveal the personal server again.", _ => "Disables this resource through Pi's exclusion filters. Source files are kept. For package resources, uninstall the package separately to remove its managed copy." }, item.config));
                        }
                    });
                });
            }
        });
    }
}
fn run_helper(cwd: &Path, request: &Value) -> Result<Value, String> {
    let sdk = crate::usage::sdk()
        .filter(|p| p.is_file())
        .ok_or("Pi SDK not found. Set PUI_PI_SDK to its dist/index.js.")?;
    let mut command = Command::new("node");
    command
        .args([
            "--input-type=module",
            "--eval",
            include_str!("../../../scripts/resources.mjs"),
        ])
        .arg(sdk)
        .env("PUI_RESOURCES_HELPER", "1")
        .current_dir(cwd)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null());
    if let Some(path) = rpc::child_path() {
        command.env("PATH", path);
    }
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
    let timeout = if request["action"] == "install" || request["action"] == "remove" {
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
fn open_config(file: PathBuf) {
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
fn demo_catalog() -> Catalog {
    let fixtures = [
        (
            "skills",
            "code-review",
            "Review changes for correctness, safety and maintainability.",
            true,
        ),
        (
            "skills",
            "browser-testing",
            "Run and inspect browser workflows.",
            true,
        ),
        (
            "skills",
            "release-notes",
            "Prepare release notes from recorded changes.",
            false,
        ),
        ("mcp", "documentation", "Streamable HTTP server", true),
        ("mcp", "filesystem", "Local stdio server", false),
        (
            "extensions",
            "workspace-tools",
            "Additional workspace commands",
            true,
        ),
        ("prompts", "review", "Review the current changes", true),
        ("themes", "midnight", "Pi CLI theme", true),
        (
            "packages",
            "npm:@example/pi-tools",
            "Installed package",
            true,
        ),
    ];
    Catalog {
        revision: "demo".into(),
        trusted: true,
        items: fixtures
            .into_iter()
            .enumerate()
            .map(|(i, (kind, name, description, enabled))| Resource {
                id: format!("demo-{i}"),
                kind: kind.into(),
                name: name.into(),
                description: description.into(),
                enabled,
                scope: if i == 1 { "project" } else { "user" }.into(),
                path: format!("~/.pi/agent/{kind}/{name}"),
                source: "Demo fixture".into(),
                config: "~/.pi/agent/settings.json".into(),
                removable: true,
            })
            .collect(),
        ..Default::default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn grouped_resource_details_toggle_without_changing_configuration() {
        fn labels(shape: &egui::Shape, out: &mut Vec<String>) {
            match shape {
                egui::Shape::Text(t) => out.push(t.galley.job.text.clone()),
                egui::Shape::Vec(shapes) => shapes.iter().for_each(|s| labels(s, out)),
                _ => {}
            }
        }
        let ctx = egui::Context::default();
        theme::install(&ctx);
        let mut resources = Resources {
            catalog: Some(demo_catalog()),
            ..Default::default()
        };
        let mut details = None;
        let render = |resources: &mut Resources, events| {
            ctx.run(
                egui::RawInput {
                    screen_rect: Some(egui::Rect::from_min_size(
                        egui::Pos2::ZERO,
                        Vec2::new(860.0, 800.0),
                    )),
                    events,
                    ..Default::default()
                },
                |ctx| {
                    egui::CentralPanel::default().show(ctx, |ui| {
                        let item = demo_catalog().items.remove(0);
                        card(ui, |ui| resources.resource(ui, item, true));
                    });
                },
            )
        };
        let mut output = render(&mut resources, vec![]);
        for _ in 0..2 {
            output = render(&mut resources, vec![]);
        }
        // Locate the first resource row from its painted title; controls are pinned to its right edge.
        fn title(shape: &egui::Shape) -> Option<egui::Rect> {
            match shape {
                egui::Shape::Text(t) if t.galley.job.text == "code-review" => {
                    Some(t.galley.rect.translate(t.pos.to_vec2()))
                }
                egui::Shape::Vec(shapes) => shapes.iter().find_map(title),
                _ => None,
            }
        }
        for shape in &output.shapes {
            if let Some(rect) = title(&shape.shape) {
                details = Some(egui::pos2(771.0, rect.center().y + 10.0));
            }
        }
        let pos = details.expect("First resource row");
        for pressed in [true, false] {
            render(
                &mut resources,
                vec![
                    egui::Event::PointerMoved(pos),
                    egui::Event::PointerButton {
                        pos,
                        button: egui::PointerButton::Primary,
                        pressed,
                        modifiers: egui::Modifiers::NONE,
                    },
                ],
            );
        }
        assert!(resources.expanded.contains("demo-0"));
        let output = render(&mut resources, vec![]);
        let mut text = Vec::new();
        for shape in &output.shapes {
            labels(&shape.shape, &mut text);
        }
        assert!(text.iter().any(|s| s == "Copy path"));
        assert!(
            text.iter()
                .any(|s| s.contains("Review changes for correctness"))
        );
        assert!(resources.pending.is_none());
        assert!(resources.job.is_none());
    }

    #[test]
    fn confirmation_is_staged_not_executed() {
        let mut resources = Resources {
            catalog: Some(demo_catalog()),
            ..Default::default()
        };
        resources.stage(
            json!({"action":"remove","id":"demo-0","scope":"user"}),
            "Remove?".into(),
            "Review".into(),
        );
        assert_eq!(
            resources.pending.as_ref().unwrap().request["revision"],
            "demo"
        );
        assert!(resources.job.is_none());
    }
    #[test]
    fn stale_workspace_results_are_discarded() {
        let (tx, rx) = mpsc::channel();
        let mut resources = Resources {
            cwd: Some("/new".into()),
            job: Some(Job {
                cwd: "/old".into(),
                mutation: true,
                rx,
            }),
            ..Default::default()
        };
        tx.send(Ok(json!({"changed":true}))).unwrap();
        resources.poll();
        assert!(resources.notice.is_none());
        assert!(resources.catalog.is_none());
        assert!(resources.job.is_none());
    }
}
