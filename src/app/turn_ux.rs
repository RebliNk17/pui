//! Per-turn UX surfaces above the composer: live plan strip, wrap-up next-step
//! box, and inline session proposal cards. Presentation follows Quiet Mac
//! primitives in `prompt_ui`.
use super::*;
use std::collections::HashSet;

const MAX_PLAN_STEPS: usize = 16;
const MAX_WRAP_UP_OPTIONS: usize = 4;
const MAX_PROPOSALS: usize = 4;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum StepStatus {
    Pending,
    InProgress,
    Completed,
    Blocked,
}

impl StepStatus {
    fn parse(value: &str) -> Option<Self> {
        match value {
            "pending" => Some(Self::Pending),
            "in_progress" => Some(Self::InProgress),
            "completed" => Some(Self::Completed),
            "blocked" => Some(Self::Blocked),
            _ => None,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct PlanStep {
    pub(super) id: String,
    pub(super) label: String,
    pub(super) status: StepStatus,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct TurnPlan {
    pub(super) title: Option<String>,
    pub(super) steps: Vec<PlanStep>,
    pub(super) expanded: bool,
}

impl TurnPlan {
    pub(super) fn counts(&self) -> (usize, usize) {
        let done = self
            .steps
            .iter()
            .filter(|s| s.status == StepStatus::Completed)
            .count();
        (done, self.steps.len())
    }

    pub(super) fn all_completed(&self) -> bool {
        !self.steps.is_empty() && self.steps.iter().all(|s| s.status == StepStatus::Completed)
    }

    pub(super) fn active_step(&self) -> Option<&PlanStep> {
        self.steps
            .iter()
            .find(|s| s.status == StepStatus::InProgress)
            .or_else(|| self.steps.iter().find(|s| s.status == StepStatus::Blocked))
            .or_else(|| self.steps.iter().find(|s| s.status == StepStatus::Pending))
            .or_else(|| self.steps.last())
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct WrapUpOption {
    pub(super) label: String,
    pub(super) prompt: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct WrapUpBox {
    pub(super) recap: String,
    pub(super) question: Option<String>,
    pub(super) options: Vec<WrapUpOption>,
    pub(super) dismissed: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) enum ProposalState {
    Pending,
    Started(String),
    KeptHere,
    Dismissed,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct SessionProposal {
    pub(super) id: String,
    pub(super) title: String,
    pub(super) brief: String,
    pub(super) access: String,
    pub(super) cwd: Option<String>,
    pub(super) state: ProposalState,
}

#[derive(Default, Clone, Debug)]
pub(super) struct TurnUx {
    pub(super) plan: Option<TurnPlan>,
    pub(super) wrap_up: Option<WrapUpBox>,
    pub(super) proposals: Vec<SessionProposal>,
    next_proposal_id: u64,
}

impl TurnUx {
    pub(super) fn on_user_submit(&mut self) {
        self.wrap_up = None;
        if self.plan.as_ref().is_some_and(TurnPlan::all_completed) {
            self.plan = None;
        }
        self.proposals
            .retain(|p| p.state != ProposalState::Dismissed);
    }

    pub(super) fn has_visible_content(&self) -> bool {
        self.plan.is_some()
            || self.wrap_up.as_ref().is_some_and(|w| !w.dismissed)
            || self
                .proposals
                .iter()
                .any(|p| p.state != ProposalState::Dismissed)
    }
}

fn trim_bounded(
    value: &Value,
    field: &str,
    max_chars: usize,
    allow_newlines: bool,
) -> Result<String, String> {
    let raw = value
        .as_str()
        .ok_or_else(|| format!("{field} must be a non-empty string"))?;
    let bad_ctrl = if allow_newlines {
        raw.contains('\0')
    } else {
        raw.chars().any(|c| c.is_control())
    };
    if bad_ctrl {
        return Err(format!("{field} contains unsupported control characters"));
    }
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return Err(format!("{field} must be a non-empty string"));
    }
    if trimmed.chars().count() > max_chars {
        return Err(format!("{field} exceeds the {max_chars}-character limit"));
    }
    Ok(trimmed.to_owned())
}

fn optional_bounded(
    value: Option<&Value>,
    field: &str,
    max_chars: usize,
    allow_newlines: bool,
) -> Result<Option<String>, String> {
    match value {
        None | Some(Value::Null) => Ok(None),
        Some(v) => Ok(Some(trim_bounded(v, field, max_chars, allow_newlines)?)),
    }
}

fn preview_brief(brief: &str, max_chars: usize) -> String {
    let collapsed: String = brief.split_whitespace().collect::<Vec<_>>().join(" ");
    let mut chars = collapsed.chars();
    let head: String = chars.by_ref().take(max_chars).collect();
    if chars.next().is_some() {
        format!("{head}…")
    } else {
        head
    }
}

enum TurnUxAction {
    None,
    TogglePlan,
    DismissWrapUp,
    WrapUpReply(usize),
    StartProposal(String),
    OpenStartedAgent(String),
    KeepProposalHere(String),
    DismissProposal(String),
}

impl Pui {
    fn turn_ux_for_mut(&mut self, source: &str) -> &mut TurnUx {
        if !source.is_empty()
            && source != self.agents.active
            && self.agents.parked.contains_key(source)
        {
            return &mut self.agents.parked.get_mut(source).unwrap().turn_ux;
        }
        &mut self.turn_ux
    }

    pub(super) fn turn_ux_control(
        &mut self,
        source: &str,
        request: &Value,
    ) -> Result<Value, String> {
        match protocol::string(request, "action") {
            "turn_update_plan" => {
                let title = optional_bounded(request.get("title"), "Plan title", 120, false)?;
                let raw_steps = request
                    .get("steps")
                    .and_then(Value::as_array)
                    .ok_or("Plan steps must be a list of 1–16 items")?;
                if raw_steps.is_empty() || raw_steps.len() > MAX_PLAN_STEPS {
                    return Err("Plan steps must contain between 1 and 16 items".into());
                }
                let mut seen = HashSet::with_capacity(raw_steps.len());
                let mut steps = Vec::with_capacity(raw_steps.len());
                for (index, item) in raw_steps.iter().enumerate() {
                    let id = trim_bounded(
                        item.get("id").unwrap_or(&Value::Null),
                        &format!("Step {} id", index + 1),
                        40,
                        false,
                    )?;
                    if !seen.insert(id.clone()) {
                        return Err(format!("Duplicate plan step id \"{id}\""));
                    }
                    let label = trim_bounded(
                        item.get("label").unwrap_or(&Value::Null),
                        &format!("Step {} label", index + 1),
                        240,
                        false,
                    )?;
                    let status = item
                        .get("status")
                        .and_then(Value::as_str)
                        .and_then(StepStatus::parse)
                        .ok_or_else(|| format!("Invalid status for step \"{id}\""))?;
                    steps.push(PlanStep { id, label, status });
                }
                let turn_ux = self.turn_ux_for_mut(source);
                let expanded = turn_ux.plan.as_ref().is_some_and(|p| p.expanded);
                let plan = TurnPlan {
                    title,
                    steps,
                    expanded,
                };
                let (completed, total) = plan.counts();
                turn_ux.plan = Some(plan);
                Ok(json!({
                    "ok": true,
                    "completed": completed,
                    "total": total,
                }))
            }
            "turn_wrap_up" => {
                let recap = trim_bounded(
                    request.get("recap").unwrap_or(&Value::Null),
                    "Wrap-up recap",
                    1200,
                    true,
                )?;
                let question =
                    optional_bounded(request.get("question"), "Wrap-up question", 400, true)?;
                let mut options = Vec::new();
                if let Some(raw_options) = request.get("options")
                    && !raw_options.is_null()
                {
                    let items = raw_options
                        .as_array()
                        .ok_or("Wrap-up options must be a list of at most 4 items")?;
                    if items.len() > MAX_WRAP_UP_OPTIONS {
                        return Err("Wrap-up options must contain at most 4 items".into());
                    }
                    for (index, item) in items.iter().enumerate() {
                        let label = trim_bounded(
                            item.get("label").unwrap_or(&Value::Null),
                            &format!("Wrap-up option {} label", index + 1),
                            40,
                            false,
                        )?;
                        let prompt = optional_bounded(
                            item.get("prompt"),
                            &format!("Wrap-up option {} prompt", index + 1),
                            2000,
                            true,
                        )?;
                        options.push(WrapUpOption { label, prompt });
                    }
                }
                let count = options.len();
                let turn_ux = self.turn_ux_for_mut(source);
                turn_ux.wrap_up = Some(WrapUpBox {
                    recap,
                    question,
                    options,
                    dismissed: false,
                });
                Ok(json!({
                    "ok": true,
                    "options": count,
                }))
            }
            "turn_propose_session" => {
                if self
                    .agents
                    .nodes
                    .get(source)
                    .is_some_and(|n| n.access == "read-only")
                {
                    return Err("Read-only workers cannot propose sessions".into());
                }
                let title = trim_bounded(
                    request.get("title").unwrap_or(&Value::Null),
                    "Proposal title",
                    100,
                    false,
                )?;
                let brief = trim_bounded(
                    request.get("brief").unwrap_or(&Value::Null),
                    "Proposal brief",
                    8000,
                    true,
                )?;
                let access = match request.get("access") {
                    None | Some(Value::Null) => "worktree".to_owned(),
                    Some(Value::String(s)) if matches!(s.as_str(), "read-only" | "worktree") => {
                        s.clone()
                    }
                    _ => return Err("Proposal access must be \"read-only\" or \"worktree\"".into()),
                };
                let cwd = optional_bounded(request.get("cwd"), "Proposal cwd", 4096, false)?;
                let turn_ux = self.turn_ux_for_mut(source);
                if turn_ux.proposals.len() >= MAX_PROPOSALS {
                    if let Some(pos) = turn_ux
                        .proposals
                        .iter()
                        .position(|p| p.state == ProposalState::Dismissed)
                    {
                        turn_ux.proposals.remove(pos);
                    } else {
                        return Err("At most 4 session proposals can be shown at once".into());
                    }
                }
                turn_ux.next_proposal_id += 1;
                let id = format!("proposal-{}", turn_ux.next_proposal_id);
                turn_ux.proposals.push(SessionProposal {
                    id: id.clone(),
                    title,
                    brief,
                    access,
                    cwd,
                    state: ProposalState::Pending,
                });
                Ok(json!({
                    "ok": true,
                    "proposal_id": id,
                    "state": "pending",
                    "note": "Displayed proposal card. Nothing executes until the user clicks Start new session."
                }))
            }
            _ => Err("Unknown turn UX action".into()),
        }
    }

    pub(super) fn send_turn_ux_prompt(&mut self, ctx: &egui::Context, text: String) -> bool {
        let text = text.trim().to_owned();
        if text.is_empty()
            || self.deletion.confirming()
            || self.resource_reload.confirming()
            || self.resource_reload.pending()
            || self.demo
            || self.loading_chat
            || self.history_pending.is_some()
            || self.restoring_code()
        {
            return false;
        }
        let mode = self.prefs.settings.delivery;
        let message = run_status::WaitingMessage {
            text: text.clone(),
            images: Vec::new(),
            mode,
        };
        if self.is_mirror() || (!self.state.ready() && self.connected) {
            if !self.remote_action(sync::Action::Queue(message.clone())) {
                if let Some(path) = self.state.session_file.as_ref().map(PathBuf::from) {
                    self.waiting.entry(path).or_default().push(message);
                } else {
                    return false;
                }
            }
            self.turn_ux.on_user_submit();
            self.focus_composer = true;
            return true;
        }
        if self.agents.idle.dormant.contains(&self.agents.active) {
            let id = self.agents.active.clone();
            if let Some(path) = self.state.session_file.as_ref().map(PathBuf::from) {
                self.waiting.entry(path).or_default().push(message);
            }
            if let Err(error) = self.wake_idle_agent(&id) {
                self.state.error = Some(error);
                return false;
            }
            self.turn_ux.on_user_submit();
            self.focus_composer = true;
            return true;
        }
        if !self.connected || self.client.is_none() {
            if self.draft.trim().is_empty() {
                self.draft = text;
            }
            if self.client.is_none() && !self.resume_agent() {
                self.connect(ctx, self.state.session_file.as_ref().map(PathBuf::from));
            }
            self.state.notice = Some(
                "Connecting to Pi. Your message is in the composer; send when connected.".into(),
            );
            return false;
        }
        self.prepare_initial_title(&message);
        if !self.deliver_message(message, "prompt") {
            return false;
        }
        self.state.restart_notice.clear();
        self.turn_ux.on_user_submit();
        self.focus_composer = true;
        true
    }

    pub(super) fn demo_turn_ux(&mut self) {
        self.prefs.hide_inspector = true;
        self.ultracode_ready = true;
        self.turn_ux.plan = Some(TurnPlan {
            title: Some("Implement local sandbox & Turn UX".into()),
            steps: vec![
                PlanStep {
                    id: "s1".into(),
                    label: "Generate macOS Seatbelt profile with workspace write allowlist".into(),
                    status: StepStatus::Completed,
                },
                PlanStep {
                    id: "s2".into(),
                    label:
                        "Wire Local sandbox & Strict sandbox toggles into Settings → Permissions"
                            .into(),
                    status: StepStatus::Completed,
                },
                PlanStep {
                    id: "s3".into(),
                    label: "Run kernel Seatbelt & UI regression test suites".into(),
                    status: StepStatus::InProgress,
                },
                PlanStep {
                    id: "s4".into(),
                    label: "Review diffs and push master to GitHub and GitLab".into(),
                    status: StepStatus::Pending,
                },
            ],
            expanded: true,
        });
        self.turn_ux.wrap_up = Some(WrapUpBox {
            recap: "Added OS-level macOS Seatbelt sandboxing (Standard & Strict modes) for both foreground bash and background shell commands, plus kernel enforcement tests. Local commits are ready on master.".into(),
            question: Some("How would you like to proceed?".into()),
            options: vec![
                WrapUpOption {
                    label: "Push to remotes".into(),
                    prompt: Some("Push master to GitHub and GitLab.".into()),
                },
                WrapUpOption {
                    label: "Rebuild Pui.app".into(),
                    prompt: Some("Run scripts/bundle-macos.sh to rebuild dist/Pui.app.".into()),
                },
                WrapUpOption {
                    label: "Run full suite".into(),
                    prompt: Some("Run scripts/test-rust.sh.".into()),
                },
            ],
            dismissed: false,
        });
        self.turn_ux.proposals = vec![SessionProposal {
            id: "proposal-1".into(),
            title: "Add PR Auto-Fix background watcher".into(),
            brief: "Watch open PR checks in the background and automatically wake the session on CI failure, merge conflict, or review comment.".into(),
            access: "worktree".into(),
            cwd: None,
            state: ProposalState::Pending,
        }];
    }

    pub(super) fn demo_agent_detail(&mut self) {
        self.background_tasks.open_agent("agent-bg-demo-1".into());
        self.state.messages.push(protocol::Message {
            role: "user".into(),
            text: "Worker Audit HUD layout constraints (agent-bg-demo-1) settled.\n### HUD Audit Summary\n- Audited `scenes/hud.gd` and `scripts/ui/control_groups.gd`.\n- Confirmed **0 anchor conflicts** when moving the six order buttons to a vertical left column on phones.\n- Verified desktop and tablet breakpoints keep the bottom bar intact.".into(),
            ..Default::default()
        });
        self.state.transcript_revision += 1;
    }

    pub(super) fn demo_shell_run(&mut self) {
        self.prefs.hide_inspector = true;
        self.terminals.toggle(&self.ctx, &self.prefs.cwd, true);
        self.state.messages = vec![
            protocol::Message {
                role: "user".into(),
                text: "How do I run the sandbox tests and inspect the build log in the terminal?".into(),
                ..Default::default()
            },
            protocol::Message {
                role: "assistant".into(),
                text: concat!(
                    "Run the focused sandbox test suite with a single click using the **Run** button on this block:\n\n",
                    "```bash\n",
                    "cargo test --bin pui -- sandbox\n",
                    "```\n\n",
                    "You can jump directly to the active shell output in [Shell 1 · build output](#terminal/term-1).\n\n",
                    "Multiline scripts and non-shell snippets intentionally omit the Run button so you copy/review them first:\n\n",
                    "```bash\n",
                    "cargo fmt --check\n",
                    "scripts/test-rust.sh\n",
                    "```\n\n",
                    "```rust\n",
                    "let mode = crate::sandbox::Mode::Strict;\n",
                    "```"
                )
                .into(),
                ..Default::default()
            },
        ];
        self.state.transcript_revision += 1;
    }

    pub(super) fn turn_ux_strip(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        if !self.turn_ux.has_visible_content() {
            return;
        }
        let can_send = !self.demo
            && !self.loading_chat
            && !self.deletion.confirming()
            && !self.resource_reload.confirming()
            && !self.resource_reload.pending()
            && self.history_pending.is_none()
            && !self.restoring_code();
        let can_start_session = can_send && self.ultracode_ready;
        let mut action = TurnUxAction::None;

        if let Some(plan) = &self.turn_ux.plan {
            if render_plan_strip(ui, plan) {
                action = TurnUxAction::TogglePlan;
            }
            ui.add_space(6.0);
        }

        if let Some(wrap_up) = &self.turn_ux.wrap_up
            && !wrap_up.dismissed
        {
            match render_wrap_up_box(ui, wrap_up, can_send) {
                WrapUpEvent::None => {}
                WrapUpEvent::Dismiss => action = TurnUxAction::DismissWrapUp,
                WrapUpEvent::Reply(idx) => action = TurnUxAction::WrapUpReply(idx),
            }
            ui.add_space(6.0);
        }

        for proposal in &self.turn_ux.proposals {
            if proposal.state == ProposalState::Dismissed {
                continue;
            }
            match render_proposal_card(ui, proposal, can_start_session, can_send) {
                ProposalEvent::None => {}
                ProposalEvent::Start => action = TurnUxAction::StartProposal(proposal.id.clone()),
                ProposalEvent::Open(agent_id) => action = TurnUxAction::OpenStartedAgent(agent_id),
                ProposalEvent::KeepHere => {
                    action = TurnUxAction::KeepProposalHere(proposal.id.clone())
                }
                ProposalEvent::Dismiss => {
                    action = TurnUxAction::DismissProposal(proposal.id.clone())
                }
            }
            ui.add_space(6.0);
        }

        self.apply_turn_ux_action(ctx, action);
    }

    fn apply_turn_ux_action(&mut self, ctx: &egui::Context, action: TurnUxAction) {
        match action {
            TurnUxAction::None => {}
            TurnUxAction::TogglePlan => {
                if let Some(plan) = &mut self.turn_ux.plan {
                    plan.expanded = !plan.expanded;
                }
            }
            TurnUxAction::DismissWrapUp => {
                if let Some(wrap_up) = &mut self.turn_ux.wrap_up {
                    wrap_up.dismissed = true;
                }
            }
            TurnUxAction::WrapUpReply(idx) => {
                let text = self
                    .turn_ux
                    .wrap_up
                    .as_ref()
                    .and_then(|w| w.options.get(idx))
                    .map(|opt| opt.prompt.clone().unwrap_or_else(|| opt.label.clone()));
                if let Some(text) = text
                    && self.send_turn_ux_prompt(ctx, text)
                    && let Some(wrap_up) = &mut self.turn_ux.wrap_up
                {
                    wrap_up.dismissed = true;
                }
            }
            TurnUxAction::StartProposal(proposal_id) => {
                let snapshot = self
                    .turn_ux
                    .proposals
                    .iter()
                    .find(|p| p.id == proposal_id && p.state == ProposalState::Pending)
                    .cloned();
                let Some(proposal) = snapshot else {
                    return;
                };
                let source = self.ensure_agent();
                let mut req = json!({
                    "action": "spawn",
                    "name": proposal.title,
                    "task": proposal.brief,
                    "access": proposal.access,
                });
                if let Some(cwd) = &proposal.cwd {
                    req["cwd"] = json!(cwd);
                }
                match self.agent_control(&source, &req) {
                    Ok(result) => {
                        if let Some(agent_id) = result["agent_id"].as_str()
                            && let Some(item) = self
                                .turn_ux
                                .proposals
                                .iter_mut()
                                .find(|p| p.id == proposal_id)
                        {
                            item.state = ProposalState::Started(agent_id.to_owned());
                        }
                    }
                    Err(error) => {
                        self.state.error = Some(error);
                    }
                }
            }
            TurnUxAction::OpenStartedAgent(agent_id) => {
                self.view_agent(&agent_id);
            }
            TurnUxAction::KeepProposalHere(proposal_id) => {
                let prompt = self
                    .turn_ux
                    .proposals
                    .iter()
                    .find(|p| p.id == proposal_id && p.state == ProposalState::Pending)
                    .map(|p| {
                        if p.brief.starts_with(&p.title) {
                            p.brief.clone()
                        } else {
                            format!("{}\n\n{}", p.title, p.brief)
                        }
                    });
                if let Some(prompt) = prompt
                    && self.send_turn_ux_prompt(ctx, prompt)
                    && let Some(item) = self
                        .turn_ux
                        .proposals
                        .iter_mut()
                        .find(|p| p.id == proposal_id)
                {
                    item.state = ProposalState::KeptHere;
                }
            }
            TurnUxAction::DismissProposal(proposal_id) => {
                if let Some(item) = self
                    .turn_ux
                    .proposals
                    .iter_mut()
                    .find(|p| p.id == proposal_id)
                {
                    item.state = ProposalState::Dismissed;
                }
            }
        }
    }
}

fn draw_step_indicator(ui: &mut egui::Ui, status: StepStatus, colors: &theme::Palette) {
    let (rect, _) = ui.allocate_exact_size(Vec2::splat(14.0), Sense::hover());
    match status {
        StepStatus::Completed => {
            icons::draw(ui, rect.shrink(1.0), Icon::Check, colors.green);
        }
        StepStatus::InProgress => {
            ui.painter()
                .circle_filled(rect.center(), 3.8, colors.accent);
        }
        StepStatus::Blocked => {
            ui.painter().text(
                rect.center(),
                egui::Align2::CENTER_CENTER,
                "!",
                FontId::proportional(11.5),
                colors.red,
            );
        }
        StepStatus::Pending => {
            ui.painter()
                .circle_stroke(rect.center(), 3.6, Stroke::new(1.2_f32, colors.dim));
        }
    }
}

fn render_plan_strip(ui: &mut egui::Ui, plan: &TurnPlan) -> bool {
    let colors = theme::palette(ui.ctx());
    let (done, total) = plan.counts();
    let all_done = plan.all_completed();
    let active = plan.active_step();
    let mut toggled = false;

    egui::Frame::new()
        .fill(colors.surface)
        .stroke(Stroke::new(1.0_f32, colors.border))
        .corner_radius(10)
        .inner_margin(egui::Margin::symmetric(12, 8))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            let header =
                ui.scope_builder(
                    egui::UiBuilder::new()
                        .id_salt("turn-ux-plan-header")
                        .sense(Sense::click()),
                    |ui| {
                        ui.horizontal(|ui| {
                            ui.spacing_mut().item_spacing.x = 7.0;
                            let right_reserve = 104.0;
                            let label_w = (ui.available_width() - right_reserve - 24.0).max(48.0);
                            draw_step_indicator(
                                ui,
                                if all_done {
                                    StepStatus::Completed
                                } else {
                                    active.map(|s| s.status).unwrap_or(StepStatus::Pending)
                                },
                                colors,
                            );
                            let summary = if all_done {
                                plan.title.as_deref().unwrap_or("Plan completed").to_owned()
                            } else if let Some(step) = active {
                                step.label.clone()
                            } else {
                                plan.title.clone().unwrap_or_else(|| "Plan".into())
                            };
                            ui.allocate_ui_with_layout(
                                Vec2::new(label_w, 18.0),
                                Layout::left_to_right(Align::Center),
                                |ui| {
                                    ui.add(
                                        egui::Label::new(RichText::new(summary).size(12.0).color(
                                            if all_done { colors.muted } else { colors.text },
                                        ))
                                        .truncate(),
                                    );
                                },
                            );
                            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                                let (chev_rect, _) =
                                    ui.allocate_exact_size(Vec2::splat(14.0), Sense::hover());
                                icons::draw(
                                    ui,
                                    chev_rect.shrink(1.5),
                                    if plan.expanded {
                                        Icon::Collapse
                                    } else {
                                        Icon::Expand
                                    },
                                    colors.muted,
                                );
                                let bar_w = 44.0_f32.min((ui.available_width() - 36.0).max(20.0));
                                let (bar_rect, _) =
                                    ui.allocate_exact_size(Vec2::new(bar_w, 4.0), Sense::hover());
                                ui.painter().rect_filled(bar_rect, 2, colors.border);
                                if total > 0 && done > 0 {
                                    let frac = (done as f32 / total as f32).clamp(0.0, 1.0);
                                    let fill_rect = egui::Rect::from_min_size(
                                        bar_rect.min,
                                        Vec2::new(
                                            (bar_rect.width() * frac).max(3.0),
                                            bar_rect.height(),
                                        ),
                                    );
                                    ui.painter().rect_filled(
                                        fill_rect,
                                        2,
                                        if all_done {
                                            colors.green
                                        } else {
                                            colors.accent
                                        },
                                    );
                                }
                                ui.label(
                                    RichText::new(format!("{done}/{total}"))
                                        .size(11.0)
                                        .color(colors.muted),
                                );
                            });
                        });
                    },
                );
            let resp = header
                .response
                .on_hover_cursor(egui::CursorIcon::PointingHand);
            resp.widget_info(|| {
                egui::WidgetInfo::labeled(
                    egui::WidgetType::Button,
                    true,
                    if plan.expanded {
                        "Collapse plan"
                    } else {
                        "Expand plan"
                    },
                )
            });
            if resp.clicked() {
                toggled = true;
            }

            if plan.expanded {
                ui.add_space(6.0);
                let (sep, _) =
                    ui.allocate_exact_size(Vec2::new(ui.available_width(), 1.0), Sense::hover());
                ui.painter().rect_filled(sep, 0, colors.border);
                ui.add_space(6.0);
                if let Some(title) = &plan.title {
                    ui.add(
                        egui::Label::new(
                            RichText::new(title).size(11.5).strong().color(colors.muted),
                        )
                        .wrap(),
                    );
                    ui.add_space(4.0);
                }
                egui::ScrollArea::vertical()
                    .id_salt("turn-ux-plan-steps")
                    .max_height(180.0)
                    .auto_shrink([false, true])
                    .show(ui, |ui| {
                        ui.spacing_mut().item_spacing.y = 5.0;
                        for step in &plan.steps {
                            ui.horizontal_top(|ui| {
                                ui.spacing_mut().item_spacing.x = 7.0;
                                draw_step_indicator(ui, step.status, colors);
                                let text = match step.status {
                                    StepStatus::InProgress => RichText::new(&step.label)
                                        .size(12.0)
                                        .strong()
                                        .color(colors.text),
                                    StepStatus::Completed => {
                                        RichText::new(&step.label).size(12.0).color(colors.muted)
                                    }
                                    StepStatus::Blocked => {
                                        RichText::new(&step.label).size(12.0).color(colors.red)
                                    }
                                    StepStatus::Pending => {
                                        RichText::new(&step.label).size(12.0).color(colors.muted)
                                    }
                                };
                                ui.add(egui::Label::new(text).wrap());
                            });
                        }
                    });
            }
        });
    toggled
}

enum WrapUpEvent {
    None,
    Dismiss,
    Reply(usize),
}

fn render_wrap_up_box(ui: &mut egui::Ui, wrap_up: &WrapUpBox, can_send: bool) -> WrapUpEvent {
    let colors = theme::palette(ui.ctx());
    let mut event = WrapUpEvent::None;

    prompt_ui::compact_inline(ui.ctx()).show(ui, |ui| {
        ui.set_width(ui.available_width());
        prompt_ui::style(ui);
        ui.horizontal_top(|ui| {
            let text_w = (ui.available_width() - 34.0).max(80.0);
            ui.vertical(|ui| {
                ui.set_width(text_w);
                ui.add(
                    egui::Label::new(RichText::new(&wrap_up.recap).size(12.5).color(colors.text))
                        .wrap(),
                );
            });
            ui.with_layout(Layout::right_to_left(Align::Min), |ui| {
                if prompt_ui::icon_action(ui, Icon::Close, "Dismiss wrap-up", true).clicked() {
                    event = WrapUpEvent::Dismiss;
                }
            });
        });
        if let Some(question) = &wrap_up.question {
            ui.add_space(6.0);
            ui.add(
                egui::Label::new(
                    RichText::new(question)
                        .size(12.5)
                        .strong()
                        .color(colors.text),
                )
                .wrap(),
            );
        }
        if !wrap_up.options.is_empty() {
            ui.add_space(8.0);
            ui.horizontal_wrapped(|ui| {
                ui.spacing_mut().item_spacing = Vec2::new(6.0, 6.0);
                for (idx, opt) in wrap_up.options.iter().enumerate() {
                    if ui
                        .push_id(("turn-ux-wrap-opt", idx), |ui| {
                            prompt_ui::button(
                                ui,
                                &opt.label,
                                prompt_ui::Button::Secondary,
                                can_send,
                            )
                        })
                        .inner
                        .clicked()
                    {
                        event = WrapUpEvent::Reply(idx);
                    }
                }
            });
        }
    });
    event
}

enum ProposalEvent {
    None,
    Start,
    Open(String),
    KeepHere,
    Dismiss,
}

fn render_proposal_card(
    ui: &mut egui::Ui,
    proposal: &SessionProposal,
    can_start: bool,
    can_send: bool,
) -> ProposalEvent {
    let colors = theme::palette(ui.ctx());
    let mut event = ProposalEvent::None;

    ui.push_id(&proposal.id, |ui| {
        prompt_ui::compact_inline(ui.ctx()).show(ui, |ui| {
            ui.set_width(ui.available_width());
            prompt_ui::style(ui);
            ui.horizontal_top(|ui| {
                ui.spacing_mut().item_spacing.x = 7.0;
                icons::inline(ui, Icon::Agents, colors.accent);
                let title_w = (ui.available_width() - 96.0).max(80.0);
                ui.vertical(|ui| {
                    ui.set_width(title_w);
                    ui.add(
                        egui::Label::new(
                            RichText::new(&proposal.title)
                                .size(12.5)
                                .strong()
                                .color(colors.text),
                        )
                        .wrap(),
                    );
                });
                ui.with_layout(Layout::right_to_left(Align::Min), |ui| {
                    egui::Frame::new()
                        .fill(prompt_ui::subtle(ui.ctx()))
                        .stroke(Stroke::new(1.0_f32, prompt_ui::border(ui.ctx())))
                        .corner_radius(99)
                        .inner_margin(egui::Margin::symmetric(7, 2))
                        .show(ui, |ui| {
                            ui.label(
                                RichText::new(&proposal.access)
                                    .size(10.5)
                                    .color(colors.muted),
                            );
                        });
                });
            });
            ui.add_space(4.0);
            ui.add(
                egui::Label::new(
                    RichText::new(preview_brief(&proposal.brief, 180))
                        .size(12.0)
                        .color(colors.muted),
                )
                .wrap(),
            );
            if let Some(cwd) = &proposal.cwd {
                ui.add_space(2.0);
                ui.add(
                    egui::Label::new(RichText::new(cwd).size(10.5).color(colors.dim)).truncate(),
                );
            }
            ui.add_space(8.0);
            ui.horizontal_wrapped(|ui| {
                ui.spacing_mut().item_spacing = Vec2::new(6.0, 6.0);
                match &proposal.state {
                    ProposalState::Pending => {
                        if prompt_ui::button(
                            ui,
                            "Start new session",
                            prompt_ui::Button::Primary,
                            can_start,
                        )
                        .clicked()
                        {
                            event = ProposalEvent::Start;
                        }
                        if prompt_ui::button(
                            ui,
                            "Do it here",
                            prompt_ui::Button::Secondary,
                            can_send,
                        )
                        .clicked()
                        {
                            event = ProposalEvent::KeepHere;
                        }
                        if prompt_ui::button(ui, "Dismiss", prompt_ui::Button::Secondary, true)
                            .clicked()
                        {
                            event = ProposalEvent::Dismiss;
                        }
                    }
                    ProposalState::Started(agent_id) => {
                        if prompt_ui::button(
                            ui,
                            "Started · Open ›",
                            prompt_ui::Button::Secondary,
                            true,
                        )
                        .clicked()
                        {
                            event = ProposalEvent::Open(agent_id.clone());
                        }
                        if prompt_ui::icon_action(ui, Icon::Close, "Dismiss proposal", true)
                            .clicked()
                        {
                            event = ProposalEvent::Dismiss;
                        }
                    }
                    ProposalState::KeptHere => {
                        ui.label(
                            RichText::new("Handled in this chat")
                                .size(11.5)
                                .color(colors.green),
                        );
                        if prompt_ui::icon_action(ui, Icon::Close, "Dismiss proposal", true)
                            .clicked()
                        {
                            event = ProposalEvent::Dismiss;
                        }
                    }
                    ProposalState::Dismissed => {}
                }
            });
        });
    });
    event
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_app(ctx: &egui::Context) -> Pui {
        let mut app = Pui::new(
            &eframe::CreationContext::_new_kittest(ctx.clone()),
            Options {
                demo: true,
                ..Default::default()
            },
        );
        app.demo = false;
        app.connected = true;
        app.state = State {
            model: "fixture".into(),
            provider: "local".into(),
            thinking: "high".into(),
            session_file: Some("/fixture/turn-ux-parent.jsonl".into()),
            ..Default::default()
        };
        app.client = Some(rpc::Client::stub());
        app.ultracode = true;
        app.ultracode_ready = true;
        app.ensure_agent();
        app
    }

    fn text_rects(shape: &egui::Shape, out: &mut Vec<(String, egui::Rect)>) {
        match shape {
            egui::Shape::Text(text) => out.push((
                text.galley.job.text.clone(),
                egui::Rect::from_min_size(text.pos, text.galley.size()),
            )),
            egui::Shape::Vec(shapes) => {
                for shape in shapes {
                    text_rects(shape, out);
                }
            }
            _ => {}
        }
    }

    fn render_frame(
        ctx: &egui::Context,
        app: &mut Pui,
        size: Vec2,
        events: Vec<egui::Event>,
    ) -> Vec<(String, egui::Rect)> {
        let output = ctx.run(
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, size)),
                events,
                ..Default::default()
            },
            |ctx| {
                app.conversation(ctx);
            },
        );
        let mut labels = Vec::new();
        for shape in output.shapes {
            text_rects(&shape.shape, &mut labels);
        }
        labels
    }

