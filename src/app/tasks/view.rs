use super::*;

impl Pui {
    pub(in crate::app) fn task_panel(&mut self, ctx: &egui::Context) {
        let colors = theme::palette(ctx);
        egui::CentralPanel::default().frame(egui::Frame::new().fill(colors.bg).inner_margin(22)).show(ctx,|ui|{
            ui.horizontal(|ui|{
                ui.heading("Verified task");
                if self.demo {ui.label(RichText::new("DEMO · no execution").color(colors.amber));}
                ui.with_layout(Layout::right_to_left(Align::Center),|ui|{
                    if ui.button("Back to chat").clicked(){self.tasks.open=false;}
                    if ui.button("New task").clicked(){self.new_task();}
                });
            });
            if self.tasks.running() && self.tasks.live != self.tasks.selected {
                ui.horizontal_wrapped(|ui| { ui.label("Another task is active."); if ui.button("Open active task").clicked() { self.tasks.selected = self.tasks.live.clone(); self.tasks.selected_run = None; } });
            }
            let mut selected=self.tasks.selected.clone();
            egui::ComboBox::from_id_salt("task-history").selected_text(self.tasks.current().map(|t|if t.objective.is_empty(){"New task".into()}else{t.objective.chars().take(80).collect::<String>()}).unwrap_or_else(||"Choose task".into())).width(ui.available_width()).show_ui(ui,|ui|{
                for t in &self.tasks.records {ui.selectable_value(&mut selected,Some(t.id.clone()),format!("{} · {}",t.phase.label(),if t.objective.is_empty(){"Untitled"}else{&t.objective}));}
            });
            if selected!=self.tasks.selected {self.tasks.selected=selected;self.tasks.selected_run=None;self.tasks.transcript=Default::default();self.tasks.revalidate_at=Some(std::time::Instant::now());}
            if let Some(e)=&self.tasks.error {ui.add(egui::Label::new(RichText::new(e).color(colors.red)).wrap());}
            ui.separator();
            let Some(task)=self.tasks.current().cloned() else {ui.label("Create a task to agree on checks, implement, and independently verify the result.");return;};
            if task.phase==Phase::Draft {
                egui::ScrollArea::vertical().id_salt("task-draft").show(ui,|ui|{
                    ui.label("Workspace");ui.add(egui::Label::new(task.source.display().to_string()).selectable(true).wrap());
                    if ui.button("Use current chat workspace and model").clicked() {
                        let source=self.prefs.cwd.clone(); let provider=self.state.provider.clone(); let model=self.state.model.clone(); let thinking=self.state.thinking.clone();
                        if let Some(t)=self.tasks.current_mut() { t.source=source; t.provider=provider; t.model=model; t.thinking=thinking; }
                        self.tasks.save();
                    }
                    ui.label(theme::small(ui,"Git: isolated worktree. Non-Git: work in place. Existing uncommitted Git changes are never silently copied or omitted."));
                    ui.add_space(10.0);ui.label("Objective");
                    let t=self.tasks.current_mut().unwrap();
                    ui.add(egui::TextEdit::multiline(&mut t.objective).desired_rows(4).desired_width(f32::INFINITY).char_limit(16000).hint_text("What observable behavior should change?"));
                    ui.label("Constraints / out of scope");
                    ui.add(egui::TextEdit::multiline(&mut t.constraints).desired_rows(3).desired_width(f32::INFINITY).char_limit(12000).hint_text("Compatibility, design constraints, protected behavior…"));
                    ui.add_space(10.0);
                    let t=self.tasks.current_mut().unwrap();
                    ui.checkbox(&mut t.dynamic_routing,"Dynamic per-stage model routing");
                    let mut limited=t.spend_limit.is_some();
                    if ui.checkbox(&mut limited,"Stop at a reported spend threshold").changed() { t.spend_limit=limited.then_some(10.0); }
                    if let Some(limit)=&mut t.spend_limit { ui.add(egui::DragValue::new(limit).range(0.1..=1000.0).prefix("$ ").speed(0.5)); }
                    ui.label(theme::small(ui,if t.dynamic_routing { "A tool-free Haiku assessment selects each stage: Haiku for easy work, Opus 5.5 / GPT-6.1 Sol for harder work. Fable/Astra always requires an explicit exceptional-model approval." } else { "Pinned to the chat model; no automatic model substitution." }));
                    ui.label(format!("Provider: {} · pinned model: {} · {}",task.provider,task.model,thinking_label(&task.thinking)));
                    ui.label(theme::small(ui,"Router and proposal calls incur provider usage. No implementation until you approve the checks. Reported thresholds can overshoot on in-flight usage; not a hard cap."));
                    if ui.add_enabled(!self.demo&&self.tasks.loaded.is_none()&&self.tasks.live.is_none(),egui::Button::new("Generate acceptance checks · uses provider")).clicked(){self.task_propose();}
                });
                if self.tasks.current().is_some_and(|t| t.objective != task.objective || t.constraints != task.constraints || t.dynamic_routing != task.dynamic_routing || t.spend_limit != task.spend_limit) { self.tasks.save(); }
                return;
            }
            ui.horizontal_wrapped(|ui|{
                ui.label(RichText::new(task.phase.label()).strong());
                ui.label(theme::small(ui,if task.dynamic_routing {format!("${:.3} reported · dynamic routing · {}",task.cost(),task.provider)} else {format!("${:.3} reported · {} / {}",task.cost(),task.provider,task.model)}));
                if task.phase.active() && ui.add_enabled(!self.demo, egui::Button::new("Stop task")).clicked(){self.tasks.stop();}
            });
            if task.phase.active() && self.tasks.live.as_ref() != Some(&task.id) && !self.demo { return; }
            if !task.note.is_empty(){ui.add(egui::Label::new(theme::small(ui,&task.note)).wrap());}
            self.task_dialog(ui);
            self.task_routing_details(ui, &task);
            if let Some(pending)=&task.pending_route {
                ui.add(egui::Label::new(format!("Exceptional model requested: {} / {}\n{}",pending.selection.provider,pending.selection.model,pending.selection.reason)).wrap());
                ui.horizontal_wrapped(|ui| {
                    if ui.button("Approve exceptional model · uses provider").clicked() { self.task_approve_exceptional(); }
                    if ui.button("Decline exceptional model").clicked() { if let Some(t)=self.tasks.current_mut(){t.pending_route=None;t.note="Exceptional routing declined. No model call was made; start a new task to revise scope or policy.".into();}self.tasks.save(); }
                });
            }
            if task.phase==Phase::AwaitingApproval {
                egui::ScrollArea::vertical().id_salt("task-contract-approval").show(ui,|ui|{
                    ui.label(RichText::new(&task.objective).strong());
                    ui.label(theme::small(ui,"Approve observable requirements and exact commands. Commands run through the existing Pi permission gate and may execute project code; this is not an OS sandbox."));
                    let t=self.tasks.current_mut().unwrap();
                    for c in &mut t.proposed.criteria {
                        ui.push_id(&c.id,|ui|{
                            ui.add_space(8.0);ui.label(&c.id);
                            ui.add(egui::TextEdit::multiline(&mut c.description).desired_rows(2).desired_width(f32::INFINITY).char_limit(4000));
                            ui.add(egui::TextEdit::multiline(&mut c.command).font(egui::TextStyle::Monospace).desired_rows(2).desired_width(f32::INFINITY).char_limit(4000).hint_text("Exact local test/build command; blank means review only"));
                        });
                    }
                    if task.dirty_source {
                        ui.add_space(10.0);ui.label(RichText::new("Source has uncommitted changes").color(colors.amber));
                        ui.checkbox(&mut self.tasks.dirty_confirmed,"Use committed HEAD only; leave uncommitted changes out of this task");
                        ui.label(theme::small(ui,"Otherwise cancel and prepare the source yourself. Pui will not commit or copy it for you."));
                    }
                    ui.add_space(10.0);ui.label(theme::small(ui,"Approval starts provider-backed implementation and fresh verification, including up to two automatic repair rounds under the displayed routing policy. Fable/Astra require separate approval. Each process has a 30-minute deadline. No automatic commits, merges or deployments. Model tools are bounded; test commands keep existing permissions."));
                    ui.horizontal_wrapped(|ui|{
                        if ui.add_enabled(!self.demo,egui::Button::new("Approve checks and start · uses provider")).clicked(){self.task_start();}
                        if ui.button("Cancel task").clicked(){if let Some(t)=self.tasks.current_mut(){t.phase=Phase::Cancelled;t.note="Cancelled before implementation.".into();}self.tasks.save();}
                    });
                });
                if self.tasks.current().is_some_and(|t| serde_json::to_value(&t.proposed).ok() != serde_json::to_value(&task.proposed).ok()) { self.tasks.save(); }
                return;
            }
            if let Some(path)=&task.workspace {
                ui.horizontal_wrapped(|ui|{
                    ui.label(theme::small(ui,if task.isolated{"Isolated worktree"}else{"Working in place · non-Git"}));
                    ui.add(egui::Label::new(theme::small(ui,path.display().to_string())).selectable(true).wrap());
                    if ui.button("Copy path").clicked(){ui.ctx().copy_text(path.display().to_string());}
                });
            }
            ui.horizontal_wrapped(|ui|{
                if ui.selectable_label(self.tasks.selected_run.is_none(),"Results & evidence").clicked(){self.tasks.selected_run=None;}
                if task.phase==Phase::Ready && ui.add_enabled(self.tasks.live.is_none()&&!self.demo,egui::Button::new("Accept this result")).clicked(){self.task_fingerprint(FingerprintStep::Accept);}
                if matches!(task.phase,Phase::Ready|Phase::Accepted) && ui.add_enabled(self.tasks.live.is_none()&&!self.demo,egui::Button::new("Revalidate source")).clicked(){self.task_fingerprint(FingerprintStep::Revalidate);}
            });
            if let Some(run)=self.tasks.selected_run.clone() {
                let state=self.tasks.processes.iter().find(|p|p.run==run).map(|p|&p.state).or_else(||self.tasks.histories.get(&run));
                if let Some(state)=state {
                    let state=state.clone();
                    if !self.tasks.processes.iter().any(|p|p.run==run) { ui.label(theme::small(ui,"Saved transcript · bounded local preview; the Pi session file retains full history.")); }
                    self.tasks.transcript.task_output = task.runs.iter().find(|r| r.id == run).is_some_and(|r| matches!(r.role, Role::Proposal | Role::Verifier));
                    let selection=task.runs.iter().find(|r|r.id==run).and_then(|r|r.selection.as_ref());
                    self.tasks.transcript.show(ui,&state.messages,selection.map(|s|s.model.as_str()).unwrap_or(&task.model),selection.map(|s|s.thinking.as_str()).unwrap_or(&task.thinking),!state.ready(),&run);
                } else {
                    if self.tasks.preview.is_none() {
                        if let Some(path) = task.runs.iter().find(|r|r.id==run).and_then(|r|r.session.clone()) {
                            let (tx,rx)=mpsc::channel(); let ctx=ctx.clone();
                            self.tasks.preview=Some((run.clone(),rx));
                            std::thread::spawn(move || { let _=tx.send(crate::history::load(&path)); ctx.request_repaint(); });
                        }
                    }
                    egui::ScrollArea::vertical().id_salt("saved-task-run").show(ui,|ui|{
                        if let Some(r)=task.runs.iter().find(|r|r.id==run){
                            ui.label(r.role.label());
                            if !matches!(r.role, Role::Proposal | Role::Verifier) || !crate::task_output::show(ui, &r.summary) {
                                ui.add(egui::Label::new(&r.summary).selectable(true).wrap());
                            }
                            if let Some(path)=&r.session{ui.label(theme::small(ui,"Full transcript retained in Pi session:"));ui.add(egui::Label::new(path.display().to_string()).selectable(true).wrap());}
                        }
                    });
                }
            } else {
                egui::ScrollArea::vertical().id_salt("task-evidence").show(ui,|ui|{
                    ui.label(RichText::new(&task.objective).strong());
                    if let Some(c)=&task.approved {for criterion in &c.criteria {ui.label(format!("{} · {}",criterion.id,criterion.description));}}
                    for attempt in task.attempts.iter().rev(){
                        ui.add_space(10.0);ui.heading(format!("Attempt {}{}",attempt.number+1,if attempt.invalidated{" · stale / superseded"}else{""}));
                        if let Some(f)=&attempt.fingerprint{ui.add(egui::Label::new(theme::small(ui,format!("Source SHA-256: {f}"))).selectable(true).wrap());}
                        ui.label(theme::small(ui,"Fingerprint scope: Git tracked + nonignored files; non-Git excludes generated directories (target, dist, build, node_modules, caches). Not an external-process lock."));
                        if !attempt.note.is_empty(){ui.label(&attempt.note);}
                        if !attempt.changes.is_empty() { ui.label(RichText::new("Recorded candidate edits").strong()); }
                        for (index, change) in attempt.changes.iter().enumerate() {
                            egui::CollapsingHeader::new(format!("{} · +{} −{}", change.path, change.added, change.removed)).id_salt((&task.id, attempt.number, "patch", index)).show(ui, |ui| {
                                ui.label(theme::small(ui, "Recorded replacements, not a net Git diff. Source fingerprint covers the candidate; shell side effects are not represented here."));
                                ui.add(egui::Label::new(RichText::new(&change.diff).monospace()).selectable(true).wrap());
                            });
                        }
                        for check in &attempt.checks {
                            egui::CollapsingHeader::new(format!("{} · {} · exit {} · {} ms",check.criterion,if check.passed(){"passed"}else{"failed / incomplete"},check.exit_code.map(|e|e.to_string()).unwrap_or_else(||"unknown".into()),check.duration_ms)).id_salt((&task.id,attempt.number,&check.criterion)).default_open(!check.passed()).show(ui,|ui|{
                                approval_view::arguments(ui,&json!({"command":check.command,"output":check.output}));
                            });
                        }
                        if let Some(review)=&attempt.review {
                            ui.label(RichText::new("Independent review").strong());ui.add(egui::Label::new(&review.summary).wrap().selectable(true));
                            for f in &review.criteria{ui.add(egui::Label::new(format!("{} · {:?}: {}",f.id,f.status,f.evidence)).wrap().selectable(true));}
                        }
                    }
                });
            }
        });
    }
    pub(super) fn task_routing_details(&self, ui: &mut egui::Ui, task: &Task) {
        if task.dynamic_routing || task.runs.iter().any(|r| r.selection.is_some()) {
            ui.collapsing("Model choices & routing reasons",|ui| {
                if task.runs.is_empty(){ui.label("Model selection happens before each stage, using the available catalog and task evidence.");}
                for run in &task.runs {
                    if let Some(selection)=&run.selection {
                        let label=run.routing_for.map(|r|format!("Router → {}",r.label())).unwrap_or_else(||run.role.label().into());
                        ui.add(egui::Label::new(format!("{} · attempt {} · {} / {} · {} · ${:.4}\n{}",label,run.attempt+1,selection.provider,selection.model,thinking_label(&selection.thinking),run.cost+run.permission_cost,selection.reason)).wrap().selectable(true));
                    }
                }
                if let Some(limit)=task.spend_limit{ui.label(format!("Reported stop threshold: ${limit:.2} (not a hard cap)"));}
            });
        }
    }
    fn task_dialog(&mut self, ui: &mut egui::Ui) {
        let Some((run, dialog)) = self
            .tasks
            .processes
            .iter()
            .find_map(|p| p.state.dialogs.front().map(|d| (p.run.clone(), d.clone())))
        else {
            return;
        };
        let id = protocol::string(&dialog, "id");
        if self.tasks.current_dialog != id {
            self.tasks.current_dialog = id.into();
            self.tasks.dialog_text = protocol::string(&dialog, "prefill").into();
        }
        if let Some(request) = questions::Request::parse(&dialog) {
            if let Some(answer) = self
                .tasks
                .questions
                .show(ui, &dialog, &request, false, true)
            {
                self.task_answer(&run, id, answer);
            }
            return;
        }
        let mut answer = None;
        egui::Frame::group(ui.style()).show(ui, |ui| {
            ui.label(
                RichText::new(format!(
                    "{} · action required",
                    self.tasks
                        .processes
                        .iter()
                        .find(|p| p.run == run)
                        .unwrap()
                        .role
                        .label()
                ))
                .strong(),
            );
            let permission = super::super::prompts::approval(&dialog);
            egui::ScrollArea::vertical()
                .id_salt(("task-permission", id))
                .max_height(210.0)
                .show(ui, |ui| {
                    if let Some(p) = &permission {
                        ui.label(&p.title);
                        ui.add(egui::Label::new(&p.explanation).wrap());
                        approval_view::arguments(ui, &p.args);
                    } else {
                        ui.add(egui::Label::new(protocol::string(&dialog, "title")).wrap());
                        ui.add(egui::Label::new(protocol::string(&dialog, "message")).wrap());
                    }
                });
            ui.horizontal_wrapped(|ui| {
                if let Some(p) = &permission {
                    if ui.button("Allow once").clicked() {
                        answer = Some(super::super::prompts::permission_response(p, false));
                    }
                    if p.always && ui.button("Always allow exact action").clicked() {
                        answer = Some(super::super::prompts::permission_response(p, true));
                    }
                } else {
                    match protocol::string(&dialog, "method") {
                        "confirm" => {
                            if ui.button("Confirm").clicked() {
                                answer = Some(json!({"confirmed":true}));
                            }
                        }
                        "select" => {
                            if let Some(options) = dialog["options"].as_array() {
                                for o in options.iter().filter_map(Value::as_str) {
                                    if ui.button(o).clicked() {
                                        answer = Some(json!({"value":o}));
                                    }
                                }
                            }
                        }
                        "input" | "editor" => {
                            ui.add(
                                egui::TextEdit::multiline(&mut self.tasks.dialog_text)
                                    .desired_rows(2),
                            );
                            if ui.button("Send answer").clicked() {
                                answer = Some(json!({"value":self.tasks.dialog_text}));
                            }
                        }
                        _ => {
                            ui.label("Unsupported extension prompt; deny to stop safely.");
                        }
                    }
                }
                if ui.button("Deny / cancel").clicked() {
                    answer = Some(json!({"cancelled":true}));
                }
            });
        });
        if let Some(answer) = answer {
            self.task_answer(&run, id, answer);
        }
    }
    pub(in crate::app) fn demo_task(&mut self) {
        let mut t = Task::new(
            self.prefs.cwd.clone(),
            "example-provider".into(),
            "example-model".into(),
            "high".into(),
        );
        t.objective = "Preserve the unsent draft when switching chats".into();
        t.parent_session = self.state.session_file.as_ref().map(PathBuf::from);
        t.phase = Phase::Verifying;
        t.workspace = Some(PathBuf::from("/demo/isolated-worktree"));
        t.isolated = true;
        t.proposed = Contract {
            criteria: vec![domain::Criterion {
                id: "C1".into(),
                description: "Switch away and back: exact draft is retained".into(),
                command: "cargo test task_draft_survives_chat_switch".into(),
            }],
        };
        t.approved = Some(t.proposed.clone());
        let mut a = Attempt::new(0);
        a.fingerprint = Some("demo-source-fingerprint-not-real".into());
        a.checks.push(Check {
            criterion: "C1".into(),
            command: t.proposed.criteria[0].command.clone(),
            exit_code: Some(0),
            output: "DEMO ONLY: example regression test passed. No command was executed.".into(),
            truncated: false,
            cancelled: false,
            duration_ms: 81,
        });
        t.attempts.push(a);
        for role in [Role::Proposal, Role::Builder, Role::Checks, Role::Verifier] {
            t.runs.push(Run {
                id: format!("demo-{role:?}"),
                role,
                attempt: 0,
                status: if role == Role::Verifier {
                    "Working"
                } else {
                    "Finished"
                }
                .into(),
                session: None,
                summary: if role == Role::Proposal {
                    serde_json::to_string(&t.proposed).unwrap()
                } else {
                    "DEMO: example agent transcript, not a provider run.".into()
                },
                cost: if role == Role::Builder { 0.42 } else { 0.03 },
                permission_cost: 0.0,
                selection: None,
                routing_for: None,
            });
        }
        let runs = t.runs.clone();
        for run in runs {
            self.tasks.histories.insert(
                run.id.clone(),
                State {
                    model: t.model.clone(),
                    provider: t.provider.clone(),
                    thinking: t.thinking.clone(),
                    messages: vec![Message {
                        role: "assistant".into(),
                        text: if run.role == Role::Proposal {
                            run.summary.clone()
                        } else {
                            format!("DEMO ONLY · {}\n\n{}", run.role.label(), run.summary)
                        },
                        ..Default::default()
                    }],
                    ..Default::default()
                },
            );
        }
        self.tasks.selected = Some(t.id.clone());
        self.tasks.records = vec![t];
        self.tasks.open = true;
        self.prefs.cards.hidden.remove(&Card::Workflow);
        self.prefs.cards.collapsed.remove(&Card::Workflow);
        self.prefs
            .cards
            .order
            .retain(|card| *card != Card::Workflow);
        self.prefs.cards.order.insert(0, Card::Workflow);
        for card in [Card::Environment, Card::Context, Card::Outputs, Card::Pulls] {
            self.prefs.cards.collapsed.insert(card);
        }
    }
}
