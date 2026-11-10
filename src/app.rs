mod agents;
mod approval_view;
mod attachments;
mod changes;
mod composer_history;
mod diagnostics;
mod files;
mod menu;
mod model_picker;
mod new_chat;
mod notifications;
mod permission_picker;
#[cfg(test)]
mod persistence_tests;
mod prompts;
mod questions;
mod rewind;
mod run_status;
mod settings;
mod sidebar;
mod sync;
mod tasks;
mod terminal;
mod updates;
mod views;

use crate::{
    Options,
    activity::{self, RecordedFile},
    icons::{self, Icon},
    inspector::{self, Card, CardLayout},
    markdown,
    protocol::{self, Message, State, Tool},
    rpc,
    sessions::{self, Session},
    theme,
    transcript::Transcript,
    workspace::{self, Workspace},
};
use eframe::egui::{self, Align, Color32, FontId, Layout, RichText, Sense, Stroke, Vec2};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::{
    path::PathBuf,
    sync::mpsc::{self, Receiver},
    time::Duration,
};

#[derive(Default, Serialize, Deserialize)]
#[serde(default)]
struct Preferences {
    cwd: PathBuf,
    settings: settings::Settings,
    notifications: notifications::NotificationSettings,
    theme: theme::Theme,
    agent_concurrency: usize,
    agent_tree: Vec<agents::AgentRecord>,
    hide_sidebar: bool,
    hide_inspector: bool,
    cards: CardLayout,
    chats: std::collections::BTreeMap<PathBuf, sidebar::ChatPrefs>,
    projects: Vec<PathBuf>,
    projects_view: bool,
    last_session: Option<PathBuf>,
}

impl Preferences {
    fn startup_session(&self, options: &Options) -> Option<PathBuf> {
        if options.demo {
            return None;
        }
        options.session.clone().or_else(|| {
            // An explicit workspace opens a new chat there, not the old workspace's chat.
            if options.cwd.is_some() {
                None
            } else {
                self.last_session.clone().filter(|path| path.is_file())
            }
        })
    }
}

pub struct Pui {
    prefs: Preferences,
    sync: sync::WindowSync,
    notifications: notifications::Notifications,
    menu: menu::Menu,
    updates: updates::Updates,
    completion_run: notifications::CompletionRun,
    demo: bool,
    agents: agents::Agents,
    tasks: tasks::Tasks,
    ultracode: bool,
    ultracode_ready: bool,
    effort_draft: Option<(String, usize)>,
    state: State,
    client: Option<rpc::Client>,
    connected: bool,
    loading_chat: bool,
    pending_session: Option<PathBuf>,
    preview_rx: Option<Receiver<Result<crate::history::Preview, String>>>,
    preview_partial: bool,
    diagnostics_open: bool,
    settings_open: bool,
    settings_view: settings::View,
    bypass_pending: Option<String>,
    history_view: rewind::View,
    history_pending: Option<rewind::Pending>,
    history_drafts: Vec<run_status::WaitingMessage>,
    waiting: std::collections::BTreeMap<PathBuf, Vec<run_status::WaitingMessage>>,
    deliveries: Vec<run_status::Delivery>,
    draft: String,
    recall: composer_history::Recall,
    recall_width: f32,
    draft_images: attachments::DraftImages,
    image_picker_rx: Option<Receiver<Vec<PathBuf>>>,
    new_chat: bool,
    chat_folder_rx: Option<Receiver<Option<PathBuf>>>,
    chat_folder_target: Option<String>,
    paste_requested: std::sync::Arc<std::sync::atomic::AtomicBool>,
    #[cfg(target_os = "macos")]
    _paste_monitor: Option<attachments::PasteMonitor>,
    search: String,
    projects: bool,
    model_picker_open: bool,
    model_anchor: Option<(egui::Rect, egui::LayerId)>,
    model_picker_focus: bool,
    model_query: String,
    model_provider: String,
    show_archived: bool,
    closed_projects: std::collections::BTreeSet<PathBuf>,
    expanded_projects: std::collections::BTreeSet<PathBuf>,
    rename_chat: Option<PathBuf>,
    rename_text: String,
    usage: crate::usage::Usage,
    sessions: Vec<Session>,
    session_rx: Option<Receiver<Vec<Session>>>,
    workspace: Workspace,
    workspace_rx: Option<Receiver<Workspace>>,
    ctx: egui::Context,
    transcript: Transcript,
    outputs: Vec<RecordedFile>,
    pulls: crate::pulls::Pulls,
    output_revision: u64,
    inspector_panel: bool,
    files: files::Browser,
    terminals: terminal::Terminals,
    changes: changes::Changes,
    diff_rx: Option<Receiver<(String, String)>>,
    selected_diff: Option<(String, String)>,
    dialog_text: String,
    questions: questions::View,
    question_designs: Option<questions::designs::Preview>,
    current_dialog: String,
    pending_dialog: Option<String>,
    focus_composer: bool,
    focus_search: bool,
    screenshot: Option<PathBuf>,
    frames: usize,
}

