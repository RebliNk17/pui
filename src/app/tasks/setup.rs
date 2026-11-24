//! Exact-command, host-controlled setup. No model prompt and no shell bypass.
//! The caller owns review/confirmation, writer authority and durable save acknowledgements.
use super::*;
use domain::setup::{SetupBinding, SetupCommand, SetupRecord, SetupStatus};
use sha2::{Digest, Sha256};
use std::{fs, io::Read, path::Path, time::Instant};

#[cfg(test)]
#[path = "setup_tests.rs"]
mod tests;

pub(super) const RISKS: &str = "Setup is separate from workflow approval. Review every exact command. Commands run on the host, without an OS sandbox, in this retained isolated workspace. Package installation can access the network, write shared package caches, remove or replace dependencies, and execute arbitrary lifecycle scripts unless explicitly disabled. No credential/environment copying or dependency symlinks are inferred or authorized. Inherited host environment and package-manager configuration can affect commands. No builder, checks, verifier, provider prompt, automatic retry or replay is started by setup. A successful exit is not proof of environment health.";

#[derive(Clone, Debug)]
pub(super) struct Plan {
    pub binding: SetupBinding,
    pub commands: Vec<String>,
    pub explanation: String,
}
impl Plan {
    pub fn with_commands(mut self, commands: Vec<String>) -> Result<Self, String> {
        validate_commands(&commands)?;
        self.commands = commands;
        Ok(self)
    }
    /// Filesystem work: call off the UI thread after editing, before confirmation/start.
    pub fn validate_current(&self, task: &Task) -> Result<(), String> {
        validate_commands(&self.commands)?;
        if self.binding != binding(task)? {
            return Err("Workflow, source, candidate or manifests changed after setup review. Inspect a fresh plan; nothing was run.".into());
        }
        Ok(())
    }
}
pub(super) fn validate_commands(commands: &[String]) -> Result<(), String> {
    if commands.is_empty()
        || commands.len() > 8
        || commands.iter().any(|command| {
            command.trim().is_empty() || command.len() > 16000 || command.contains('\0')
        })
        || commands.iter().map(String::len).sum::<usize>() > 48000
    {
        return Err(
            "Review 1–8 explicit, nonempty setup commands (16,000 bytes each; 48,000 bytes total)."
                .into(),
        );
    }
    Ok(())
}
fn eligible(task: &Task) -> Result<(), String> {
    if !task.isolated || task.workspace.is_none() {
        return Err("Setup currently supports retained isolated workspaces only; in-place/source setup is not authorized.".into());
    }
    if !matches!(task.phase, Phase::Blocked | Phase::Interrupted) {
        return Err("Stop the workflow and inspect its blocked/interrupted evidence before reviewing setup.".into());
    }
    if task
        .setup
        .iter()
        .any(|record| record.status == SetupStatus::Running)
    {
        return Err(
            "A setup command is still recorded as running; inspect its uncertain outcome first."
                .into(),
        );
    }
    Ok(())
}
fn task_digest(task: &Task) -> Result<String, String> {
    let bytes = serde_json::to_vec(task).map_err(|_| "Cannot bind workflow setup evidence.")?;
    Ok(crate::hex::lower(Sha256::digest(bytes)))
}
fn binding(task: &Task) -> Result<SetupBinding, String> {
    eligible(task)?;
    let source = fs::canonicalize(&task.source).map_err(|_| "Setup source is unavailable.")?;
    let workspace = fs::canonicalize(task.workspace.as_ref().unwrap())
        .map_err(|_| "Retained setup workspace is unavailable.")?;
    if source == workspace {
        return Err("Setup cannot target the original source workspace.".into());
    }
    Ok(SetupBinding {
        task: task.id.clone(),
        source_fingerprint: domain::workspace::fingerprint(&source)?,
        workspace_fingerprint: domain::workspace::fingerprint(&workspace)?,
        manifests_digest: manifests_digest(&workspace)?,
        task_digest: task_digest(task)?,
        candidate_fingerprint: task
            .latest()
            .and_then(|attempt| attempt.fingerprint.clone()),
        source,
        workspace,
    })
}
/// Bounded regular-file inspection, never credential/configuration inspection.
fn bounded(root: &Path, name: &str, max: u64) -> Result<Option<Vec<u8>>, String> {
    let path = root.join(name);
    let metadata = match fs::symlink_metadata(&path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(_) => {
            return Err(format!(
                "Cannot inspect {name}; setup prerequisites remain unknown."
            ));
        }
    };
    if !metadata.is_file() || metadata.len() > max {
        return Err(format!(
            "{name} must be a bounded regular file; symlinks are not followed."
        ));
    }
    let mut options = fs::OpenOptions::new();
    options.read(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK);
    }
    let file = options
        .open(path)
        .map_err(|_| format!("Cannot safely read {name}."))?;
    if !file.metadata().is_ok_and(|metadata| metadata.is_file()) {
        return Err(format!("{name} is not a regular file."));
    }
    let mut bytes = Vec::new();
    file.take(max + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| format!("Cannot read {name}."))?;
    if bytes.len() as u64 > max {
        return Err(format!("{name} exceeds the setup inspection limit."));
    }
    Ok(Some(bytes))
}
fn manifests_digest(root: &Path) -> Result<String, String> {
    let mut hash = Sha256::new();
    for (name, max) in [
        ("package.json", 256 * 1024),
        ("package-lock.json", 2 * 1024 * 1024),
        ("npm-shrinkwrap.json", 2 * 1024 * 1024),
    ] {
        hash.update(name.as_bytes());
        match bounded(root, name, max)? {
            Some(bytes) => {
                hash.update([1]);
                hash.update((bytes.len() as u64).to_le_bytes());
                hash.update(bytes);
            }
            None => hash.update([0]),
        }
    }
    Ok(crate::hex::lower(hash.finalize()))
}
fn npm_suggestion(root: &Path) -> Result<Option<String>, String> {
    if bounded(root, "npm-shrinkwrap.json", 2 * 1024 * 1024)?.is_some() {
        return Ok(None); // This overrides package-lock; do not guess another install plan.
    }
    let (Some(manifest), Some(lock)) = (
        bounded(root, "package.json", 256 * 1024)?,
        bounded(root, "package-lock.json", 2 * 1024 * 1024)?,
    ) else {
        return Ok(None);
    };
    let manifest: Value = serde_json::from_slice(&manifest)
        .map_err(|_| "package.json is invalid; setup requirements are unknown.")?;
    let lock: Value = serde_json::from_slice(&lock)
        .map_err(|_| "package-lock.json is invalid; setup requirements are unknown.")?;
    let Some(package) = lock["packages"][""].as_object() else {
        return Ok(None);
    };
    if !manifest.is_object()
        || !matches!(lock["lockfileVersion"].as_u64(), Some(2 | 3))
        || manifest.get("workspaces").is_some()
        || manifest
            .get("packageManager")
            .is_some_and(|value| !value.as_str().is_some_and(|name| name.starts_with("npm@")))
    {
        return Ok(None);
    }
    for key in ["dependencies", "devDependencies", "optionalDependencies"] {
        let expected = manifest.get(key);
        let frozen = package.get(key);
        if expected.is_some_and(|value| !value.is_object()) || expected != frozen {
            return Ok(None);
        }
    }
    Ok(Some("npm ci --ignore-scripts".into()))
}
/// Read-only off-thread plan generation. Merely viewing this never starts Pi.
pub(super) fn plan(task: &Task) -> Result<Plan, String> {
    let binding = binding(task)?;
    if let Some(receipt) = &task.profile {
        let current = domain::profile::load(&binding.workspace)?
            .ok_or("Reviewed execution profile disappeared.")?;
        if current.digest != receipt.digest {
            return Err("Execution profile changed; setup was not authorized.".into());
        }
        if !receipt.profile.setup.is_empty() {
            return Ok(Plan {
                binding,
                commands: receipt.profile.setup.clone(),
                explanation: format!(
                    "Repository profile {} · SHA-256 {}. These exact setup commands are suggestions, not authority. Review network, lifecycle scripts and shared-cache effects before separate confirmation.",
                    receipt.profile.name, receipt.digest
                ),
            });
        }
    }
    let (commands, explanation) = match npm_suggestion(&binding.workspace) {
        Ok(Some(command)) => (vec![command], "Supported npm package-lock v2/v3 root declarations match package.json. Proposed frozen install: npm ci --ignore-scripts. This can use the network and shared npm cache and replaces node_modules. Lifecycle scripts are disabled; packages needing generated/native artifacts may remain unusable. Review/edit to npm ci only if you explicitly authorize lifecycle scripts. Lockfile consistency and dependency usability are not established until execution/checks.".to_owned()),
        Ok(None) => (vec![], "No supported unambiguous npm frozen-lockfile plan was found. Requirements remain unknown. Supply explicit project-specific commands yourself, or cancel; no install command is inferred.".to_owned()),
        Err(error) => (vec![], format!("{error} Supply explicit commands only after independent inspection, or cancel.")),
    };
    Ok(Plan {
        binding,
        commands,
        explanation,
    })
}