    fn click_label(ctx: &egui::Context, app: &mut Pui, size: Vec2, needle: &str) {
        let mut target = None;
        for _ in 0..3 {
            let labels = render_frame(ctx, app, size, vec![]);
            target = labels
                .iter()
                .find(|(text, _)| text == needle)
                .map(|(_, rect)| rect.center());
        }
        let pos = target.unwrap_or_else(|| panic!("missing rendered label {needle:?}"));
        let events = vec![
            egui::Event::PointerMoved(pos),
            egui::Event::PointerButton {
                pos,
                button: egui::PointerButton::Primary,
                pressed: true,
                modifiers: egui::Modifiers::NONE,
            },
            egui::Event::PointerButton {
                pos,
                button: egui::PointerButton::Primary,
                pressed: false,
                modifiers: egui::Modifiers::NONE,
            },
        ];
        render_frame(ctx, app, size, events);
    }

    #[test]
    fn turn_update_plan_validates_updates_and_preserves_expand_state() {
        let ctx = egui::Context::default();
        let mut app = test_app(&ctx);
        let source = app.agents.active.clone();

        let res = app
            .agent_control(
                &source,
                &json!({
                    "action": "turn_update_plan",
                    "title": "Ship Turn UX",
                    "steps": [
                        { "id": "s1", "label": "Adapter tools", "status": "completed" },
                        { "id": "s2", "label": "Host state", "status": "in_progress" },
                        { "id": "s3", "label": "UI & tests", "status": "pending" }
                    ]
                }),
            )
            .unwrap();
        assert_eq!(res["completed"], 1);
        assert_eq!(res["total"], 3);
        let plan = app.turn_ux.plan.as_ref().unwrap();
        assert_eq!(plan.title.as_deref(), Some("Ship Turn UX"));
        assert!(!plan.expanded);
        assert_eq!(plan.active_step().unwrap().id, "s2");

        // User expands the plan strip via UI click; subsequent updates preserve expanded state.
        theme::install(&ctx);
        click_label(&ctx, &mut app, Vec2::new(720.0, 560.0), "Host state");
        assert!(app.turn_ux.plan.as_ref().unwrap().expanded);
        let res = app
            .agent_control(
                &source,
                &json!({
                    "action": "turn_update_plan",
                    "title": "Ship Turn UX",
                    "steps": [
                        { "id": "s1", "label": "Adapter tools", "status": "completed" },
                        { "id": "s2", "label": "Host state", "status": "completed" },
                        { "id": "s3", "label": "UI & tests", "status": "completed" }
                    ]
                }),
            )
            .unwrap();
        assert_eq!(res["completed"], 3);
        assert!(app.turn_ux.plan.as_ref().unwrap().expanded);
        assert!(app.turn_ux.plan.as_ref().unwrap().all_completed());

        // Validation errors are rejected cleanly.
        assert!(
            app.agent_control(
                &source,
                &json!({ "action": "turn_update_plan", "steps": [] })
            )
            .is_err()
        );
        assert!(
            app.agent_control(
                &source,
                &json!({
                    "action": "turn_update_plan",
                    "steps": [
                        { "id": "dup", "label": "One", "status": "pending" },
                        { "id": "dup", "label": "Two", "status": "pending" }
                    ]
                })
            )
            .is_err()
        );
        assert!(
            app.agent_control(
                &source,
                &json!({
                    "action": "turn_update_plan",
                    "steps": [{ "id": "s1", "label": "Bad", "status": "unknown" }]
                })
            )
            .is_err()
        );
    }