impl Pui {
    pub fn new(cc: &eframe::CreationContext<'_>, options: Options) -> Self {
        theme::install(&cc.egui_ctx);
        let mut prefs: Preferences = cc
            .storage
            .and_then(|s| eframe::get_value(s, "preferences"))
            .unwrap_or_default();
        options.theme.unwrap_or(prefs.theme).apply(&cc.egui_ctx);
        if let Some(theme) = options.theme {
            prefs.theme = theme;
        }
        let session = prefs.startup_session(&options);
        let projects = prefs.projects_view;
        prefs.cwd = options
            .cwd
            .or_else(|| {
                (!prefs.cwd.as_os_str().is_empty() && prefs.cwd.is_dir())
                    .then_some(prefs.cwd.clone())
            })
            .unwrap_or_else(|| std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")));
        prefs.cards.normalize();
        if prefs.agent_concurrency == 0 {
            prefs.agent_concurrency = 4;
        }
        let paste_requested = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
        #[cfg(target_os = "macos")]
        let paste_monitor =
            attachments::PasteMonitor::install(&cc.egui_ctx, paste_requested.clone());
        let mut app = Self {
            sync: sync::WindowSync::new(&cc.egui_ctx, options.demo || cfg!(test)),
            notifications: Default::default(),
            menu: menu::Menu::new(&cc.egui_ctx),
            updates: Default::default(),
            completion_run: Default::default(),
            prefs,
            demo: options.demo,
            agents: Default::default(),
            tasks: tasks::Tasks::new(&cc.egui_ctx, !options.demo && !cfg!(test)),
            ultracode: false,
            ultracode_ready: false,
            effort_draft: None,
            state: State::default(),
            client: None,
            connected: false,
            loading_chat: false,
            pending_session: None,
            preview_rx: None,
            preview_partial: false,
            diagnostics_open: false,
            settings_open: false,
            settings_view: Default::default(),
            bypass_pending: None,
            history_view: Default::default(),
            history_pending: None,
            history_drafts: Vec::new(),
            waiting: Default::default(),
            deliveries: Vec::new(),
            draft: String::new(),
            recall: Default::default(),
            recall_width: 700.0,
            draft_images: Default::default(),
            image_picker_rx: None,
            new_chat: session.is_none() && !options.demo,
            chat_folder_rx: None,
            chat_folder_target: None,
            paste_requested,
            #[cfg(target_os = "macos")]
            _paste_monitor: paste_monitor,
            search: String::new(),
            projects,
            model_picker_open: false,
            model_anchor: None,
            model_picker_focus: false,
            model_query: String::new(),
            model_provider: String::new(),
            show_archived: false,
            closed_projects: Default::default(),
            expanded_projects: Default::default(),
            rename_chat: None,
            rename_text: String::new(),
            usage: if options.demo {
                crate::usage::Usage::demo()
            } else {
                crate::usage::Usage::default()
            },
            sessions: Vec::new(),
            session_rx: None,
            workspace: Workspace::default(),
            workspace_rx: None,
            ctx: cc.egui_ctx.clone(),
            transcript: Transcript::default(),
            outputs: Vec::new(),
            pulls: Default::default(),
            output_revision: u64::MAX,
            inspector_panel: false,
            files: Default::default(),
            terminals: Default::default(),
            changes: Default::default(),
            diff_rx: None,
            selected_diff: None,
            dialog_text: String::new(),
            questions: Default::default(),
            question_designs: options.demo_question_designs.then(Default::default),
            current_dialog: String::new(),
            pending_dialog: None,
            focus_composer: true,
            focus_search: false,
            screenshot: options.screenshot,
            frames: 0,
        };
        app.settings_open = options.settings || options.demo_settings;
        if let Some(page) = options.settings_page.as_deref() {
            app.settings_view.select(page);
        }
        if app.demo {
            app.load_demo();
            if options.demo_approval {
                app.state.busy = true;
                let args = json!({"code":"const results = await Promise.all([\n  tools.read({ path: \"src/app/prompts.rs\" }),\n  tools.bash({ command: \"cargo test --all-targets\", timeout: 120 })\n]);\nfor (const result of results) text(result);"});
                app.state.dialogs.push_back(json!({"type":"extension_ui_request","method":"select","id":"preview-approval","title":format!("Allow codemode?\n\nAuto flagged this action for your review.\n\n{args}\n\nApprove this exact action."),"options":["Allow once","Always allow this exact action (this chat)","Deny"]}));
            }
            if options.demo_questions {
                app.state.busy = true;
                app.state.dialogs.push_back(questions::demo_dialog());
            }
            if options.demo_model_picker {
                app.model_picker_open = true;
                app.ultracode_ready = true;
            }
            if options.demo_agents {
                app.demo_agent_tree();
            }
            if options.demo_task {
                app.demo_task();
            }
            if options.demo_working {
                app.state.busy = true;
                if let Some(last) = app.state.messages.last_mut() {
                    last.images.push(
                        crate::media::Attachment::from_bytes(include_bytes!("../assets/icon.png"))
                            .unwrap(),
                    );
                }
            }
            if options.demo_compacting {
                app.state.compacting = true;
                app.state.session_file = Some("demo-0".into());
                app.waiting.insert(
                    PathBuf::from("demo-0"),
                    vec![run_status::WaitingMessage {
                        text: "Review this after compaction finishes".into(),
                        images: Vec::new(),
                        mode: settings::DeliveryMode::Queue,
                    }],
                );
            }
            if options.demo_theater {
                let images = vec![
                    crate::media::Attachment::from_bytes(include_bytes!("../assets/preview.png"))
                        .unwrap(),
                    crate::media::Attachment::from_bytes(include_bytes!("../assets/icon.png"))
                        .unwrap(),
                ];
                app.transcript.gallery.open(&images, 0);
            }
            if options.demo_terminal {
                app.terminals.toggle(&cc.egui_ctx, &app.prefs.cwd, true);
                app.focus_composer = false;
            }
            if options.demo_files {
                app.files.toggle(&cc.egui_ctx, &app.prefs.cwd);
            }
            if options.demo_changes {
                app.inspector_panel = true;
            }
            if options.demo_notifications {
                app.state.notice = Some("Returned to the selected prompt. Review the draft, then send when ready. Nothing was rerun.".into());
                app.state.add_diagnostic("MCP server unavailable (HTTP 503). The remote service could not be reached. Local chat is still available.".into());
                app.diagnostics_open = true;
            }
            app.transcript.expand_activity = options.demo_expanded;
            if options.demo_collapsed {
                app.prefs.cards.collapsed = Card::ALL.into_iter().collect();
            }
        } else {
            app.restore_agents();
            app.refresh_sessions();
            app.refresh_workspace();
            if let Some(id) = session.as_deref().and_then(|path| app.agent_for_path(path)) {
                // Restore workers through their saved launch policy (including read-only access).
                app.view_agent(&id);
            } else {
                app.connect(&cc.egui_ctx, session);
            }
        }
        app
    }

    fn refresh_sessions(&mut self) {
        if self.session_rx.is_some() || self.demo {
            return;
        }
        let (tx, rx) = mpsc::channel();
        self.session_rx = Some(rx);
        let ctx = self.ctx.clone();
        std::thread::spawn(move || {
            let _ = tx.send(sessions::discover());
            ctx.request_repaint();
        });
    }

    fn refresh_workspace(&mut self) {
        if self.workspace_rx.is_some() || self.demo {
            return;
        }
        let cwd = self.prefs.cwd.clone();
        let (tx, rx) = mpsc::channel();
        self.workspace_rx = Some(rx);
        let ctx = self.ctx.clone();
        std::thread::spawn(move || {
            let _ = tx.send(workspace::inspect(&cwd));
            ctx.request_repaint();
        });
    }

    fn connect(&mut self, ctx: &egui::Context, session: Option<PathBuf>) {
        #[cfg(not(test))]
        if !self.demo && !self.sync.enabled() {
            self.sync = sync::WindowSync::new(ctx, false);
        }
        if let Some(client) = self.client.take() {
            client.retire();
        }
        self.sync.detach_mirror();
        if session.is_some() {
            self.new_chat = false;
        }
        self.recall = Default::default();
        self.completion_run = Default::default();
        self.dialog_text.clear();
        self.questions = Default::default();
        self.current_dialog.clear();
        self.pending_dialog = None;
        self.connected = false;
        self.ultracode = false;
        self.ultracode_ready = false;
        self.loading_chat = true;
        self.pending_session = session.clone();
        self.preview_rx = None;
        self.preview_partial = false;
        let models = std::mem::take(&mut self.state.models);
        self.state = State {
            models,
            ..Default::default()
        };
        self.transcript.clear();
        self.pulls = Default::default();
        if let Some(path) = session.clone() {
            self.state.session_file = Some(path.display().to_string());
            self.state.session_name = self
                .sessions
                .iter()
                .find(|s| s.path == path)
                .map(|s| s.title.clone())
                .unwrap_or_else(|| "Opening chat…".into());
            let (tx, rx) = mpsc::channel();
            self.preview_rx = Some(rx);
            let ctx = ctx.clone();
            std::thread::spawn(move || {
                let _ = tx.send(crate::history::load(&path));
                ctx.request_repaint();
            });
        }
        self.output_revision = u64::MAX;
        if let Some(path) = session.as_ref() {
            if !self.attach_or_claim(path) {
                return;
            }
        }
        #[cfg(not(test))]
        {
            self.client = Some(rpc::Client::start_with(
                &self.prefs.cwd,
                session.as_deref(),
                ctx.clone(),
                rpc::Launch {
                    settings: self.prefs.settings.payload(),
                    ..Default::default()
                },
            ));
        }
        // Interaction tests never spawn a real agent or initialize user extensions.
        #[cfg(test)]
        {
            self.client = Some(rpc::Client::stub());
        }
        self.send(protocol::command("get_state"));
        self.send(protocol::command("get_messages"));
        self.send(protocol::command("get_available_models"));
        self.send(protocol::command("get_available_thinking_levels"));
        self.send(protocol::command("get_session_stats"));
        self.refresh_workspace();
        #[cfg(not(test))]
        if !self.demo {
            self.usage.refresh(&self.ctx, false);
        }
    }

    // Viewing another chat is independent of its run state or IPC registration.
    // Keep the RPC connection (and outstanding approvals/deliveries) parked.
    fn retain_chat_on_switch(&self) -> bool {
        self.has_tree()
            || self.sync_live_owner()
            || (!self.is_mirror()
                && self.client.is_some()
                && (!self.state.ready() || !self.deliveries.is_empty()))
    }

    fn open_chat(&mut self, session: Session) {
        self.tasks.open = false;
        let path = session.path.clone();
        if !self.loading_chat
            && self.connected
            && self
                .state
                .session_file
                .as_deref()
                .is_some_and(|p| std::path::Path::new(p) == path)
        {
            return;
        }
        if let Some(id) = self.agent_for_path(&path) {
            if id != self.agents.active {
                self.view_agent(&id);
                return;
            }
            if self.has_tree() {
                if self.client.is_none() && !self.resume_agent() {
                    self.connect(&self.ctx.clone(), Some(path));
                }
                return;
            }
        }
        if self.retain_chat_on_switch() {
            self.ensure_agent();
            self.publish_live();
            self.park_tree();
            self.prefs.cwd = session.cwd;
            self.connect(&self.ctx.clone(), Some(path));
            self.ensure_agent();
            return;
        }
        if self.pending_session.as_ref() == Some(&path)
            || (!self.loading_chat
                && self.connected
                && self
                    .state
                    .session_file
                    .as_deref()
                    .is_some_and(|p| std::path::Path::new(p) == path))
        {
            return;
        }
        // Superseding a pending switch gets a fresh client and receiver. Late
        // events/preview results from the abandoned session cannot select it again.
        let reuse = !self.loading_chat
            && self.connected
            && self.client.is_some()
            && self.prefs.cwd == session.cwd
            && !self.sync.enabled();
        self.prefs.cwd = session.cwd;
        self.workspace_rx = None;
        if reuse {
            self.loading_chat = true;
            self.pending_session = Some(path.clone());
            self.preview_partial = false;
            self.state.session_name = session.title;
            self.state.session_file = Some(path.display().to_string());
            self.state.messages.clear();
            self.state.cost = 0.0;
            self.state.context_tokens = None;
            self.state.outputs_revision = self.state.outputs_revision.wrapping_add(1);
            self.transcript.clear();
            let (tx, rx) = mpsc::channel();
            self.preview_rx = Some(rx);
            let preview_path = path.clone();
            let ctx = self.ctx.clone();
            std::thread::spawn(move || {
                let _ = tx.send(crate::history::load(&preview_path));
                ctx.request_repaint();
            });
            self.send(json!({"type":"switch_session","sessionPath":path}));
            self.refresh_workspace();
        } else {
            self.connect(&self.ctx.clone(), Some(path));
        }
        self.draft.clear();
        self.focus_composer = true;
        self.draft_images.clear();
        self.pulls = Default::default();
    }

    fn send(&mut self, record: Value) {
        if record["type"] == "abort" {
            self.completion_run.cancel();
        }
        if self.remote_action(sync::Action::Rpc(record.clone())) {
            return;
        }
        if let Some(client) = &mut self.client {
            if let Err(error) = client.send(record) {
                self.state.error = Some(error);
                self.loading_chat = false;
                self.pending_session = None;
                self.preview_rx = None;
                self.connected = false;
            }
        }
    }

    fn poll(&mut self) {
        self.usage.poll();
        self.notifications.poll();
        self.updates.poll();
        self.settings_view.resources.poll();
        self.poll_chat_folder();
        self.poll_tasks();
        self.poll_rpc();
        self.poll_history();
        self.poll_other_agents();
        self.poll_views();
        self.publish_live();
        self.poll_window_sync();
    }
    fn poll_rpc(&mut self) {
        if let Some(preview) = self.preview_rx.as_ref().and_then(|rx| rx.try_recv().ok()) {
            self.preview_rx = None;
            if let Ok(preview) = preview {
                self.preview_partial = preview.partial;
                self.state.messages = preview.messages;
                self.state.outputs_revision = self.state.outputs_revision.wrapping_add(1);
            }
        }
        let events: Vec<_> = self
            .client
            .as_ref()
            .map(|c| c.events.try_iter().collect())
            .unwrap_or_default();
        for event in events {
            match event {
                rpc::Event::History(messages) => {
                    self.preview_rx = None;
                    self.preview_partial = false;
                    self.transcript
                        .replace_history(&self.state.messages, &messages);
                    self.state.messages = messages;
                    if !self.state.messages.is_empty() {
                        self.new_chat = false;
                    }
                    self.state.transcript_revision = self.state.transcript_revision.wrapping_add(1);
                    self.state.prose_revision = self.state.prose_revision.wrapping_add(1);
                    self.state.outputs_revision = self.state.outputs_revision.wrapping_add(1);
                    self.connected = true;
                    self.loading_chat = false;
                    self.pending_session = None;
                }
                rpc::Event::Record(record) => {
                    self.completion_event(&record);
                    self.agent_event(&record);
                    if record["type"] == "extension_ui_request"
                        && record["method"] == "input"
                        && record["title"] == "Pui steering window"
                    {
                        let before = self.deliveries.len();
                        self.deliver_waiting_at_window(true);
                        if self.deliveries.len() > before {
                            self.deliveries.last_mut().unwrap().window =
                                record["id"].as_str().map(str::to_owned);
                        } else {
                            self.send(json!({"type":"extension_ui_response","id":record["id"],"value":"continue"}));
                        }
                        continue;
                    }
                    let history_source = self.agents.active.clone();
                    if self.history_event(&record) {
                        // Fork can open a new owner. Never apply remaining source
                        // records to the newly visible chat.
                        if self.agents.active != history_source {
                            break;
                        }
                        continue;
                    }
                    if self.control_request(&record) {
                        continue;
                    }
                    if record["type"] == "extension_ui_request"
                        && record["method"] == "setStatus"
                        && record["statusKey"] == "pui-ultracode"
                    {
                        if let Ok(value) =
                            serde_json::from_str::<Value>(protocol::string(&record, "statusText"))
                        {
                            self.ultracode_ready = value["ready"] == true;
                            self.ultracode = value["enabled"] == true;
                        }
                        continue;
                    }
                    let kind = protocol::string(&record, "type").to_owned();
                    let command = protocol::string(&record, "command").to_owned();
                    let success = record["success"] == true;
                    if kind == "response" && self.pending_session.is_none() {
                        self.connected = true;
                    }
                    if kind == "response"
                        && !success
                        && matches!(command.as_str(), "get_messages" | "new_session")
                    {
                        self.loading_chat = false;
                        self.pending_session = None;
                        self.preview_rx = None;
                    }
                    if kind == "extension_ui_request"
                        && protocol::string(&record, "method") == "set_editor_text"
                    {
                        self.draft = protocol::string(&record, "text").into();
                        self.focus_composer = true;
                    }
                    if kind == "response" {
                        let mut mail_record = record.clone();
                        mail_record["id"] =
                            format!("{}:{}", self.agents.active, protocol::string(&record, "id"))
                                .into();
                        self.agent_mail_response(&mail_record);
                        self.delivery_response(&record);
                    }
                    if kind == "response" && !success && command == "prompt" {
                        self.agent_failed(protocol::string(&record, "error"));
                    }
                    let cancelled = record["data"]["cancelled"] == true;
                    self.state.apply(record);
                    if kind == "response" && command == "switch_session" {
                        if success && !cancelled {
                            self.send(protocol::command("get_state"));
                            self.send(protocol::command("get_messages"));
                            self.send(protocol::command("get_available_thinking_levels"));
                            self.send(protocol::command("get_session_stats"));
                        } else {
                            self.loading_chat = false;
                            self.pending_session = None;
                            self.preview_rx = None;
                            if cancelled {
                                self.state.notice = Some(
                                    "Pi cancelled the chat switch; showing the previous chat."
                                        .into(),
                                );
                            }
                            self.send(protocol::command("get_state"));
                            self.send(protocol::command("get_messages"));
                        }
                    }
                    if kind == "agent_settled" {
                        if !self.demo {
                            self.usage.refresh(&self.ctx, false);
                        }
                        self.send(protocol::command("get_session_stats"));
                        self.send(protocol::command("get_state"));
                        if !self.agents.background {
                            self.refresh_workspace();
                            self.refresh_sessions();
                        }
                    }
                    if success
                        && matches!(
                            command.as_str(),
                            "new_session" | "set_model" | "set_thinking_level"
                        )
                    {
                        self.send(protocol::command("get_state"));
                        self.send(protocol::command("get_available_thinking_levels"));
                        if command == "new_session" {
                            self.send(protocol::command("get_messages"));
                            self.send(protocol::command("get_session_stats"));
                        }
                    }
                }
                rpc::Event::Error(error) => {
                    self.agent_failed(&error);
                    self.state.error = Some(error);
                }
                rpc::Event::Exited => {
                    self.completion_run = Default::default();
                    if let Some(client) = self.client.take() {
                        client.retire();
                    }
                    self.connected = false;
                    self.loading_chat = false;
                    self.pending_session = None;
                    self.preview_rx = None;
                    self.state.busy = false;
                    self.state.pending = false;
                    self.state.compacting = false;
                    self.state.dialogs.clear();
                    if self.history_pending.take().is_some() {
                        self.state.error = Some("Pi disconnected during history navigation. Outcome unknown; check the chat before retrying. Nothing was resent.".into());
                    }
                    self.agent_event(&json!({"type":"agent_settled"}));
                }
            }
        }
    }
    fn poll_views(&mut self) {
        if let Some(result) = self.session_rx.as_ref().and_then(|rx| rx.try_recv().ok()) {
            self.sessions = result;
            self.session_rx = None;
        }
        if let Some(result) = self.workspace_rx.as_ref().and_then(|rx| rx.try_recv().ok()) {
            self.workspace = result;
            self.workspace_rx = None;
            self.changes.invalidate();
        }
        if let Some(result) = self.diff_rx.as_ref().and_then(|rx| rx.try_recv().ok()) {
            self.selected_diff = Some(result);
            self.diff_rx = None;
        }
        if self.output_revision != self.state.outputs_revision {
            self.outputs = activity::recorded_files(&self.state.messages);
            self.output_revision = self.state.outputs_revision;
        }
    }

    fn new_session(&mut self) {
        self.start_chat_in(self.prefs.cwd.clone());
    }

    fn submit(&mut self, ctx: &egui::Context) {
        self.submit_mode(ctx, self.prefs.settings.delivery);
    }

    fn submit_mode(&mut self, ctx: &egui::Context, mode: settings::DeliveryMode) {
        if self.is_mirror() {
            self.queue_draft_mode(mode);
            return;
        }
        if !self.state.ready() && self.connected {
            self.queue_draft_mode(mode);
            return;
        }
        if (self.draft.trim().is_empty() && self.draft_images.is_empty())
            || (!self.draft_images.is_empty() && !self.draft_images.ready())
            || !self.state.ready()
            || self.demo
            || self.loading_chat
            || self.history_pending.is_some()
            || self.restoring_code()
        {
            return;
        }
        if !self.connected || self.client.is_none() {
            if self.client.is_none() {
                if self.resume_agent() {
                    return;
                }
                self.connect(ctx, self.state.session_file.as_ref().map(PathBuf::from));
            }
            self.state.notice =
                Some("Connecting to Pi. Your draft is preserved; send when connected.".into());
            return;
        }
        let message = run_status::WaitingMessage {
            text: self.draft.trim().to_owned(),
            images: self.draft_images.images(),
            mode,
        };
        if !self.deliver_message(message, "prompt") {
            return;
        }
        self.draft_images.clear();
        self.draft.clear();
        self.recall = Default::default();
        self.focus_composer = true;
    }

    fn stop(&mut self) {
        if self.tasks.open && self.tasks.running() {
            self.tasks.stop();
            return;
        }
        if self.is_mirror() {
            self.remote_action(sync::Action::Rpc(protocol::command("clear_queue")));
            self.remote_action(sync::Action::Rpc(protocol::command("abort")));
            return;
        }
        if let Some(path) = self.state.session_file.as_ref().map(PathBuf::from) {
            self.waiting.remove(&path);
        }
        self.send(protocol::command("clear_queue"));
        self.send(protocol::command("abort"));
    }

    fn welcome(&mut self, ui: &mut egui::Ui) {
        let colors = crate::theme::palette(ui.ctx());
        ui.add_space((ui.available_height() * 0.16).clamp(30.0, 110.0));
        ui.vertical_centered(|ui| {
            let (rect, _) = ui.allocate_exact_size(Vec2::splat(62.0), Sense::hover());
            ui.painter().rect_filled(rect, 17, colors.accent_bg);
            ui.painter().text(
                rect.center(),
                egui::Align2::CENTER_CENTER,
                "π",
                FontId::proportional(45.0),
                colors.accent,
            );
            ui.add_space(19.0);
            ui.label(
                RichText::new("A little space for big ideas.")
                    .size(25.0)
                    .strong(),
            );
            ui.add_space(9.0);
            ui.label(
                RichText::new("Your Pi agent, with room to think.")
                    .size(14.0)
                    .color(colors.muted),
            );
            ui.add_space(5.0);
            if self.new_chat {
                self.new_chat_project(ui);
            } else {
                ui.label(theme::small(
                    ui,
                    format!("Working in {}", self.prefs.cwd.display()),
                ));
            }
        });
        ui.add_space(35.0);
        for (icon, title, prompt) in [
            (
                Icon::Search,
                "Understand this project",
                "Explore this project and explain its architecture. Don't change any files.",
            ),
            (
                Icon::Terminal,
                "Find the next step",
                "Inspect this project and suggest three useful next steps. Don't change any files.",
            ),
            (
                Icon::Spark,
                "Build something new",
                "I'd like to build a new feature. First help me understand where to start.",
            ),
        ] {
            let response = egui::Frame::new()
                .fill(colors.sidebar)
                .stroke(Stroke::new(1.0, colors.border))
                .corner_radius(10)
                .inner_margin(14)
                .show(ui, |ui| {
                    ui.horizontal(|ui| {
                        icons::inline(ui, icon, colors.accent);
                        ui.label(RichText::new(title).size(12.0));
                        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                            ui.label(RichText::new("↗").color(colors.dim));
                        });
                    });
                    ui.set_min_width((ui.available_width() - 2.0).max(0.0));
                })
                .response
                .interact(Sense::click())
                .on_hover_cursor(egui::CursorIcon::PointingHand);
            if response.clicked() {
                self.draft = prompt.into();
                self.focus_composer = true;
            }
            ui.add_space(4.0);
        }
    }

    fn composer(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        let colors = crate::theme::palette(ui.ctx());
        if let Some(error) = self.state.error.clone() {
            egui::Frame::new()
                .fill(colors.error_bg)
                .corner_radius(7)
                .inner_margin(10)
                .show(ui, |ui| {
                    ui.horizontal_wrapped(|ui| {
                        ui.label(RichText::new(&error).size(11.0).color(colors.red));
                        if ui.small_button("Dismiss").clicked() {
                            self.state.error = None;
                        }
                    });
                });
            ui.add_space(8.0);
        }
        // Optional extension/MCP diagnostics live in a fixed top-bar popover.
        // Inserting multiline notifications here moves the viewport and composer.
        if self.demo {
            ui.horizontal(|ui| {
                icons::inline(ui, Icon::Spark, colors.accent);
                ui.label(theme::small(ui, "Design preview · example conversation"));
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    if ui.small_button("Start a real session").clicked() {
                        self.new_session();
                    }
                });
            });
            ui.add_space(7.0);
        }
        self.history_draft_recovery(ui);
        self.recover_shared_draft(ui);
        self.waiting_messages(ui);
        self.agent_question(ui);
        let settings_before = self.prefs.settings.clone();
        self.permission_confirmation(ui);
        self.propagate_settings(&settings_before);
        let mut submit = false;
        let mut alternate = false;
        egui::Frame::new()
            .fill(colors.sidebar)
            .stroke(Stroke::new(1.0, colors.border))
            .corner_radius(18)
            .inner_margin(egui::Margin {
                left: 14,
                right: 10,
                top: 12,
                bottom: 7,
            })
            .show(ui, |ui| {
                ui.spacing_mut().item_spacing.y = 4.0;
                self.draft_images.show(ui);
                self.recall_width = ui.available_width();
                self.recall_before_edit(ctx);
                let response = ui.add(
                    egui::TextEdit::multiline(&mut self.draft)
                        .id(egui::Id::new("composer"))
                        .hint_text(
                            RichText::new(if self.demo {
                                "Start a real session to talk to Pi…"
                            } else {
                                "Message Pi… (paste or drop images)"
                            })
                            .color(colors.dim),
                        )
                        .font(FontId::proportional(14.0))
                        .frame(false)
                        .desired_width(f32::INFINITY)
                        .desired_rows(1)
                        .return_key(egui::KeyboardShortcut::new(
                            egui::Modifiers::SHIFT,
                            egui::Key::Enter,
                        )),
                );
                if self.focus_composer {
                    response.request_focus();
                    self.focus_composer = false;
                }
                if response.has_focus()
                    && ctx.input(|i| i.key_pressed(egui::Key::Enter) && !i.modifiers.shift)
                {
                    submit = true;
                    alternate = ctx.input(|i| i.modifiers.command);
                }
                ui.add_space(4.0);
                ui.horizontal(|ui| {
                    ui.set_height(28.0);
                    ui.spacing_mut().button_padding = egui::vec2(6.0, 5.0);
                    ui.spacing_mut().item_spacing.x = 4.0;
                    let attach =
                        icons::button(ui, Icon::Plus, "Attach images · right-click for clipboard");
                    if attach.clicked() {
                        self.pick_images();
                    }
                    attach.context_menu(|ui| {
                        if ui.button("Paste image from clipboard").clicked() {
                            self.draft_images.clipboard(ctx);
                            ui.close();
                        }
                    });

                    let (rect, _) = ui.allocate_exact_size(Vec2::new(22.0, 28.0), Sense::hover());
                    icons::draw(ui, rect, Icon::Spark, colors.accent);
                    self.model_button(ui);
                    self.permission_button(ui);
                    if icons::button(ui, Icon::Sliders, "Settings").clicked() {
                        self.settings_open = !self.settings_open;
                    }
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        let busy = !self.state.ready();
                        let enabled = !self.loading_chat
                            && self.history_pending.is_none()
                            && !self.restoring_code()
                            && (!self.draft.trim().is_empty() || self.draft_images.ready())
                            && (self.draft_images.is_empty() || self.draft_images.ready())
                            && !self.demo;
                        let (rect, response) =
                            ui.allocate_exact_size(Vec2::splat(28.0), Sense::click());
                        let response = if enabled {
                            response.on_hover_cursor(egui::CursorIcon::PointingHand)
                        } else {
                            response
                        };
                        ui.painter().rect_filled(
                            rect,
                            9,
                            if enabled {
                                colors.accent
                            } else {
                                colors.surface
                            },
                        );
                        icons::draw(
                            ui,
                            rect,
                            Icon::ArrowUp,
                            if enabled { colors.bg } else { colors.dim },
                        );
                        response.widget_info(|| {
                            egui::WidgetInfo::labeled(
                                egui::WidgetType::Button,
                                enabled,
                                "Send message",
                            )
                        });
                        if response
                            .on_hover_text(if busy {
                                if self.prefs.settings.delivery == settings::DeliveryMode::Steer {
                                    "Steer · Enter · ⌘Enter to queue"
                                } else {
                                    "Queue · Enter · ⌘Enter to steer"
                                }
                            } else {
                                "Send · Enter"
                            })
                            .clicked()
                            && enabled
                        {
                            submit = true;
                        }
                        if busy {
                            let stop = icons::button(
                                ui,
                                Icon::Stop,
                                "Stop · Escape (also clears waiting messages)",
                            );
                            stop.widget_info(|| {
                                egui::WidgetInfo::labeled(
                                    egui::WidgetType::Button,
                                    true,
                                    "Stop agent",
                                )
                            });
                            if stop.clicked() {
                                self.stop();
                            }
                            if self.has_tree() {
                                stop.context_menu(|ui| {
                                    if ui.button("Stop tree").clicked() {
                                        let active = self.agents.active.clone();
                                        self.stop_agent_tree(&active);
                                        ui.close();
                                    }
                                });
                            }
                        }
                    });
                });
            });
        ui.add_space(3.0);
        ui.vertical_centered(|ui| {
            ui.label(
                RichText::new(format!(
                    "{} · Enter: {} · ⌘Enter: {} · Shift+Enter: new line",
                    self.prefs.settings.permission.label(),
                    self.prefs.settings.delivery.label(),
                    self.prefs.settings.delivery.opposite().label()
                ))
                .size(10.0)
                .color(colors.dim),
            );
        });
        if submit {
            if alternate {
                self.submit_mode(ctx, self.prefs.settings.delivery.opposite());
            } else {
                self.submit(ctx);
            }
        }
    }

    fn dialogs(&mut self, ctx: &egui::Context) {
        let mut open = true;
        if let Some((path, diff)) = &self.selected_diff {
            egui::Window::new(path)
                .id(egui::Id::new("diff-viewer"))
                .open(&mut open)
                .default_size([850.0, 560.0])
                .show(ctx, |ui| {
                    ui.label(theme::small(
                        ui,
                        "Read-only recorded change or working-tree diff · no files are modified",
                    ));
                    ui.separator();
                    egui::ScrollArea::both().show(ui, |ui| markdown::diff(ui, diff));
                });
        }
        if !open {
            self.selected_diff = None;
        }
    }

    fn load_demo(&mut self) {
        self.prefs.cards = CardLayout::default();
        self.prefs.hide_inspector = false;
        self.prefs.hide_sidebar = false;
        self.state.model = "gpt-6.1-sol".into();
        self.state.model_name = "GPT-6.1 Sol".into();
        self.state.provider = "litellm".into();
        self.state.models = vec![
            crate::protocol::Model {
                id: "gpt-6.1-sol".into(),
                name: "GPT-6.1 Sol".into(),
                provider: "litellm".into(),
                context_window: 600000,
                input_cost: 2.0,
                output_cost: 10.0,
            },
            crate::protocol::Model {
                id: "claude-opus-5-5".into(),
                name: "Claude Opus 5.5".into(),
                provider: "litellm".into(),
                context_window: 200000,
                input_cost: 5.0,
                output_cost: 25.0,
            },
        ];
        self.state.levels = ["low", "medium", "high", "xhigh"]
            .into_iter()
            .map(str::to_owned)
            .collect();
        self.state.thinking = "high".into();
        self.state.session_name =
            "make sure that the skill is up to date and push it into skillshop".into();
        self.state.cost = 15.60;
        self.state.context_tokens = Some(478700.0);
        self.state.context_window = 600000.0;
        self.state.tokens =
            json!({"input":822000,"output":103000,"cacheRead":73000000,"cacheWrite":1900000});
        for index in 0..22 {
            self.state.messages.push(Message::from_json(&json!({"role":"user","content":format!("Example sent message {}",index+1),"timestamp":1731050000000_i64+index*60000})));
            self.state.messages.push(Message::from_json(&json!({"role":"assistant","model":"gpt-6.1-sol","content":[{"type":"text","text":"Earlier conversation — click a mark in the rail to jump to the corresponding sent message."}],"timestamp":1731050005000_i64+index*60000})));
        }
        self.state.messages.push(Message::from_json(&json!({"role":"assistant","model":"gpt-6.1-sol","timestamp":1731055560000_i64,"usage":{"cost":{"total":2.13},"cacheRead":100},"content":[{"type":"text","text":"3. **Wrapped publish button**\n   - Fixed the header layout and prevented button-label wrapping. Tested desktop and mobile widths.\n\n4. **Share for editing**\n   - Moved it into the artifact header, beside the publish action.\n   - Supports all signed-in organization members or multiple selected accounts.\n\n**Validation:** frontend tests, Pi tests, PostgreSQL race suite, and browser checks in both preview modes pass.\n\n**Still local—not committed, pushed, or deployed.** Production will remain unchanged until the release blocker is resolved."}]})));
        self.state.messages.push(Message::from_json(&json!({"role":"user","timestamp":1731056460000_i64,"content":"https://github.com/example/agent-harbor/pull/56 is still failing\nso push everything here and lets merge it"})));
        let mut tools = Vec::new();
        for (index,(name,args)) in [
            ("enter_worktree",json!({"name":"cli-production-login-default"})),
            ("bash",json!({"command":"git status --short; git log -3 --oneline; gh pr view 56"})),
            ("read",json!({"path":".github/workflows/ci.yml"})),
            ("read",json!({"path":".github/workflows/build-image.yml"})),
            ("bash",json!({"command":"gh run view 42569759760 --log-failed"})),
            ("read",json!({"path":"deploy/ci/install-trivy.py"})),
            ("read",json!({"path":"deploy/ci/install_trivy_test.py"})),
            ("bash",json!({"command":"python3 -m unittest discover"})),
            ("edit",json!({"path":"pi/skill/SKILL.md","oldText":"version: 1.0.0","newText":"version: 1.1.0\n# Updated publishing workflow"})),
        ].into_iter().enumerate(){
            let mut tool=Tool{id:format!("demo-{index}"),name:name.into(),finished:true,output:if name=="bash"{"Example command output — preview data, no command was executed.".into()}else{"Example file contents.".into()},..Default::default()};
            tool.set_args(&args);tool.duration_ms=Some(if name=="bash"{44}else{1});tools.push(tool);
        }
        let mut first = Message::from_json(
            &json!({"role":"assistant","model":"gpt-6.1-sol","thinkingLevel":"high","timestamp":1731056580000_i64,"content":[{"type":"text","text":"I’ll add the artifact changes to PR #56, inspect its current failures, and fix the blockers. I’ll merge only after all required checks pass—without bypassing protections."}]}),
        );
        first.blocks.extend(
            tools
                .iter()
                .map(|t| crate::protocol::Block::Tool(t.id.clone())),
        );
        first.tools = tools;
        self.state.messages.push(first);
        {
            use base64::Engine as _;
            let data = base64::engine::general_purpose::STANDARD
                .encode(include_bytes!("../assets/icon.png"));
            self.state.messages.push(Message::from_json(&json!({"role":"user","timestamp":1731056500000_i64,"content":[{"type":"image","data":data,"mimeType":"image/png"},{"type":"text","text":"Keep the interface quiet and focused."}]})));
        }
        let mut last = Message::from_json(
            &json!({"role":"assistant","model":"gpt-6.1-sol","timestamp":1731056580000_i64,"usage":{"cost":{"total":0.55},"cacheRead":100},"content":[{"type":"text","text":"The latest failures all stop at the same point: Trivy’s pinned GitHub archive download returns HTTP 504, before scanning begins. The builds passed that stage.\n\nI’m adding a fallback to the exact upstream release-asset API, with the same SHA-256 verification. I’ll also isolate Go cache paths to avoid collisions between concurrent self-hosted jobs. The artifact changes are now combined with the installer/login work."}]}),
        );
        for (i, path) in [
            "deploy/ci/install_trivy_test.py",
            "deploy/ci/install-trivy.py",
            "architecture.md",
            "README.md",
            "commands.go",
            "commands.md",
            "product-spec.md",
            "setup.md",
            "cli.go",
            "clilist_test.go",
            "editor_preview_test.go",
        ]
        .iter()
        .enumerate()
        {
            let mut tool = Tool {
                id: format!("demo-final-{i}"),
                name: "edit".into(),
                finished: true,
                failed: i < 2,
                duration_ms: Some(13),
                output: if i < 2 {
                    "Example failure: network request returned HTTP 504.".into()
                } else {
                    "Recorded replacement applied (preview only).".into()
                },
                ..Default::default()
            };
            tool.set_args(&json!({"path":path,"oldText":"previous implementation","newText":"updated implementation\n# verified fallback"}));
            last.blocks
                .push(crate::protocol::Block::Tool(tool.id.clone()));
            last.tools.push(tool);
        }
        self.state.messages.push(last);
        let mut nested = Tool {
            id: "demo-script".into(),
            name: "codemode".into(),
            finished: true,
            duration_ms: Some(78),
            output: "Read the configuration and checked the workspace.".into(),
            ..Default::default()
        };
        nested.set_args(&json!({"code":"const result = await tools.read({ path: \"Cargo.toml\" });\ntext(result);"}));
        nested.set_nested(&json!({"nestedCalls":{"complete":true,"calls":[{"id":"demo-script/1","name":"read","arguments":{"path":"Cargo.toml"},"status":"ok","durationMs":2},{"id":"demo-script/2","name":"bash","arguments":{"command":"git status --short"},"status":"ok","durationMs":44}]}}));
        if let Some(message) = self.state.messages.last_mut() {
            message
                .blocks
                .push(protocol::Block::Tool(nested.id.clone()));
            message.tools.push(nested);
        }
        self.state.transcript_revision += 1;
        self.state.outputs_revision += 1;
        let now = std::time::SystemTime::now();
        self.sessions = [
            "make sure that the skill is up to date…",
            "lets make a UI for Pi harness",
            "install unity skill",
            "we have more models available",
            "New table",
            "Read-only reviewer for pending changes",
            "Prepare/test LOCAL ONLY findings",
        ]
        .iter()
        .enumerate()
        .map(|(i, title)| Session {
            path: PathBuf::from(format!("demo-{i}")),
            cwd: self.prefs.cwd.clone(),
            project: PathBuf::from(match i {
                0 => "/demo/remote-control-agents",
                1 => "/demo/pui",
                2 | 3 => "/demo/snake",
                _ => "/demo/AlteryraMVP",
            }),
            title: (*title).into(),
            modified: now - Duration::from_secs(i as u64 * 3900),
        })
        .collect();
        self.workspace = Workspace {
            root: self.prefs.cwd.clone(),
            branch: "ui/share-copy-icon".into(),
            is_git: true,
            ..Default::default()
        };
    }
}

