//! CLI drives the same supervisor as the UI, then exits at acceptance approval.
use super::*;
impl Pui {
    pub(crate) fn run_task_proposal(path: &std::path::Path) -> Result<(), String> {
        // Keep AppKit setup off the headless worker. This starts no ordinary chat.
        let path = path.to_path_buf();
        std::thread::spawn(move || Self::proposal_worker(&path))
            .join()
            .map_err(|_| "Proposal worker failed; no execution was approved".to_owned())?
    }
    fn proposal_worker(path: &std::path::Path) -> Result<(), String> {
        let task = domain::proposal::Request::load(path)?.task()?;
        let ctx = egui::Context::default();
        let mut app = Pui::new(
            &eframe::CreationContext::_new_kittest(ctx.clone()),
            Options {
                demo: true,
                cwd: Some(task.source.clone()),
                ..Default::default()
            },
        );
        app.demo = false;
        app.tasks = Tasks::new(&ctx, true);
        app.state = State::default();
        app.sessions.clear();
        app.agents = Default::default();
        app.prefs.settings.permission = settings::PermissionMode::Manual;
        let mut discovery = rpc::Client::start_with(
            &task.source,
            None,
            ctx.clone(),
            rpc::Launch {
                args: vec![
                    "--no-extensions".into(),
                    "--no-mcp".into(),
                    "--no-tools".into(),
                    "--no-session".into(),
                ],
                ..Default::default()
            },
        );
        let id = discovery.enqueue(protocol::command("get_available_models"))?;
        let deadline = std::time::Instant::now() + Duration::from_secs(30);
        loop {
            if std::time::Instant::now() > deadline {
                return Err("Model discovery timed out before any prompt.".into());
            }
            match discovery.events.recv_timeout(Duration::from_millis(100)) {
                Ok(rpc::Event::Record(record))
                    if record["type"] == "response" && record["id"] == id =>
                {
                    if record["success"] != true {
                        return Err("Model discovery failed before any prompt.".into());
                    }
                    app.state.apply(record);
                    break;
                }
                Ok(rpc::Event::Error(_) | rpc::Event::Exited) => {
                    return Err("Model discovery disconnected before any prompt.".into());
                }
                _ => {}
            }
        }
        drop(discovery);
        let deadline = std::time::Instant::now() + Duration::from_secs(10);
        while app.tasks.loaded.is_some() {
            app.tasks.persist_loaded();
            if std::time::Instant::now() > deadline {
                return Err("Task store did not initialize.".into());
            }
            std::thread::sleep(Duration::from_millis(20));
        }
        if let Some(e) = app.tasks.error.clone() {
            return Err(e);
        }
        let id = task.id.clone();
        app.tasks.selected = Some(id.clone());
        app.tasks.records.insert(0, task);
        app.task_propose();
        let deadline = std::time::Instant::now() + Duration::from_secs(15 * 60);
        let mut last = String::new();
        loop {
            app.poll_tasks();
            let t = app.tasks.current().ok_or("Proposal task missing")?;
            let signature = format!("{}:{}", t.phase.label(), t.runs.len());
            if signature != last {
                eprintln!(
                    "{} · {} recorded runs · ${:.4}",
                    t.phase.label(),
                    t.runs.len(),
                    t.cost()
                );
                last = signature;
            }
            if t.phase == Phase::AwaitingApproval {
                let result = t.clone();
                let store = domain::store::home()
                    .ok_or("Task store unavailable")?
                    .join(format!("{id}.json"));
                app.tasks.shutdown();
                println!("{}",serde_json::to_string_pretty(&json!({"task_id":id,"task_file":store,"phase":result.phase,"source":result.source,"reported_cost":result.cost(),"threshold":result.spend_limit,"criteria":result.proposed,"runs":result.runs})).map_err(|e|e.to_string())?);
                return Ok(());
            }
            if !t.phase.active() {
                let message = format!("{}: {}", t.phase.label(), t.note);
                app.tasks.shutdown();
                return Err(message);
            }
            if app
                .tasks
                .processes
                .iter()
                .any(|p| !p.state.dialogs.is_empty())
            {
                app.tasks.block("Proposal requires interactive input. No prompt was approved automatically; inspect the saved task in Pui.");
                app.tasks.shutdown();
                return Err("Interactive input required; proposal paused safely.".into());
            }
            if std::time::Instant::now() > deadline {
                app.tasks.stop();
                app.tasks.shutdown();
                return Err(
                    "Proposal pilot exceeded 15 minutes. No implementation started.".into(),
                );
            }
            std::thread::sleep(Duration::from_millis(30));
        }
    }
}