pub(super) struct Process {
    pub state: State,
    pub record: SetupRecord,
    client: rpc::Client,
    initialized: bool,
    initialization: String,
    pending: Option<String>,
    inspecting: Option<Receiver<Result<bool, String>>>,
    persisted: Option<SetupRecord>,
    ctx: egui::Context,
    submitted: bool,
    checking_submission: bool,
    verified_submission: bool,
    changed: bool,
    deadline: Instant,
}
impl Process {
    /// Caller must have an owning writer claim, fresh async validation, and separate
    /// exact-plan user confirmation. No command runs until durable `persisted` ack.
    pub fn start(
        task: &Task,
        plan: Plan,
        ctx: egui::Context,
        mut settings: Value,
    ) -> Result<Self, String> {
        eligible(task)?;
        validate_commands(&plan.commands)?;
        if plan.binding.task != task.id || plan.binding.task_digest != task_digest(task)? {
            return Err("Setup approval no longer matches this exact workflow record.".into());
        }
        if !settings.is_object() {
            settings = json!({});
        }
        settings["permission"] = "manual".into(); // No classifier, remembered bypass or provider call.
        let launch = rpc::Launch {
            args: vec![
                "--no-session",
                "--no-tools",
                "--no-extensions",
                "--no-mcp",
                "--no-context-files",
                "--no-skills",
                "--no-prompt-templates",
            ]
            .into_iter()
            .map(str::to_owned)
            .collect(),
            readonly: false,
            worker: true,
            task_role: Some("checks".into()),
            settings,
        };
        #[cfg(not(test))]
        let client = rpc::Client::start_with(&plan.binding.workspace, None, ctx.clone(), launch);
        #[cfg(test)]
        let client = {
            let _ = launch;
            rpc::Client::stub()
        };
        Self::with_client(plan, ctx, client)
    }
    fn with_client(
        plan: Plan,
        ctx: egui::Context,
        mut client: rpc::Client,
    ) -> Result<Self, String> {
        let initialization = client.enqueue(protocol::command("get_state"))?;
        let mut result = Self {
            record: SetupRecord {
                id: uuid::Uuid::new_v4().to_string(),
                binding: plan.binding,
                commands: plan.commands,
                approved: domain::now(),
                finished: None,
                results: vec![],
                status: SetupStatus::Running,
                source_changed: false,
                note: "Awaiting durable command evidence and normal exact-command shell approval."
                    .into(),
            },
            state: State::default(),
            client,
            initialized: false,
            initialization,
            pending: None,
            inspecting: None,
            persisted: None,
            ctx,
            submitted: false,
            checking_submission: false,
            verified_submission: false,
            changed: true,
            deadline: Instant::now(),
        };
        result.stage_next();
        result
            .ctx
            .request_repaint_after(Duration::from_secs(30 * 60));
        Ok(result)
    }
    pub fn finished(&self) -> bool {
        self.record.status != SetupStatus::Running
    }
    /// Reaping a Pi supervisor must never stall the rendering thread.
    pub fn retire(self) {
        std::thread::spawn(move || drop(self));
    }
    pub fn needs_persist(&self) -> bool {
        self.persisted.as_ref() != Some(&self.record)
    }
    /// Acknowledgement must refer to exactly this record after a durable store write.
    pub fn persisted(&mut self, record: &SetupRecord) -> Result<(), String> {
        if record != &self.record {
            return Err("Stale setup persistence acknowledgement; no command submitted.".into());
        }
        self.persisted = Some(record.clone());
        self.submit();
        Ok(())
    }
    fn stage_next(&mut self) {
        let command = self.record.commands[self.record.results.len()].clone();
        self.record.results.push(SetupCommand {
            command,
            started: domain::now(),
            finished: None,
            exit_code: None,
            cancelled: false,
            output: String::new(),
            truncated: false,
            error: None,
        });
        self.submitted = false;
        self.verified_submission = false;
        self.changed = true;
    }
    fn inspect_fingerprints(&mut self) {
        let binding = self.record.binding.clone();
        let (tx, rx) = mpsc::channel();
        self.inspecting = Some(rx);
        let ctx = self.ctx.clone();
        std::thread::spawn(move || {
            let result = (|| {
                Ok(
                    fs::canonicalize(&binding.source).map_err(|error| error.to_string())?
                        != binding.source
                        || fs::canonicalize(&binding.workspace)
                            .map_err(|error| error.to_string())?
                            != binding.workspace
                        || domain::workspace::fingerprint(&binding.source)?
                            != binding.source_fingerprint
                        || domain::workspace::fingerprint(&binding.workspace)?
                            != binding.workspace_fingerprint
                        || manifests_digest(&binding.workspace)? != binding.manifests_digest,
                )
            })();
            let _ = tx.send(result);
            ctx.request_repaint();
        });
    }
    fn submit(&mut self) {
        if self.finished()
            || !self.initialized
            || self.pending.is_some()
            || self.inspecting.is_some()
            || self.submitted
            || self.needs_persist()
        {
            return;
        }
        if !self.verified_submission {
            self.checking_submission = true;
            self.inspect_fingerprints();
            return;
        }
        self.submitted = true;
        self.deadline = Instant::now();
        self.ctx.request_repaint_after(Duration::from_secs(30 * 60));
        let command = &self.record.results.last().unwrap().command;
        match self
            .client
            .enqueue(json!({"type":"bash","command":command,"excludeFromContext":true}))
        {
            Ok(id) => self.pending = Some(id),
            Err(error) => self.stop(SetupStatus::Unknown, &error),
        }
    }
    fn stop(&mut self, status: SetupStatus, note: &str) {
        if self.finished() {
            return;
        }
        if let Some(result) = self
            .record
            .results
            .last_mut()
            .filter(|result| result.finished.is_none())
        {
            result.finished = Some(domain::now());
            result.error = Some(note.into());
            result.cancelled = status == SetupStatus::Cancelled;
        }
        self.record.status = status;
        self.record.finished = Some(domain::now());
        self.record.note = note.into();
        self.pending = None;
        self.inspecting = None;
        self.state.dialogs.clear();
        self.changed = true;
    }
    pub fn cancel(&mut self) {
        if self.finished() {
            return;
        }
        let _ = self.client.send(protocol::command("abort_bash"));
        self.stop(SetupStatus::Cancelled, "Setup cancelled. Exit status/effects may be incomplete; nothing was retried or resumed.");
    }
    pub fn answer_dialog(&mut self, id: &str, value: Option<Value>) -> Result<(), String> {
        if self.finished() || !self.state.dialogs.iter().any(|dialog| dialog["id"] == id) {
            return Err("This setup permission question is no longer pending.".into());
        }
        let confirm = self
            .state
            .dialogs
            .iter()
            .any(|dialog| dialog["id"] == id && dialog["method"] == "confirm");
        let response = match value {
            Some(value) if confirm => {
                json!({"type":"extension_ui_response","id":id,"confirmed":value.as_bool().unwrap_or(false)})
            }
            Some(value) => json!({"type":"extension_ui_response","id":id,"value":value}),
            None => {
                if let Some(result) = self.record.results.last_mut() {
                    result.cancelled = true;
                    self.changed = true;
                }
                json!({"type":"extension_ui_response","id":id,"cancelled":true})
            }
        };
        self.client.send(response)?;
        self.state.dialogs.retain(|dialog| dialog["id"] != id);
        Ok(())
    }
    pub fn poll(&mut self) -> bool {
        if self.finished() {
            return std::mem::take(&mut self.changed);
        }
        let events: Vec<_> = self.client.events.try_iter().collect();
        for event in events {
            self.event(event);
        }
        let inspection = self.inspecting.as_ref().and_then(|rx| match rx.try_recv() {
            Ok(result) => Some(result),
            Err(mpsc::TryRecvError::Disconnected) => Some(Err(
                "Post-setup fingerprint inspection was interrupted.".into(),
            )),
            Err(mpsc::TryRecvError::Empty) => None,
        });
        if let Some(result) = inspection {
            self.inspecting = None;
            match result {
                Err(error) => self.stop(
                    SetupStatus::Unknown,
                    &format!("{error} No subsequent command or recovery was started."),
                ),
                Ok(true) => {
                    self.record.source_changed = true;
                    self.stop(SetupStatus::Failed, "Setup changed tracked/nonignored source or manifests. Candidate evidence is stale; no subsequent command, builder or check was started.");
                }
                Ok(false) if self.checking_submission => {
                    self.checking_submission = false;
                    self.verified_submission = true;
                    self.submit();
                }
                Ok(false) => {
                    let result = self.record.results.last().unwrap();
                    if result.cancelled {
                        self.stop(
                            SetupStatus::Cancelled,
                            "Setup command cancelled; no retry or continuation.",
                        );
                    } else if result.exit_code.is_none() {
                        self.stop(SetupStatus::Unknown, "Setup command returned no exit status; effects unknown, no retry or continuation.");
                    } else if result.exit_code != Some(0) {
                        self.stop(SetupStatus::Failed, "Setup command failed. Its exact output is retained; environment health remains unknown. No retry or continuation.");
                    } else if self.record.results.len() == self.record.commands.len() {
                        self.stop(SetupStatus::Succeeded, "All explicitly approved setup commands exited successfully without changing captured source. Refresh read-only prerequisite evidence; environment health is not established. No workflow stage was resumed.");
                    } else {
                        self.stage_next();
                    }
                }
            }
        }
        if !self.finished() && self.deadline.elapsed() > Duration::from_secs(30 * 60) {
            let _ = self.client.send(protocol::command("abort_bash"));
            self.stop(
                SetupStatus::Unknown,
                "Setup exceeded 30 minutes; effects are uncertain. No action was retried.",
            );
        }
        std::mem::take(&mut self.changed)
    }
    fn event(&mut self, event: rpc::Event) {
        if self.finished() {
            return;
        }
        let record = match event {
            rpc::Event::Record(record) => record,
            rpc::Event::History(_) => return,
            rpc::Event::Error(error) => {
                self.stop(SetupStatus::Unknown, &error);
                return;
            }
            rpc::Event::Exited => {
                self.stop(
                    SetupStatus::Unknown,
                    "Setup Pi process exited. Command effects may be incomplete; no replay.",
                );
                return;
            }
        };
        if record["type"] == "response" && record["id"] == self.initialization {
            if record["success"] != true || record["command"] != "get_state" {
                self.stop(
                    SetupStatus::Unknown,
                    "Setup Pi initialization failed; no setup command was submitted.",
                );
            } else {
                self.initialized = true;
                self.submit();
            }
            return;
        }
        if record["type"] == "response"
            && self.pending.as_ref().is_some_and(|id| record["id"] == *id)
        {
            if record["command"] != "bash" || record["success"] != true {
                self.stop(SetupStatus::Unknown, "Setup shell request was rejected or unrecognized; effects are unknown. No retry.");
                return;
            }
            self.pending = None;
            let data = &record["data"];
            let result = self.record.results.last_mut().unwrap();
            let raw = protocol::string(data, "output");
            result.output = raw.chars().take(24000).collect();
            result.truncated = data["truncated"] == true || raw.len() > result.output.len();
            result.exit_code = data["exitCode"].as_i64();
            result.cancelled |= data["cancelled"] == true;
            result.finished = Some(domain::now());
            self.changed = true;
            self.inspect_fingerprints();
            return;
        }
        // Native shared prompt renderer owns permission presentation and cancellation.
        // Unexpected provider activity is never accepted as setup evidence.
        if matches!(
            protocol::string(&record, "type"),
            "agent_start" | "message_start"
        ) {
            let _ = self.client.send(protocol::command("abort"));
            self.stop(SetupStatus::Unknown, "Unexpected agent activity in host-only setup; stopped without a setup continuation.");
            return;
        }
        self.state.apply(record);
    }
}