impl eframe::App for Pui {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.prefs.theme.apply(ctx);
        self.frames += 1;
        self.poll();
        self.menu_commands(ctx);
        if self.screenshot.is_none() {
            self.agent_close_guard(ctx);
        }
        let terminal_input = !self.settings_open && self.terminals.focused(ctx);
        if !self.settings_open && !self.tasks.open && !terminal_input {
            self.image_inputs(ctx);
        }
        let theater = self.transcript.gallery.is_open() || self.draft_images.gallery.is_open();
        if theater {
            self.model_picker_open = false;
            self.transcript.gallery.viewer(ctx);
            self.draft_images.gallery.viewer(ctx);
        }
        if !self.demo {
            self.deliver_waiting();
            self.publish_live();
        }
        self.pulls.update(
            ctx,
            &self.state.messages,
            self.state.prose_revision,
            self.demo,
        );
        if ctx.input_mut(|i| i.consume_key(egui::Modifiers::COMMAND, egui::Key::Comma)) {
            self.settings_open = !self.settings_open;
            self.model_picker_open = false;
        }
        if !self.settings_open
            && (!terminal_input || cfg!(target_os = "macos"))
            && ctx.input_mut(|i| i.consume_key(egui::Modifiers::COMMAND, egui::Key::B))
        {
            self.prefs.hide_sidebar = !self.prefs.hide_sidebar;
        }
        if !self.settings_open
            && (!terminal_input || cfg!(target_os = "macos"))
            && ctx.input_mut(|i| i.consume_key(egui::Modifiers::COMMAND, egui::Key::I))
        {
            self.files.close();
            self.inspector_panel = false;
            self.prefs.hide_inspector = !self.prefs.hide_inspector;
        }
        if !self.settings_open
            && (!terminal_input || cfg!(target_os = "macos"))
            && ctx.input_mut(|i| i.consume_key(egui::Modifiers::COMMAND, egui::Key::K))
        {
            self.prefs.hide_sidebar = false;
            self.focus_search = true;
            self.focus_composer = false;
        }
        if !self.settings_open
            && (!terminal_input || cfg!(target_os = "macos"))
            && ctx.input_mut(|i| i.consume_key(egui::Modifiers::COMMAND, egui::Key::N))
        {
            self.new_session();
        }
        if !self.settings_open
            && !terminal_input
            && !self.diagnostics_open
            && self.state.dialogs.is_empty()
            && ctx.input(|i| i.key_pressed(egui::Key::Escape))
            && !self.transcript.gallery.is_open()
            && !self.draft_images.gallery.is_open()
            && !theater
            && ((!self.tasks.open && !self.state.ready())
                || (self.tasks.open && self.tasks.running()))
        {
            self.stop();
        }
        if let Some(preview) = self.question_designs.as_mut() {
            preview.show(ctx);
        } else if self.settings_open {
            self.model_picker_open = false;
            self.session_settings(ctx);
        } else {
            if !self.prefs.hide_sidebar {
                self.sidebar(ctx);
            }
            self.top_bar(ctx);
            let terminal_expanded =
                self.terminals.is_open() && self.terminals.show(ctx, &self.prefs.cwd, self.demo);
            if self.terminals.focused(ctx) {
                self.focus_composer = false;
            }
            let central = if terminal_expanded {
                true
            } else if self.files.is_open() {
                self.files.show(ctx, &self.prefs.cwd)
            } else if self.inspector_panel {
                self.changes.show(
                    ctx,
                    &self.prefs.cwd,
                    self.state.session_file.as_deref().unwrap_or("new"),
                    &self.workspace,
                    &self.outputs,
                    self.workspace_rx.is_some(),
                )
            } else {
                if !self.prefs.hide_inspector && ctx.screen_rect().width() > 1110.0 {
                    self.inspector(ctx);
                }
                false
            };
            if self.changes.take_refresh() {
                self.refresh_workspace();
            }
            if !central {
                if self.tasks.open {
                    self.task_panel(ctx);
                } else {
                    self.conversation(ctx);
                }
            }
            self.dialogs(ctx);
            self.model_picker(ctx);
            self.history_confirmation(ctx);
        }
        if !theater {
            self.transcript.gallery.viewer(ctx);
            self.draft_images.gallery.viewer(ctx);
        }
        self.task_watch_source();
        self.menu_windows(ctx);
        self.refresh_menu(ctx);
        if self.screenshot.is_some() && self.frames == 25 {
            ctx.send_viewport_cmd(egui::ViewportCommand::Screenshot(egui::UserData::default()));
        }
        if let Some(path) = &self.screenshot {
            for event in ctx.input(|i| i.events.clone()) {
                if let egui::Event::Screenshot { image, .. } = event {
                    let result = image::save_buffer(
                        path,
                        image.as_raw(),
                        image.width() as u32,
                        image.height() as u32,
                        image::ColorType::Rgba8,
                    );
                    if let Err(error) = result {
                        eprintln!("Screenshot failed: {error}");
                    }
                    ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                }
            }
            ctx.request_repaint_after(Duration::from_millis(100));
        }
    }
    fn save(&mut self, storage: &mut dyn eframe::Storage) {
        if !self.demo && self.screenshot.is_none() {
            self.prefs.agent_tree = self.agent_records();
            self.prefs.projects_view = self.projects;
            self.prefs.last_session = self.pending_session.clone().or_else(|| {
                (!self.new_chat)
                    .then(|| self.state.session_file.as_ref().map(PathBuf::from))
                    .flatten()
            });
            eframe::set_value(storage, "preferences", &self.prefs);
        }
    }
    fn auto_save_interval(&self) -> Duration {
        Duration::from_secs(300)
    }
    fn on_exit(&mut self, _gl: Option<&eframe::glow::Context>) {
        self.terminals.shutdown();
        self.tasks.shutdown();
        self.shutdown_agents();
        self.agents = Default::default();
    }
    fn clear_color(&self, _visuals: &egui::Visuals) -> [f32; 4] {
        theme::palette(&self.ctx).bg.to_normalized_gamma_f32()
    }
}

fn thinking_label(level: &str) -> String {
    if level == "xhigh" {
        return "Extra high".into();
    }
    let mut chars = level.chars();
    match chars.next() {
        Some(c) => format!("{}{}", c.to_uppercase(), chars.as_str()),
        None => "Thinking".into(),
    }
}

fn dot(ui: &mut egui::Ui, color: Color32) {
    let (rect, _) = ui.allocate_exact_size(Vec2::new(8.0, 14.0), Sense::hover());
    ui.painter().circle_filled(rect.center(), 3.0, color);
}