    #[test]
    fn wrap_up_quick_reply_sends_prompt_and_clears_completed_plan() {
        let ctx = egui::Context::default();
        theme::install(&ctx);
        let mut app = test_app(&ctx);
        let source = app.agents.active.clone();

        app.agent_control(
            &source,
            &json!({
                "action": "turn_update_plan",
                "steps": [{ "id": "s1", "label": "Done", "status": "completed" }]
            }),
        )
        .unwrap();
        app.agent_control(
            &source,
            &json!({
                "action": "turn_wrap_up",
                "recap": "Finished wiring Turn UX controls and tests.",
                "question": "What should we do next?",
                "options": [
                    { "label": "Run full suite", "prompt": "Run scripts/test-rust.sh and report results." },
                    { "label": "Inspect diff" }
                ]
            }),
        )
        .unwrap();
        assert!(app.turn_ux.wrap_up.is_some());

        click_label(&ctx, &mut app, Vec2::new(720.0, 560.0), "Run full suite");

        assert!(app.turn_ux.wrap_up.is_none());
        assert!(
            app.turn_ux.plan.is_none(),
            "completed plan should clear on new user prompt"
        );
        assert_eq!(app.deliveries.len(), 1);
        assert_eq!(
            app.deliveries[0].message.text,
            "Run scripts/test-rust.sh and report results."
        );
    }

    #[test]
    fn user_submit_clears_wrap_up_but_keeps_in_progress_plan() {
        let ctx = egui::Context::default();
        let mut app = test_app(&ctx);
        let source = app.agents.active.clone();

        app.agent_control(
            &source,
            &json!({
                "action": "turn_update_plan",
                "steps": [
                    { "id": "s1", "label": "Done", "status": "completed" },
                    { "id": "s2", "label": "Still working", "status": "in_progress" }
                ]
            }),
        )
        .unwrap();
        app.agent_control(
            &source,
            &json!({
                "action": "turn_wrap_up",
                "recap": "Halfway through.",
                "options": [{ "label": "Continue" }]
            }),
        )
        .unwrap();

        app.draft = "Please keep going".into();
        app.submit(&ctx);

        assert!(app.turn_ux.wrap_up.is_none());
        assert!(
            app.turn_ux.plan.is_some(),
            "incomplete plan should remain visible across turns"
        );
    }

    #[test]
    fn proposal_card_starts_sidebar_session_keeps_here_and_dismisses() {
        let ctx = egui::Context::default();
        theme::install(&ctx);
        let mut app = test_app(&ctx);
        let source = app.agents.active.clone();
        let size = Vec2::new(760.0, 620.0);

        // Start a read-only proposed session via the UI button.
        let p1 = app
            .agent_control(
                &source,
                &json!({
                    "action": "turn_propose_session",
                    "title": "Audit CI scripts",
                    "brief": "Inspect scripts/ and report redundant checks.",
                    "access": "read-only"
                }),
            )
            .unwrap();
        assert_eq!(p1["state"], "pending");

        click_label(&ctx, &mut app, size, "Start new session");

        let spawned_id = match &app.turn_ux.proposals[0].state {
            ProposalState::Started(id) => id.clone(),
            other => panic!("expected Started state, got {other:?}"),
        };
        assert!(app.agents.nodes.contains_key(&spawned_id));
        assert!(!app.agents.nodes[&spawned_id].background);

        // Clicking "Started · Open ›" switches to the spawned sidebar child session.
        click_label(&ctx, &mut app, size, "Started · Open ›");
        assert_eq!(app.agents.active, spawned_id);

        // Switch back to parent and test "Dismiss" and "Do it here".
        app.view_agent(&source);
        assert_eq!(app.agents.active, source);

        app.agent_control(
            &source,
            &json!({
                "action": "turn_propose_session",
                "title": "Follow-up cleanup",
                "brief": "Remove unused helper after refactor."
            }),
        )
        .unwrap();
        click_label(&ctx, &mut app, size, "Dismiss");
        assert_eq!(app.turn_ux.proposals[1].state, ProposalState::Dismissed);

        app.agent_control(
            &source,
            &json!({
                "action": "turn_propose_session",
                "title": "Tighten docs",
                "brief": "Document the new flags."
            }),
        )
        .unwrap();
        click_label(&ctx, &mut app, size, "Do it here");
        let kept = app
            .turn_ux
            .proposals
            .iter()
            .find(|p| p.title == "Tighten docs")
            .expect("kept proposal should remain visible");
        assert_eq!(kept.state, ProposalState::KeptHere);
        assert_eq!(app.deliveries.len(), 1);
        assert!(app.deliveries[0].message.text.contains("Tighten docs"));

        // Read-only workers cannot propose sessions.
        assert!(
            app.agent_control(
                &spawned_id,
                &json!({
                    "action": "turn_propose_session",
                    "title": "Blocked",
                    "brief": "Should fail"
                })
            )
            .is_err()
        );
    }

    #[test]
    fn switching_chats_preserves_each_chats_turn_ux_state() {
        let ctx = egui::Context::default();
        let mut app = test_app(&ctx);
        let parent = app.agents.active.clone();
        let child = app
            .agent_control(
                &parent,
                &json!({
                    "action": "spawn",
                    "name": "Child session",
                    "task": "Work independently",
                    "access": "read-only"
                }),
            )
            .unwrap()["agent_id"]
            .as_str()
            .unwrap()
            .to_owned();

        app.agent_control(
            &parent,
            &json!({
                "action": "turn_update_plan",
                "title": "Parent plan",
                "steps": [{ "id": "p1", "label": "Parent step", "status": "in_progress" }]
            }),
        )
        .unwrap();
        app.agent_control(
            &parent,
            &json!({
                "action": "turn_wrap_up",
                "recap": "Parent recap",
                "options": [{ "label": "Parent next" }]
            }),
        )
        .unwrap();
        app.agent_control(
            &parent,
            &json!({
                "action": "turn_propose_session",
                "title": "Parent proposal",
                "brief": "Parent follow-up"
            }),
        )
        .unwrap();

        // Switch to child: child has clean TurnUx initially, then gets its own plan.
        app.view_agent(&child);
        assert!(app.turn_ux.plan.is_none());
        assert!(app.turn_ux.wrap_up.is_none());
        assert!(app.turn_ux.proposals.is_empty());

        app.agent_control(
            &child,
            &json!({
                "action": "turn_update_plan",
                "title": "Child plan",
                "steps": [{ "id": "c1", "label": "Child step", "status": "pending" }]
            }),
        )
        .unwrap();

        // Switch back to parent: all parent TurnUx state is intact.
        app.view_agent(&parent);
        assert_eq!(
            app.turn_ux.plan.as_ref().unwrap().title.as_deref(),
            Some("Parent plan")
        );
        assert_eq!(app.turn_ux.wrap_up.as_ref().unwrap().recap, "Parent recap");
        assert_eq!(app.turn_ux.proposals.len(), 1);
        assert_eq!(app.turn_ux.proposals[0].title, "Parent proposal");

        // Switch back to child: child plan is intact too.
        app.view_agent(&child);
        assert_eq!(
            app.turn_ux.plan.as_ref().unwrap().title.as_deref(),
            Some("Child plan")
        );
    }

    #[test]
    fn turn_ux_renders_cleanly_in_light_dark_themes_and_narrow_widths() {
        for mode in [theme::Theme::Light, theme::Theme::Dark] {
            for size in [Vec2::new(360.0, 680.0), Vec2::new(760.0, 680.0)] {
                let ctx = egui::Context::default();
                theme::install(&ctx);
                mode.apply(&ctx);
                let mut app = test_app(&ctx);
                let source = app.agents.active.clone();

                app.agent_control(
                    &source,
                    &json!({
                        "action": "turn_update_plan",
                        "title": "Refactor transport",
                        "steps": [
                            { "id": "s1", "label": "Inspect RPC bridge", "status": "completed" },
                            { "id": "s2", "label": "Implement live plan strip", "status": "in_progress" },
                            { "id": "s3", "label": "Verify blocked state", "status": "blocked" },
                            { "id": "s4", "label": "Final check", "status": "pending" }
                        ]
                    }),
                )
                .unwrap();
                app.turn_ux.plan.as_mut().unwrap().expanded = true;
                app.agent_control(
                    &source,
                    &json!({
                        "action": "turn_wrap_up",
                        "recap": "Implemented the plan strip, wrap-up card, and proposal cards with per-chat persistence.",
                        "question": "Ready to run the full validation suite?",
                        "options": [
                            { "label": "Run tests" },
                            { "label": "Review changes" }
                        ]
                    }),
                )
                .unwrap();
                app.agent_control(
                    &source,
                    &json!({
                        "action": "turn_propose_session",
                        "title": "Benchmark startup time",
                        "brief": "Profile cold launch and RPC connect latency on macOS."
                    }),
                )
                .unwrap();

                let mut labels = Vec::new();
                for _ in 0..3 {
                    labels = render_frame(&ctx, &mut app, size, vec![]);
                }
                let screen = ctx.screen_rect();
                for needle in [
                    "1/4",
                    "Inspect RPC bridge",
                    "Run tests",
                    "Benchmark startup time",
                    "Start new session",
                    "Do it here",
                    "Dismiss",
                ] {
                    let (_, rect) = labels
                        .iter()
                        .find(|(text, _)| text.contains(needle))
                        .unwrap_or_else(|| panic!("missing {needle:?} in {mode:?} at {size:?}"));
                    assert!(
                        rect.left() >= screen.left() - 1.0 && rect.right() <= screen.right() + 1.0,
                        "{needle:?} horizontal overflow {rect:?} outside {screen:?} in {mode:?} at {size:?}"
                    );
                }
            }
        }
    }
}
