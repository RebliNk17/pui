mod about;
mod agents;
mod approval_view;
mod attachments;
mod attention;
mod auth_prompt;
mod automation;
mod catalog;
mod changes;
mod chat_find;
mod completions;
mod composer_hints;
mod composer_history;
pub(crate) mod composer_text;
mod composer_toolbar;
mod confirmations;
pub(crate) mod context_capture;
mod deletion;
mod diagnostics;
mod dictation;
mod diff_viewer;
mod files;
mod kanban;
mod menu;
mod model_cache;
mod model_picker;
#[cfg(test)]
mod native_escape_tests;
mod navigation;
mod new_chat;
mod notifications;
mod panel_resize;
mod permission_picker;
mod permission_worker;
#[cfg(test)]
mod persistence_tests;
mod pi_folder;
mod prompt_ui;
#[cfg(test)]
mod prompt_ui_tests;
mod prompts;
mod questions;
mod remote_host;
mod resource_reload;
mod restart;
mod rewind;
mod run_status;
mod search;
mod session_layout;
mod session_titles;
mod settings;
mod side_chat;
mod sidebar;
mod sidebar_order;
mod sidebar_tree;
mod sidebar_view;
mod sync;
mod tasks;
mod terminal;
mod updates;
pub(crate) use updates::install_staged_update;
mod views;
mod webview;

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
    color_styles: theme::Styles,
    agent_concurrency: usize,
    agent_tree: Vec<agents::AgentRecord>,
    hide_sidebar: bool,
    hide_inspector: bool,
    /// Set by "Don't ask again" on the Bypass confirmation; reset in Settings → Permissions.
    skip_bypass_confirmation: bool,
    cards: CardLayout,
    chats: std::collections::BTreeMap<PathBuf, sidebar::ChatPrefs>,
    projects: Vec<PathBuf>,
    projects_view: bool,
    sidebar_view: sidebar_view::Options,
    workspace_layouts: Vec<session_layout::Saved>,
    table_headers: files::TableHeaders,
    diff_wrap: bool,
    transcript_view: crate::transcript::View,
    closed_projects: std::collections::BTreeSet<PathBuf>,
    expanded_projects: std::collections::BTreeSet<PathBuf>,
    session_order: sidebar_order::Options,
    closed_threads: std::collections::BTreeSet<String>,
    last_session: Option<PathBuf>,
    model_catalogs: Vec<model_cache::Catalog>,
    hidden_models: std::collections::BTreeSet<String>,
    restart_checkpoints: Vec<restart::Checkpoint>,
    /// Acknowledges the synchronous pre-update recovery snapshot after normal save.
    update_recovery_id: Option<String>,
    usage_cache: Option<crate::usage::CachedUsage>,
    /// Models from providers registered by Pui's Pi extension (e.g.
    /// Antigravity accounts), learned from Pi and kept app-wide so Settings
    /// never depends on a running chat.
    extension_models: Vec<protocol::Model>,
    /// Folder to move into Pui's own Pi folder at the next launch.
    pi_migration_pending: Option<PathBuf>,
    remote: crate::remote::RemoteConfig,
    /// "Not now" on the migration offer; Settings → General can reopen it.
    pi_migration_dismissed: bool,
    /// Give Pi provider API keys from environment variables (off by default).
    use_environment_keys: bool,
}

impl Preferences {
    /// Bypass is confirmed per chat and is never a default for new chats or
    /// workers. Existing chats that inherited it keep it explicitly.
    fn normalize_default_permission(&mut self) {
        if self.settings.permission != settings::PermissionMode::Bypass {
            return;
        }
        for record in &mut self.agent_tree {
            if record.permission.is_none() && record.access != "read-only" {
                record.permission = Some(settings::PermissionMode::Bypass);
            }
        }
        self.settings.permission = settings::PermissionMode::Auto;
    }
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

#[derive(Clone, Debug)]
pub(super) struct CloudSessionMeta {
    pub(super) host: crate::remote::CloudHostConfig,
    pub(super) status: String,
    pub(super) busy: bool,
    pub(super) has_pending_dialog: bool,
    pub(super) cwd: String,
    pub(super) repo_name: String,
    pub(super) branch: Option<String>,
}

pub struct Pui {
    prefs: Preferences,
    sync: sync::WindowSync,
    notifications: notifications::Notifications,
    menu: menu::Menu,
    navigation: navigation::Navigation,
    updates: updates::Updates,
    update_install_requested: bool,
    update_idle: updates::Idle,
    update_recovery: Option<restart::Recovery>,
    /// App-level model catalog and Pi defaults; never tied to a chat.
    catalog: catalog::AppCatalog,
    pi_folder: pi_folder::PiFolder,
    completion_run: notifications::CompletionRun,
    attention: attention::Attention,
    attention_observed: attention::Observed,
    attention_dock: attention::DockWindow,
    demo: bool,
    agents: agents::Agents,
    side_chat: side_chat::SideChat,
    restart_pending: Vec<restart::Checkpoint>,
    tasks: tasks::Tasks,
    kanban: kanban::Board,
    automation: automation::Automation,
    deletion: deletion::View,
    ultracode: bool,
    ultracode_ready: bool,
    resource_reload: resource_reload::Reload,
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
    worker_upgrade_pending: Option<(String, settings::PermissionMode)>,
    history_view: rewind::View,
    history_pending: Option<rewind::Pending>,
    history_drafts: Vec<run_status::WaitingMessage>,
    waiting: std::collections::BTreeMap<PathBuf, Vec<run_status::WaitingMessage>>,
    deliveries: Vec<run_status::Delivery>,
    interrupt: Option<run_status::Interrupt>,
    draft: String,
    dictation: dictation::Dictation,
    context_chips: context_capture::Draft,
    recall: composer_history::Recall,
    composer_hints: composer_hints::Hints,
    demo_composer_hints: bool,
    recall_width: f32,
    completions: completions::View,
    draft_images: attachments::DraftImages,
    image_picker_rx: Option<Receiver<Vec<PathBuf>>>,
    new_chat: bool,
    chat_folder_rx: Option<Receiver<Option<PathBuf>>>,
    chat_folder_target: Option<String>,
    project_picker: new_chat::project_picker::Picker,
    paste_requested: std::sync::Arc<std::sync::atomic::AtomicBool>,
    #[cfg(target_os = "macos")]
    _paste_monitor: Option<attachments::PasteMonitor>,
    search: String,
    search_open: bool,
    spotlight: search::Search,
    projects: bool,
    model_picker_open: bool,
    effort_picker_open: bool,
    effort_anchor: Option<(egui::Rect, egui::LayerId)>,
    effort_picker_focus: bool,
    model_anchor: Option<(egui::Rect, egui::LayerId)>,
    model_picker_focus: bool,
    model_query: String,
    model_provider: String,
    show_archived: bool,
    show_hidden: bool,
    closed_projects: std::collections::BTreeSet<PathBuf>,
    expanded_projects: std::collections::BTreeSet<PathBuf>,
    sidebar_cache: sidebar_tree::ListingCache,
    sidebar_running: sidebar_order::RunningOrder,
    section_view: sidebar::sections::View,
    rename_chat: Option<PathBuf>,
    rename_text: String,
    usage: crate::usage::Usage,
    dismissed_usage_warning: Option<String>,
    dismissed_auth_error: Option<String>,
    sessions: Vec<Session>,
    local_sessions: Vec<Session>,
    cloud_sessions: Vec<Session>,
    cloud_session_meta: std::collections::HashMap<PathBuf, CloudSessionMeta>,
    cloud_sessions_rx: Option<
        Receiver<(
            Vec<Session>,
            std::collections::HashMap<PathBuf, CloudSessionMeta>,
        )>,
    >,
    cloud_sessions_last_poll: Option<std::time::Instant>,
    session_rx: Option<Receiver<Vec<Session>>>,
    session_watch: Option<sessions::Watch>,
    workspace: Workspace,
    workspace_rx: Option<workspace::read_only::Request<Workspace>>,
    workspace_retry: bool,
    ctx: egui::Context,
    transcript: Transcript,
    outputs: Vec<RecordedFile>,
    pulls: crate::pulls::Pulls,
    output_revision: u64,
    inspector_panel: bool,
    files: files::Browser,
    preview_opens: Vec<String>,
    session_layouts: session_layout::SessionLayouts,
    terminals: terminal::Terminals,
    changes: changes::Changes,
    diff_rx: Option<workspace::read_only::Request<changes::git_files::Loaded>>,
    dialog_text: String,
    questions: questions::View,
    question_designs: Option<questions::designs::Preview>,
    current_dialog: String,
    pending_dialog: Option<String>,
    focus_composer: bool,
    focus_search: bool,
    chat_find: chat_find::ChatFind,
    screenshot: Option<PathBuf>,
    frames: usize,
    remote: crate::remote::RemoteServer,
    pending_origins: std::collections::BTreeMap<String, crate::remote::SessionOrigin>,
    remote_snapshot_tick: u64,
}

impl Pui {
    pub fn new(cc: &eframe::CreationContext<'_>, options: Options) -> Self {
        theme::install(&cc.egui_ctx);
        let (recovery, recovery_error) = if options.demo || cfg!(test) {
            (None, None)
        } else {
            match restart::Recovery::location()
                .map(|path| restart::Recovery::load(&path, cc.storage))
                .transpose()
            {
                Ok(recovery) => (recovery.flatten(), None),
                Err(error) => (None, Some(error)),
            }
        };
        let storage = recovery
            .as_ref()
            .map(|s| s as &dyn eframe::Storage)
            .or(cc.storage);
        let mut prefs: Preferences = storage
            .and_then(|s| eframe::get_value(s, "preferences"))
            .unwrap_or_default();
        prefs.settings.normalize_update_preferences();
        prefs.normalize_default_permission();
        // Before any Pi process or session discovery: adopt or offer Pui's Pi folder.
        let pi_folder = if options.demo || cfg!(test) {
            pi_folder::PiFolder::default()
        } else {
            prefs.setup_pi_folder()
        };
        crate::runtime::set_use_environment_keys(prefs.use_environment_keys);
        diff_viewer::wrap_preference::init(&cc.egui_ctx, prefs.diff_wrap);
        prefs
            .color_styles
            .apply(&cc.egui_ctx, options.theme.unwrap_or(prefs.theme));
        if let Some(theme) = options.theme {
            prefs.theme = theme;
        }
        let session = prefs.startup_session(&options);
        let restore_composers = options.cwd.is_none() && options.session.is_none() && !options.demo;
        let restore_blank = session.is_none() && options.cwd.is_none() && !options.demo;
        let projects = prefs.projects_view;
        let closed_projects = prefs.closed_projects.clone();
        let expanded_projects = prefs.expanded_projects.clone();
        let usage_cache = prefs.usage_cache.clone();
        let remote = {
            let mut remote_cfg = prefs.remote.clone();
            if options.demo || cfg!(test) {
                remote_cfg.enabled = false;
            }
            let wake_ctx = cc.egui_ctx.clone();
            crate::remote::RemoteServer::new(remote_cfg, move || {
                wake_ctx.request_repaint();
            })
        };
        prefs.cwd = options
            .cwd
            .or_else(|| {
                (!prefs.cwd.as_os_str().is_empty() && prefs.cwd.is_dir())
                    .then_some(prefs.cwd.clone())
            })
            .unwrap_or_else(|| std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")));
        prefs.cards.normalize();
        sidebar::sections::normalize(&mut prefs.sidebar_view);
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
            navigation: Default::default(),
            updates: Default::default(),
            update_install_requested: false,
            update_idle: Default::default(),
            update_recovery: recovery.clone(),
            catalog: Default::default(),
            pi_folder,
            completion_run: Default::default(),
            attention: attention::Attention::restore(
                storage
                    .and_then(|s| eframe::get_value::<attention::Stored>(s, "attention-v1"))
                    .unwrap_or_default()
                    .0,
            ),
            attention_observed: Default::default(),
            attention_dock: attention::DockWindow::new(options.demo && options.demo_attention_dock),
            restart_pending: prefs.restart_checkpoints.clone(),
            prefs,
            demo: options.demo,
            agents: Default::default(),
            side_chat: Default::default(),
            tasks: tasks::Tasks::new(&cc.egui_ctx, !options.demo && !cfg!(test)),
            kanban: Default::default(),
            automation: automation::Automation::new(!options.demo && !cfg!(test)),
            deletion: Default::default(),
            ultracode: false,
            ultracode_ready: false,
            resource_reload: Default::default(),
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
            worker_upgrade_pending: None,
            history_view: Default::default(),
            history_pending: None,
            history_drafts: Vec::new(),
            waiting: Default::default(),
            deliveries: Vec::new(),
            interrupt: None,
            draft: String::new(),
            dictation: dictation::Dictation::default(),
            context_chips: Default::default(),
            recall: Default::default(),
            composer_hints: Default::default(),
            demo_composer_hints: options.demo_composer_hints,
            recall_width: 700.0,
            completions: Default::default(),
            draft_images: Default::default(),
            image_picker_rx: None,
            new_chat: session.is_none() && !options.demo,
            chat_folder_rx: None,
            chat_folder_target: None,
            project_picker: Default::default(),
            paste_requested,
            #[cfg(target_os = "macos")]
            _paste_monitor: paste_monitor,
            search: String::new(),
            search_open: false,
            spotlight: Default::default(),
            projects,
            model_picker_open: false,
            effort_picker_open: false,
            effort_anchor: None,
            effort_picker_focus: false,
            model_anchor: None,
            model_picker_focus: false,
            model_query: String::new(),
            model_provider: String::new(),
            show_archived: false,
            show_hidden: false,
            closed_projects,
            expanded_projects,
            sidebar_cache: Default::default(),
            sidebar_running: Default::default(),
            section_view: Default::default(),
            rename_chat: None,
            rename_text: String::new(),
            usage: if options.demo {
                crate::usage::Usage::demo()
            } else {
                usage_cache
                    .map(crate::usage::Usage::from_cached)
                    .unwrap_or_default()
            },
            dismissed_usage_warning: None,
            dismissed_auth_error: None,
            sessions: Vec::new(),
            local_sessions: Vec::new(),
            cloud_sessions: Vec::new(),
            cloud_session_meta: std::collections::HashMap::new(),
            cloud_sessions_rx: None,
            cloud_sessions_last_poll: None,
            session_rx: None,
            session_watch: if options.demo || cfg!(test) {
                None
            } else {
                sessions::session_root().map(|root| sessions::Watch::new(&root, &cc.egui_ctx))
            },
            workspace: Workspace::default(),
            workspace_rx: None,
            workspace_retry: false,
            ctx: cc.egui_ctx.clone(),
            transcript: Transcript::default(),
            outputs: Vec::new(),
            pulls: Default::default(),
            output_revision: u64::MAX,
            inspector_panel: false,
            files: Default::default(),
            preview_opens: Vec::new(),
            session_layouts: Default::default(),
            terminals: Default::default(),
            changes: Default::default(),
            diff_rx: None,
            dialog_text: String::new(),
            questions: Default::default(),
            question_designs: options.demo_question_designs.then(Default::default),
            current_dialog: String::new(),
            pending_dialog: None,
            focus_composer: true,
            focus_search: false,
            chat_find: chat_find::ChatFind::default(),
            screenshot: options.screenshot,
            frames: 0,
            remote,
            pending_origins: Default::default(),
            remote_snapshot_tick: 0,
        };
        app.settings_open = options.settings || options.demo_settings;
        if let Some(page) = options.settings_page.as_deref() {
            app.settings_view.select(page);
        }
        // Dedicated, corruption-isolated local data; restores no review/editor/action.
        if !app.demo {
            app.changes.review_comments.restore_checkpoint(storage);
        }
        if app.demo {
            app.load_demo();
            if options.demo_attention {
                app.demo_attention();
            }
            if options.demo_sections {
                app.demo_sidebar_sections();
            }
            if options.demo_view_menu {
                sidebar_view::demo_menu(&app.ctx);
            }
            if options.demo_search {
                app.search = "context".into();
                app.open_search();
            }
            if options.demo_search_attention {
                app.demo_search_attention();
            }
            if options.demo_quick_open {
                app.open_quick_files();
                app.search = "src/app/search.rs:42".into();
            }
            if options.demo_side_chat {
                app.demo_side_chat();
            }
            if options.demo_languages {
                app.state.session_name = "Language glyph preview".into();
                app.state.messages = vec![Message {
                    role: "assistant".into(),
                    text: "**Glyph coverage preview**\n\nHebrew: שלום עולם — שָׁלוֹם\n\nGreek: Ελληνικά · Cyrillic: Привет\n\nArabic: مرحبا · Hindi: हिन्दी · Thai: ไทย\n\nCJK: 日本語 · 中文 · 한국어\n\n`שלום English 123`\n\nHebrew and mixed-direction text use Unicode bidirectional layout. Arabic/Indic complex-script shaping and precise vowel-mark positioning remain unsupported.".into(),
                    ..Default::default()
                }];
                app.draft = "שלום עולם · English 123 · 日本語".into();
                app.prefs.hide_inspector = true;
            }
            if options.demo_markdown {
                app.state.session_name = "Mixed Markdown alignment preview".into();
                app.state.messages = vec![Message {
                    role: "assistant".into(),
                    text: "**Status:** [#69](https://example.com/pr/69) is merged, but this linked scan still contains the finding. It does **not prove an exploitable bypass**, nor does \"Passed\" prove resolution. The post-merge release is queued; successful deployment is not yet verified.\n\nNormal **bold** normal *italic* normal [link](https://example.com) normal `inline code` normal **[bold link](https://example.com)** normal.\n\n- Normal **bold** and [linked text](https://example.com) with `inline code`.\n\n## Heading with **bold**, [link](https://example.com) and `code`\n\n| Style | Mixed content |\n| --- | --- |\n| Formatting | Normal **bold** *italic* [link](https://example.com) `code` |\n| Languages | English **מודגש** [קישור](https://example.com) `code` |".into(),
                    ..Default::default()
                }];
                app.draft.clear();
                app.prefs.hide_inspector = true;
            }
            if options.demo_quote_selection {
                app.state.messages = vec![Message {
                    role: "assistant".into(),
                    text: context_capture::demo_rendered(&cc.egui_ctx).into(),
                    ..Default::default()
                }];
                app.state.prose_revision = app.state.prose_revision.wrapping_add(1);
                app.prefs.hide_inspector = true;
                app.draft = "Keep this draft".into();
            }
            if options.demo_context_chips {
                app.draft = "Keep this draft".into();
                app.prefs.hide_inspector = true;
                for (label, text) in [
                    (
                        "src/main.rs:4–5 · selected source",
                        "let chosen = \"frozen literal\";",
                    ),
                    (
                        "Terminal · visible viewport · /demo/project",
                        "$ example output\nLiteral untrusted text",
                    ),
                    (
                        "Browser Console · https://example.test · app.js:12",
                        "Example error: selected console message",
                    ),
                ] {
                    context_capture::queue(&cc.egui_ctx, label, text);
                    app.collect_context_capture();
                }
            }
            app.menu.about = options.demo_about;
            // Preview only: opening Updates must not start a network check.
            app.updates.open = options.demo_updates;
            if options.demo_approval {
                app.state.busy = true;
                let args = json!({"code":"const results = await Promise.all([\n  tools.read({ path: \"src/app/prompts.rs\" }),\n  tools.bash({ command: \"cargo test --all-targets\", timeout: 120 })\n]);\nfor (const result of results) text(result);"});
                app.state.dialogs.push_back(json!({"type":"extension_ui_request","method":"select","id":"preview-approval","title":format!("Allow codemode?\n\nAuto flagged this action for your review.\n\n{args}\n\nApprove this exact action."),"options":["Allow once","Always allow this exact action (this chat)","Deny"]}));
            }
            if options.demo_questions {
                app.state.busy = true;
                app.state.dialogs.push_back(questions::demo_dialog());
            }
            if options.demo_model_picker || options.demo_effort_picker {
                app.model_picker_open = options.demo_model_picker;
                app.effort_picker_open = options.demo_effort_picker;
                app.ultracode_ready = true;
            }
            if options.demo_automation {
                app.demo_automation();
                app.open_automation();
            } else if options.demo_kanban {
                app.demo_kanban_agents();
                app.open_agents_board();
            } else if options.demo_agents {
                app.demo_agent_tree();
            }
            if options.demo_sidebar_density {
                app.demo_sidebar_density();
            }
            if options.demo_task {
                app.demo_task();
                if options.demo_task_proposal {
                    app.tasks.demo_proposal();
                }
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
                if options.demo_terminal_tabs {
                    app.terminals.demo_tabs(&cc.egui_ctx);
                }
                app.focus_composer = false;
                if options.demo_terminal_context {
                    app.terminals.demo_context();
                }
            }
            if options.demo_files {
                app.files.toggle(&cc.egui_ctx, &app.prefs.cwd);
                if options.demo_source_context {
                    app.files.demo_source_context(&cc.egui_ctx);
                }
                if options.demo_preview_budget {
                    app.files.demo_preview_budget(&cc.egui_ctx);
                    app.focus_composer = false;
                }
                if options.demo_preview_queue {
                    app.files.demo_preview_queue(&cc.egui_ctx);
                    app.focus_composer = false;
                }
                if options.demo_table_grid {
                    app.focus_composer = false;
                    app.files.demo_table_grid(&cc.egui_ctx);
                    if options.demo_table_projection {
                        app.files.demo_table_projection();
                    }
                    if options.demo_table_headerless {
                        app.prefs.table_headers.set(
                            &app.prefs.cwd,
                            &app.prefs.cwd.join("table-keyboard-fixture.csv"),
                            false,
                        );
                    }
                }
            }
            if options.demo_changes {
                app.inspector_panel = true;
            }
            if options.demo_inline_review {
                let source=(0..4).map(|n|format!("@@ -{} +{} @@ synthetic hunk {}\n-old value\n+new literal value 世界\n",n*20+1,n*20+1,n+1)).collect::<String>();
                app.outputs = vec![RecordedFile {
                    path: "src/synthetic-inline.rs".into(),
                    diff: source,
                    ..Default::default()
                }];
                app.output_revision = app.state.outputs_revision;
                let owner = app.review_comment_owner();
                app.changes.demo_inline(&app.prefs.cwd, &owner);
                app.focus_composer = false;
            }
            if options.demo_branches {
                app.workspace = Workspace {
                    root: app.prefs.cwd.clone(),
                    branch: "fixture/main".into(),
                    is_git: true,
                    ..Default::default()
                };
                let review_owner = app.review_comment_owner();
                app.changes.demo_branches(
                    &cc.egui_ctx,
                    &app.prefs.cwd,
                    &review_owner,
                    &app.workspace,
                );
                if options.demo_git_history {
                    app.changes.demo_git_history();
                    if options.demo_history_filter {
                        app.changes.demo_history_filter();
                    }
                    if options.demo_commit_detail {
                        app.changes.demo_commit_detail(&cc.egui_ctx);
                    }
                }
                app.focus_composer = false;
            }
            if options.demo_review_files || options.demo_review_comments {
                app.outputs = activity::recorded_files(&app.state.messages);
                app.output_revision = app.state.outputs_revision;
                if let Some(file) = app.outputs.last() {
                    let document = (file.path.clone(), file.diff.clone()).into();
                    app.open_diff_review(document, true);
                }
                app.focus_composer = false;
            }
            if options.demo_review_comments {
                app.changes.demo_review_comments(app.output_revision);
                if options.demo_review_note_delete {
                    app.changes.demo_review_note_delete();
                }
                if options.demo_review_note_edit {
                    app.changes.demo_review_note_edit();
                }
                if options.demo_review_note_files {
                    app.demo_review_note_files();
                }
            }
            if options.demo_git_files {
                app.workspace = Workspace {
                    root: app.prefs.cwd.clone(),
                    branch: "demo · no repository read".into(),
                    is_git: true,
                    files: ["first.txt", "second.txt", "third.txt"]
                        .into_iter()
                        .map(|path| workspace::FileChange {
                            path: path.into(),
                            status: "??".into(),
                            ..Default::default()
                        })
                        .collect(),
                    ..Default::default()
                };
                app.open_diff_review(
                    (
                        "first.txt".into(),
                        "Untracked file — Git has no diff yet. Pui does not stage or modify files."
                            .into(),
                    )
                        .into(),
                    false,
                );
                app.focus_composer = false;
            }
            if options.demo_diff_context {
                let fixture = context_capture::demo_diff(&cc.egui_ctx);
                if let Some(message) = app.state.messages.last_mut() {
                    message.tools.push(protocol::Tool {
                        name: "edit".into(),
                        path: fixture.0.clone(),
                        diff: fixture.1.clone(),
                        added: 1,
                        removed: 1,
                        finished: true,
                        ..Default::default()
                    });
                    app.state.outputs_revision = app.state.outputs_revision.wrapping_add(1);
                }
                app.open_diff_review(fixture.into(), true);
            }
            if options.demo_diff_hunks {
                let document = if options.demo_diff_wrap {
                    diff_viewer::demo_wrap(&cc.egui_ctx)
                } else {
                    diff_viewer::demo_hunks(&cc.egui_ctx)
                };
                if let Some(message) = app.state.messages.last_mut() {
                    message.tools.push(protocol::Tool {
                        name: "edit".into(),
                        path: document.0.clone(),
                        diff: document.1.clone(),
                        added: if options.demo_diff_wrap { 2 } else { 5 },
                        removed: if options.demo_diff_wrap { 2 } else { 5 },
                        finished: true,
                        ..Default::default()
                    });
                    app.state.outputs_revision = app.state.outputs_revision.wrapping_add(1);
                }
                app.open_diff_review(document, true);
            }
            if options.demo_confirmation {
                app.agents.close_prompt = true;
            }
            if options.demo_notifications {
                app.state.notice = Some("Returned to the selected prompt. Review the draft, then send when ready. Nothing was rerun.".into());
                app.state.add_diagnostic("MCP server unavailable (HTTP 503). The remote service could not be reached. Local chat is still available.".into());
                app.diagnostics_open = true;
            }
            if options.demo_composer_hints {
                app.state.busy = true;
                app.draft.clear();
                app.focus_composer = false;
                app.prefs.settings.delivery = settings::DeliveryMode::Steer;
            }
            if options.demo_send_now {
                app.state.busy = true;
                app.state.pending_messages.push(protocol::PendingMessage {
                    id: "demo-interrupt".into(),
                    text: "Stop the browser search and use the CSV I attached instead.".into(),
                    images: vec![],
                    status: protocol::DeliveryStatus::Interrupting,
                    observed: 0,
                });
            }
            app.transcript.expand_activity = options.demo_expanded;
            if options.demo_collapsed {
                app.prefs.cards.collapsed = Card::ALL.into_iter().collect();
            }
        } else {
            app.notifications.initialize(&cc.egui_ctx);
            app.restore_agents();
            if restore_composers {
                app.restore_parked_composers();
            }
            app.refresh_sessions();
            app.refresh_workspace();
            if let Some(id) = session.as_deref().and_then(|path| app.agent_for_path(path)) {
                // Restore workers through their saved launch policy (including read-only access).
                app.view_agent(&id);
            } else {
                app.connect(&cc.egui_ctx, session);
            }
            if restore_blank {
                app.restore_blank_checkpoint();
            }
            app.restore_workspace_layout();
        }
        if let Some(target) = options.open {
            if let Err(error) = app.files.open_target(&cc.egui_ctx, &app.prefs.cwd, &target) {
                app.state.notice = Some(error);
            }
        }
        if let Some(device) = options.browser_device {
            if let Err(error) = app
                .files
                .configure_browser(&device, options.browser_console)
            {
                app.state.notice = Some(error);
            }
        } else if options.browser_console {
            let _ = app.files.configure_browser("responsive", true);
        }
        if let Some(error) = recovery_error {
            app.state.error = Some(error);
        }
        app.remember_navigation();
        app
    }

    fn refresh_sessions(&mut self) {
        if self.session_rx.is_some() || self.demo {
            return;
        }
        if let Some(watch) = &mut self.session_watch {
            watch.scanned();
        }
        let (tx, rx) = mpsc::channel();
        self.session_rx = Some(rx);
        let ctx = self.ctx.clone();
        let retained = self
            .prefs
            .chats
            .iter()
            .filter(|(_, prefs)| prefs.pinned || prefs.hidden || prefs.section.is_some())
            .map(|(path, _)| path.clone())
            .collect::<Vec<_>>();
        std::thread::spawn(move || {
            let _ = tx.send(sessions::discover(&retained));
            ctx.request_repaint();
        });
    }

    fn refresh_workspace(&mut self) {
        if self.workspace_rx.is_some() || self.demo {
            return;
        }
        if changes::git_files::workspace_worker_busy(&self.ctx) {
            self.workspace_retry = true;
            self.ctx
                .request_repaint_after(std::time::Duration::from_millis(80));
            return;
        }
        self.workspace_retry = false;
        self.workspace_rx = Some(changes::git_files::start_workspace(
            &self.ctx,
            self.prefs.cwd.clone(),
        ));
    }

    fn connect(&mut self, ctx: &egui::Context, session: Option<PathBuf>) {
        if session.as_ref().is_some_and(|path| {
            self.agents
                .idle
                .retiring
                .values()
                .any(|(old, _)| old == path)
        }) {
            self.state.notice =
                Some("Previous process is still shutting down. Your draft is preserved.".into());
            return;
        }
        if session
            .as_deref()
            .is_some_and(|p| self.deletion.deleting_chat(p))
        {
            return;
        }
        let active_cloud_host = if let Some(ref p) = session {
            if let Some(meta) = self.cloud_session_meta.get(p) {
                if let Some(node) = self.agents.nodes.get_mut(&self.agents.active) {
                    node.cloud_host = Some(meta.host.clone());
                    node.cloud_repo = None;
                }
                Some(meta.host.clone())
            } else if p.is_file() {
                if let Some(node) = self.agents.nodes.get_mut(&self.agents.active) {
                    node.cloud_host = None;
                    node.cloud_repo = None;
                }
                None
            } else {
                self.agents
                    .nodes
                    .get(&self.agents.active)
                    .and_then(|n| n.cloud_host.clone())
            }
        } else {
            self.agents
                .nodes
                .get(&self.agents.active)
                .and_then(|n| n.cloud_host.clone())
        };
        let is_cloud_session = active_cloud_host.is_some();
        #[cfg(not(test))]
        let active_cloud_repo = self
            .agents
            .nodes
            .get(&self.agents.active)
            .and_then(|n| n.cloud_repo.clone());
        #[cfg(not(test))]
        if active_cloud_host.is_none() && session.as_ref().is_some_and(|p| !p.is_file()) {
            self.loading_chat = false;
            self.state.error =
                Some("This chat was deleted or its session file is unavailable.".into());
            return;
        }
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
        self.resource_reload = Default::default();
        self.recall = Default::default();
        self.completion_run = Default::default();
        self.attention_observed = Default::default();
        self.dialog_text.clear();
        self.preview_opens.clear();
        self.questions = Default::default();
        self.current_dialog.clear();
        self.pending_dialog = None;
        self.connected = false;
        self.completions = Default::default();
        self.ultracode = false;
        self.ultracode_ready = false;
        self.loading_chat = true;
        self.pending_session = session.clone();
        self.preview_rx = None;
        self.preview_partial = false;
        self.uncertain_deliveries();
        let restart_notice = if session.is_some()
            && self.state.session_file.as_deref().map(std::path::Path::new) == session.as_deref()
        {
            std::mem::take(&mut self.state.restart_notice)
        } else {
            String::new()
        };
        let pending_messages =
            if self.state.session_file.as_deref().map(std::path::Path::new) == session.as_deref() {
                std::mem::take(&mut self.state.pending_messages)
            } else {
                Vec::new()
            };
        self.state = State {
            pending_messages,
            restart_notice,
            ..Default::default()
        };
        self.seed_model_catalog();
        self.transcript.clear();
        self.chat_find.clear_results();
        self.pulls = Default::default();
        if let Some(path) = session.clone() {
            self.state.session_file = Some(path.display().to_string());
            self.state.session_name = self
                .sessions
                .iter()
                .find(|s| s.path == path)
                .map(|s| s.title.clone())
                .unwrap_or_else(|| "Opening chat…".into());
            self.state.preserve_title(&self.state.session_name.clone());
            if active_cloud_host.is_none() {
                let (tx, rx) = mpsc::channel();
                self.preview_rx = Some(rx);
                let ctx = ctx.clone();
                std::thread::spawn(move || {
                    let _ = tx.send(crate::history::load(&path));
                    ctx.request_repaint();
                });
            }
        }
        self.output_revision = u64::MAX;
        self.restore_restart_checkpoint();
        if active_cloud_host.is_none() {
            if let Some(path) = session.as_ref() {
                if !self.attach_or_claim(path) {
                    return;
                }
            }
        }
        #[cfg(not(test))]
        {
            if let Some(cloud_cfg) = active_cloud_host {
                // Cloud agents always run in Auto mode inside an isolated pui_agent/… worktree.
                self.client = Some(rpc::Client::start_cloud(
                    cloud_cfg,
                    &self.prefs.cwd,
                    active_cloud_repo,
                    session.as_deref(),
                    "auto".to_owned(),
                    ctx.clone(),
                ));
            } else {
                self.client = Some(rpc::Client::start_with(
                    &self.prefs.cwd,
                    session.as_deref(),
                    ctx.clone(),
                    rpc::Launch {
                        settings: self.agent_settings(&self.agents.active).payload(),
                        ..Default::default()
                    },
                ));
            }
        }
        // Interaction tests never spawn a real agent or initialize user extensions.
        #[cfg(test)]
        {
            self.client = Some(rpc::Client::stub());
        }
        self.send(protocol::command("get_state"));
        self.send(protocol::command("get_available_models"));
        self.send(protocol::command("get_available_thinking_levels"));
        self.send(protocol::command("get_entries"));
        self.send(protocol::command("get_commands"));
        self.send(protocol::command("get_session_stats"));
        if !is_cloud_session {
            self.refresh_workspace();
        } else {
            self.refresh_cloud_sessions(true);
        }
        #[cfg(not(test))]
        if !self.demo {
            self.usage.refresh(&self.ctx, false);
        }
    }

    // Viewing another chat is independent of its run state or IPC registration.
    // Keep the RPC connection (and outstanding approvals/deliveries) parked.
    fn retain_chat_on_switch(&self) -> bool {
        self.has_tree()
            || self.resource_reload.pending()
            || self.resource_reload.connector_pending()
            || !self.draft.is_empty()
            || !self.draft_images.is_empty()
            || self.sync_live_owner()
            || (!self.is_mirror()
                && self.client.is_some()
                && (!self.state.ready() || !self.deliveries.is_empty()))
    }

    fn open_chat(&mut self, session: Session) {
        if self.deletion.in_progress() || self.deletion.deleting_chat(&session.path) {
            return;
        }
        self.kanban.open = false;
        self.automation.open = false;
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
            if self.agents.idle.dormant.contains(&id) {
                if let Err(error) = self.wake_idle_agent(&id) {
                    self.state.error = Some(error);
                }
                return;
            }
            if self.agents.nodes[&id].stopped && self.agents.nodes[&id].status != "Saved" {
                return;
            }
            if self.has_tree() {
                if self.client.is_none() && !self.resume_agent() {
                    self.connect(&self.ctx.clone(), Some(path));
                }
                return;
            }
        }
        self.discard_empty_new_chat();
        let target_is_cloud = self.cloud_session_meta.contains_key(&path);
        let current_is_cloud = self
            .agents
            .nodes
            .get(&self.agents.active)
            .is_some_and(|n| n.cloud_host.is_some());
        if self.retain_chat_on_switch() {
            self.ensure_agent();
            self.publish_live();
            self.park_tree();
            if !target_is_cloud {
                self.prefs.cwd = session.cwd;
            }
            self.connect(&self.ctx.clone(), Some(path));
            self.ensure_agent();
            self.enter_session_layout();
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
        let reuse = !target_is_cloud
            && !current_is_cloud
            && !self.loading_chat
            && self.connected
            && self.client.is_some()
            && self.prefs.cwd == session.cwd
            && !self.sync.enabled();
        self.leave_session_layout();
        if !target_is_cloud {
            self.prefs.cwd = session.cwd;
        }
        self.workspace_rx = None;
        self.draft.clear();
        self.context_chips.clear();
        self.draft_images.clear();
        self.state.restart_notice.clear();
        if reuse {
            self.loading_chat = true;
            self.pending_session = Some(path.clone());
            self.preview_partial = false;
            self.state.session_name = session.title;
            self.state.session_file = Some(path.display().to_string());
            self.state.title_request = None;
            self.state.session_id = None;
            self.state.preserve_title(&self.state.session_name.clone());
            self.state.messages.clear();
            self.state.cost = 0.0;
            self.state.context_tokens = None;
            self.state.context_breakdown = None;
            self.state.outputs_revision = self.state.outputs_revision.wrapping_add(1);
            self.transcript.clear();
            self.chat_find.clear_results();
            let (tx, rx) = mpsc::channel();
            self.preview_rx = Some(rx);
            let preview_path = path.clone();
            let ctx = self.ctx.clone();
            std::thread::spawn(move || {
                let _ = tx.send(crate::history::load(&preview_path));
                ctx.request_repaint();
            });
            self.restore_restart_checkpoint();
            self.send(json!({"type":"switch_session","sessionPath":path}));
            self.refresh_workspace();
        } else {
            self.connect(&self.ctx.clone(), Some(path));
        }
        self.enter_session_layout();
        self.focus_composer = true;
        self.pulls = Default::default();
    }

    fn send(&mut self, record: Value) {
        if record["type"] == "abort" {
            self.completion_run.cancel();
            self.attention_observed.cancel();
        }
        if self.remote_action(sync::Action::Rpc(record.clone())) {
            return;
        }
        if self.workflow_answer(&record) {
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
        self.poll_deletions();
        self.usage.poll();
        self.poll_notifications();
        self.updates
            .maybe_auto_check(&self.ctx, self.prefs.settings.auto_check_updates, self.demo);
        self.updates.poll();
        self.updates
            .maybe_auto_download(&self.ctx, &mut self.prefs.settings, self.demo);
        self.settings_view.resources.poll();
        self.settings_view.providers.poll();
        if let Some(host_name) = self.settings_view.providers.take_cloud_auth_completed() {
            self.state.error = None;
            self.dismissed_auth_error = None;
            self.state.notice = Some(format!(
                "Signed in to Claude on {host_name}. You can send a message now."
            ));
        }
        if self.catalog.poll(&self.ctx) {
            self.catalog.with_extensions(&self.prefs.extension_models);
        }
        if !self.demo && !self.catalog.started {
            self.catalog.request(&self.ctx);
        }
        if self.settings_view.providers.reload_requested {
            self.settings_view.providers.reload_requested = false;
            self.refresh_providers_catalog();
            self.catalog.request(&self.ctx);
            self.on_provider_credentials_updated();
        } else if !self.demo
            && self
                .settings_view
                .providers
                .config_changed(std::time::Instant::now())
        {
            self.settings_view.providers.reload_from_disk();
            self.refresh_providers_catalog();
            self.catalog.request(&self.ctx);
        }
        if !self.demo {
            // Low-frequency wake for the provider-file check; no model calls.
            self.ctx.request_repaint_after(Duration::from_secs(2));
        }
        self.poll_chat_folder();
        self.poll_tasks();
        self.poll_rpc();
        self.poll_history();
        self.poll_other_agents();
        self.poll_automation();
        self.poll_views();
        self.publish_live();
        self.poll_window_sync();
        self.poll_remote();
        self.poll_idle_agents(std::time::Instant::now());
    }
    fn poll_rpc(&mut self) -> bool {
        let mut title_dirty = false;
        let mut had_activity = false;
        if let Some(preview) = self.preview_rx.as_ref().and_then(|rx| rx.try_recv().ok()) {
            had_activity = true;
            self.preview_rx = None;
            if let Ok(preview) = preview {
                self.preview_partial = preview.partial;
                self.state.messages = preview.messages;
                title_dirty = true;
                self.state.outputs_revision = self.state.outputs_revision.wrapping_add(1);
            }
        }
        let events: Vec<_> = self
            .client
            .as_ref()
            .map(|c| c.events.try_iter().collect())
            .unwrap_or_default();
        if !events.is_empty() {
            had_activity = true;
        } else if !had_activity
            && !self.resource_reload.pending()
            && !self.resource_reload.connector_pending()
            && self.agents.background
        {
            return false;
        }
        for event in events {
            match event {
                rpc::Event::History(messages) => {
                    title_dirty = true;
                    self.resource_reload_history();
                    self.preview_rx = None;
                    self.preview_partial = false;
                    self.transcript
                        .replace_history(&self.state.messages, &messages);
                    self.state.messages = messages;
                    self.state.reconcile_pending_messages();
                    if !self.state.messages.is_empty() {
                        self.new_chat = false;
                    }
                    self.state.transcript_revision = self.state.transcript_revision.wrapping_add(1);
                    self.state.prose_revision = self.state.prose_revision.wrapping_add(1);
                    self.state.outputs_revision = self.state.outputs_revision.wrapping_add(1);
                    self.connected = true;
                    self.loading_chat = self.resource_reload.pending();
                    self.pending_session = None;
                }
                rpc::Event::Record(record) => {
                    title_dirty |= session_titles::title_event(&record);
                    if self.connector_reconnect_record(&record) {
                        continue;
                    }
                    self.resource_reload_record(&record);
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
                            title_dirty = false;
                            break;
                        }
                        continue;
                    }
                    if self.open_request(&record) {
                        continue;
                    }
                    if self.control_request(&record) {
                        continue;
                    }
                    self.attention_request(&record);
                    if record["type"] == "extension_ui_request" && record["method"] == "setStatus" {
                        let status_key = protocol::string(&record, "statusKey");
                        if status_key == "pui-ultracode" {
                            if let Ok(value) = serde_json::from_str::<Value>(protocol::string(
                                &record,
                                "statusText",
                            )) {
                                self.ultracode_ready = value["ready"] == true;
                                self.ultracode = value["enabled"] == true;
                            }
                            continue;
                        }
                        if status_key == "pui-models" {
                            self.send(protocol::command("get_available_models"));
                            if let Ok(value) = serde_json::from_str::<Value>(protocol::string(
                                &record,
                                "statusText",
                            )) && value["switched"].is_string()
                            {
                                self.send(protocol::command("get_state"));
                                self.send(protocol::command("get_available_thinking_levels"));
                            }
                            continue;
                        }
                    }
                    let kind = protocol::string(&record, "type").to_owned();
                    let command = protocol::string(&record, "command").to_owned();
                    let success = record["success"] == true;
                    if kind == "response" && self.pending_session.is_none() {
                        self.connected = true;
                    }
                    if kind == "response"
                        && !success
                        && matches!(command.as_str(), "get_entries" | "new_session")
                    {
                        self.loading_chat = false;
                        self.pending_session = None;
                        self.preview_rx = None;
                    }
                    if kind == "extension_ui_request"
                        && protocol::string(&record, "method") == "set_editor_text"
                    {
                        self.context_chips.clear();
                        self.draft = protocol::string(&record, "text").into();
                        if !self.agents.background {
                            self.focus_composer = true;
                        }
                    }
                    if kind == "response" {
                        let mut mail_record = record.clone();
                        mail_record["id"] =
                            format!("{}:{}", self.agents.active, protocol::string(&record, "id"))
                                .into();
                        self.automation_response(
                            protocol::string(&mail_record, "id"),
                            &mail_record,
                        );
                        self.agent_mail_response(&mail_record);
                        self.delivery_response(&record);
                    }
                    if kind == "response" && !success && command == "prompt" {
                        self.agent_failed(protocol::string(&record, "error"));
                    }
                    let cancelled = record["data"]["cancelled"] == true;
                    let record_for_interrupt =
                        if self.interrupt.is_some() && kind == "response" && command == "abort" {
                            record.clone()
                        } else {
                            Value::Null
                        };
                    let manual_compaction = record["reason"] == "manual"
                        && record["result"].is_object()
                        && record["aborted"] != true
                        && !record["errorMessage"].is_string();
                    let user_message = matches!(kind.as_str(), "message_start" | "message_end")
                        && record["message"]["role"] == "user";
                    let was_compacting = self.state.compacting;
                    self.state.apply(record);
                    if kind == "response"
                        && success
                        && matches!(
                            command.as_str(),
                            "get_state"
                                | "get_available_models"
                                | "set_model"
                                | "set_thinking_level"
                        )
                    {
                        if command == "get_available_models" && !self.demo && !cfg!(test) {
                            self.settings_view.providers.reload_from_disk();
                        }
                        if command == "get_available_models" && success {
                            self.catalog.learn_extension_models(
                                &self.state.models,
                                &mut self.prefs.extension_models,
                            );
                            self.catalog.with_extensions(&self.prefs.extension_models);
                        }
                        self.remember_model_catalog(&command);
                    }
                    if user_message || (kind == "response" && command == "get_entries") {
                        self.state.reconcile_pending_messages();
                    }
                    self.interrupt_response(&record_for_interrupt);
                    if kind == "session_info_changed" {
                        self.refresh_sessions();
                    }
                    if kind == "response" && command == "switch_session" {
                        if success && !cancelled {
                            self.send(protocol::command("get_state"));
                            self.send(protocol::command("get_entries"));
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
                            self.send(protocol::command("get_entries"));
                        }
                    }
                    // Runs can contain hundreds of turns; settle-only stats leave
                    // the context meter stale until after auto-compaction starts.
                    if kind == "turn_end" || (!was_compacting && self.state.compacting) {
                        self.send(protocol::command("get_session_stats"));
                    }
                    // Boundary-hook compactions need not emit compaction_end.
                    // Refresh at settle too, when Pi has persisted the full turn.
                    if kind == "agent_settled" || (kind == "compaction_end" && manual_compaction) {
                        self.send(protocol::command("get_entries"));
                    }
                    if kind == "response" && success && command == "prompt" {
                        self.send(protocol::command("get_commands"));
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
                            self.send(protocol::command("get_entries"));
                            self.send(protocol::command("get_session_stats"));
                        }
                    }
                }
                rpc::Event::Error(error) => {
                    self.attention_failure();
                    self.uncertain_deliveries();
                    self.agent_failed(&error);
                    self.state.error = Some(error);
                }
                rpc::Event::Exited => {
                    self.attention_failure();
                    self.attention_observed = Default::default();
                    self.cancel_workflows_for(&self.agents.active.clone());
                    self.uncertain_deliveries();
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
                    self.state.retrying = false;
                    self.state.dialogs.clear();
                    if self.history_pending.take().is_some() {
                        self.state.error = Some("Pi disconnected during history navigation. Outcome unknown; check the chat before retrying. Nothing was resent.".into());
                    }
                    self.agent_event(&json!({"type":"agent_settled"}));
                }
            }
        }
        self.poll_resource_reload();
        self.promote_attention();
        if title_dirty {
            self.update_session_title();
        }
        had_activity
    }
    pub(super) fn active_cloud_host(&self) -> Option<crate::remote::CloudHostConfig> {
        if let Some(cfg) = self
            .agents
            .nodes
            .get(&self.agents.active)
            .and_then(|n| n.cloud_host.clone())
        {
            return Some(cfg);
        }
        self.state
            .session_file
            .as_deref()
            .and_then(|p| self.cloud_session_meta.get(std::path::Path::new(p)))
            .map(|m| m.host.clone())
    }

    pub(super) fn active_cloud_meta(&self) -> Option<&CloudSessionMeta> {
        self.state
            .session_file
            .as_deref()
            .and_then(|p| self.cloud_session_meta.get(std::path::Path::new(p)))
    }

    fn merge_sessions(&mut self) {
        let mut combined = self.local_sessions.clone();
        let mut seen: std::collections::HashSet<PathBuf> =
            combined.iter().map(|s| s.path.clone()).collect();
        for s in &self.cloud_sessions {
            if seen.insert(s.path.clone()) {
                combined.push(s.clone());
            }
        }
        combined.sort_by_key(|s| std::cmp::Reverse(s.modified));
        self.sessions = combined;
    }

    pub(super) fn refresh_cloud_sessions(&mut self, force: bool) {
        if self.demo || cfg!(test) {
            return;
        }
        let Some(cfg) = self.prefs.remote.cloud_host.clone() else {
            if !self.cloud_sessions.is_empty() || !self.cloud_session_meta.is_empty() {
                self.cloud_sessions.clear();
                self.cloud_session_meta.clear();
                self.merge_sessions();
            }
            return;
        };
        if self.cloud_sessions_rx.is_some() {
            return;
        }
        if !force
            && self
                .cloud_sessions_last_poll
                .is_some_and(|t| t.elapsed() < Duration::from_secs(8))
        {
            return;
        }
        self.cloud_sessions_last_poll = Some(std::time::Instant::now());
        let (tx, rx) = mpsc::channel();
        self.cloud_sessions_rx = Some(rx);
        let ctx = self.ctx.clone();
        std::thread::spawn(move || {
            if let Ok(items) = crate::remote::cloud::fetch_cloud_sessions(&cfg) {
                let mut sessions_out = Vec::with_capacity(items.len());
                let mut meta_out = std::collections::HashMap::with_capacity(items.len());
                for item in items {
                    let path = PathBuf::from(&item.path);
                    if let Some(origin) = item.origin {
                        sessions::remember_origin(&path, origin);
                    }
                    let project = crate::remote::cloud::project_path_for_cloud_session(
                        &item.project,
                        &item.project_name,
                        &item.cwd,
                    );
                    let repo_name = if !item.project_name.trim().is_empty() {
                        item.project_name.clone()
                    } else {
                        sessions::project_name(&project)
                    };
                    let modified =
                        std::time::UNIX_EPOCH + Duration::from_millis(item.modified_ms.max(1));
                    let created = item
                        .created_ms
                        .map(|ms| std::time::UNIX_EPOCH + Duration::from_millis(ms));
                    meta_out.insert(
                        path.clone(),
                        CloudSessionMeta {
                            host: cfg.clone(),
                            status: item.status,
                            busy: item.busy,
                            has_pending_dialog: item.has_pending_dialog,
                            cwd: item.cwd.clone(),
                            repo_name,
                            branch: item.branch,
                        },
                    );
                    sessions_out.push(Session {
                        path,
                        cwd: PathBuf::from(&item.cwd),
                        project,
                        title: item.title,
                        created,
                        modified,
                    });
                }
                let _ = tx.send((sessions_out, meta_out));
                ctx.request_repaint();
            }
        });
    }

    fn poll_views(&mut self) {
        if let Some(result) = self.session_rx.as_ref().and_then(|rx| rx.try_recv().ok()) {
            self.local_sessions = result;
            self.merge_sessions();
            self.session_rx = None;
            self.refresh_session_titles();
        }
        if let Some(rx) = &self.cloud_sessions_rx {
            match rx.try_recv() {
                Ok((cloud_sessions, cloud_meta)) => {
                    self.cloud_sessions = cloud_sessions;
                    self.cloud_session_meta = cloud_meta;
                    self.cloud_sessions_rx = None;
                    self.merge_sessions();
                    self.refresh_session_titles();
                }
                Err(mpsc::TryRecvError::Disconnected) => {
                    self.cloud_sessions_rx = None;
                }
                Err(mpsc::TryRecvError::Empty) => {}
            }
        }
        self.refresh_cloud_sessions(false);
        if !self.demo
            && self
                .session_watch
                .as_mut()
                .is_some_and(|watch| watch.ready(&self.ctx, self.session_rx.is_some()))
        {
            self.refresh_sessions();
        }
        let workspace = self
            .workspace_rx
            .as_ref()
            .and_then(|rx| match rx.try_recv() {
                Ok(result) => Some(result),
                Err(mpsc::TryRecvError::Empty) => None,
                Err(mpsc::TryRecvError::Disconnected) => Some(Workspace {
                    root: self.prefs.cwd.clone(),
                    error: Some(
                        "Git workspace worker ended without a result. Refresh to retry.".into(),
                    ),
                    ..Default::default()
                }),
            });
        if let Some(result) = workspace {
            self.workspace_rx = None;
            if result.error.as_deref()
                == Some("A Git workspace read is still finishing. Refresh to retry.")
            {
                self.workspace_retry = true;
                self.ctx
                    .request_repaint_after(std::time::Duration::from_millis(80));
            } else {
                self.workspace_retry = false;
                self.workspace = result;
                self.diff_rx = None; // a refreshed repository snapshot invalidates an in-flight open
                self.changes.invalidate();
            }
        }
        if self.workspace_retry && self.workspace_rx.is_none() && !self.demo {
            if changes::git_files::workspace_worker_busy(&self.ctx) {
                self.ctx
                    .request_repaint_after(std::time::Duration::from_millis(80));
            } else {
                self.refresh_workspace();
            }
        }
        self.cancel_git_open_for_file_navigation();
        let git_review_overlay = self.settings_open
            || self.search_open
            || self.kanban.open
            || self.automation.open
            || self.tasks.open
            || self.question_designs.is_some()
            || self.transcript.gallery.is_open()
            || self.draft_images.gallery.is_open()
            || self.terminals.layout() == (true, true)
            || egui::Popup::is_any_open(&self.ctx)
            || self.ctx.memory(|memory| memory.top_modal_layer().is_some());
        if !git_review_overlay && let Some(receiver) = &self.diff_rx {
            match receiver.try_recv() {
                Ok(result) => {
                    self.diff_rx = None;
                    if let Some(result) = result.finish(&self.workspace) {
                        match result {
                            Ok(document) => {
                                let composer = egui::Id::new("composer");
                                let preserve_composer = self.focus_composer
                                    || self.ctx.memory(|memory| memory.has_focus(composer));
                                let focus_composer = self.focus_composer;
                                self.open_diff_review(document.into(), false);
                                // The explicit file action normally focuses review, but a
                                // user who resumed drafting while Git read keeps the caret.
                                if preserve_composer {
                                    self.focus_composer = focus_composer;
                                    self.ctx.memory_mut(|memory| memory.request_focus(composer));
                                }
                            }
                            Err(error) => self.state.notice = Some(error),
                        }
                    }
                }
                Err(mpsc::TryRecvError::Disconnected) => {
                    self.diff_rx = None;
                    self.state.notice = Some(
                        "Git review worker ended without a result. Try the file action again."
                            .into(),
                    );
                }
                Err(mpsc::TryRecvError::Empty) => {}
            }
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
        if self.deletion.confirming() {
            return;
        }
        if self.resource_reload.confirming() || self.resource_reload.pending() {
            return;
        }
        if self.submit_command() {
            return;
        }
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
        if self.agents.idle.dormant.contains(&self.agents.active) {
            let id = self.agents.active.clone();
            // Capture this explicit Send before reconnect marks the chat loading.
            self.queue_draft_mode(mode);
            if let Err(error) = self.wake_idle_agent(&id) {
                self.state.error = Some(error);
            }
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
        self.prepare_initial_title(&message);
        if !self.deliver_message(message, "prompt") {
            return;
        }
        self.state.restart_notice.clear();
        self.draft_images.clear();
        self.draft.clear();
        self.context_chips.clear();
        self.recall = Default::default();
        self.focus_composer = true;
    }

    fn stop(&mut self) {
        if self.tasks.open && self.tasks.running() {
            self.tasks.stop();
            return;
        }
        self.cancel_send_now();
        self.pause_agent_jobs(&self.agents.active.clone());
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
            let response = ui
                .scope_builder(
                    egui::UiBuilder::new().id_salt(title).sense(Sense::click()),
                    |ui| {
                        egui::Frame::new()
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
                            });
                    },
                )
                .response
                .on_hover_cursor(egui::CursorIcon::PointingHand);
            if response.clicked() {
                self.context_chips.clear();
                self.draft = prompt.into();
                self.focus_composer = true;
            }
            ui.add_space(4.0);
        }
    }

    fn composer(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        if let Some(notice) = self.dictation.poll(&mut self.draft) {
            self.state.notice = Some(notice);
        }
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
        if self.demo && !self.side_chat.rendering {
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
        self.restart_recovery(ui);
        self.history_draft_recovery(ui);
        self.recover_shared_draft(ui);
        self.waiting_messages(ui);
        self.agent_question(ui);
        let settings_before = self.prefs.settings.clone();
        self.permission_confirmation(ui);
        self.propagate_settings(&settings_before);
        self.auth_error_banner(ui, ctx);
        self.usage_limit_banner(ui);
        let mut submit = false;
        let mut alternate = false;
        let mut completion_anchor = None;
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
                self.context_chips(ui);
                self.recall_width = ui.available_width();
                let completing = self.completion_active(ctx);
                self.completion_keys(ctx);
                self.recall_before_edit(ctx);
                if self.composer_formatting_available(ctx) {
                    let events_before = ctx.input(|input| input.events.len());
                    composer_text::keys(ctx, &mut self.draft);
                    if ctx.input(|input| input.events.len()) < events_before {
                        self.context_chips.clear();
                    }
                }
                let list_focus = composer_text::list_at_cursor(ctx, &self.draft);
                let smart_newline = ctx.input(|i| {
                    !i.events
                        .iter()
                        .any(|e| matches!(e, egui::Event::Paste(_) | egui::Event::Ime(_)))
                });
                let cursor_id = ui.make_persistent_id((
                    "composer-last-cursor",
                    self.state.session_file.as_deref(),
                ));
                let cursor_before = egui::TextEdit::load_state(ctx, composer_text::widget_id(ctx, "composer"))
                    .and_then(|s| s.cursor.char_range());
                let reveal_cursor = self.focus_composer
                    || ctx.data(|d| {
                        d.get_temp::<Option<egui::text::CCursorRange>>(cursor_id)
                            .is_some_and(|last| last != cursor_before)
                    });
                let mut layouter = composer_text::layout;
                // Only the editor scrolls: delivery/model controls stay below it.
                // Bound growth on short windows and at higher UI zoom as well.
                let max_height = if self.side_chat.rendering { 72.0 } else { (ctx.screen_rect().height() * 0.3).clamp(48.0, 240.0) };
                let scroll = egui::ScrollArea::vertical()
                    .id_salt(("composer-text-scroll", self.state.session_file.as_deref()))
                    .max_height(max_height)
                    // Reserve growth budget even in the bottom panel's cached short rect;
                    // auto-shrink still keeps one-line drafts compact.
                    .min_scrolled_height(max_height)
                    .auto_shrink([false, true])
                    .animated(false)
                    .show(ui, |ui| {
                        let decorations = ui.painter().add(egui::Shape::Noop);
                        let mut buffer = composer_text::Buffer {
                            text: &mut self.draft,
                            smart_newline,
                        };
                        let output = egui::TextEdit::multiline(&mut buffer)
                            .id(composer_text::widget_id(ctx, "composer"))
                            .font(FontId::proportional(14.0))
                            .layouter(&mut layouter)
                            .frame(false)
                            .lock_focus(completing || list_focus)
                            .desired_width(f32::INFINITY)
                            .desired_rows(1)
                            .return_key(egui::KeyboardShortcut::new(
                                egui::Modifiers::SHIFT,
                                egui::Key::Enter,
                            ))
                            .show(ui);
                        composer_text::decorate(ui, decorations, &output, &self.draft);
                        composer_text::links(ui, &output, &self.draft);
                        // Recall/completion restore cursor state before TextEdit runs,
                        // so its normal selection-change auto-scroll cannot detect them.
                        if reveal_cursor {
                            if let Some(range) = output.state.cursor.char_range() {
                                let caret = output
                                    .galley
                                    .pos_from_cursor(range.primary)
                                    .translate(output.galley_pos.to_vec2());
                                ui.scroll_to_rect(caret, None);
                            }
                        }
                        ctx.data_mut(|d| {
                            d.insert_temp(cursor_id, output.state.cursor.char_range())
                        });
                        #[cfg(test)]
                        if let Some(range) = output.state.cursor.char_range() {
                            let caret = output
                                .galley
                                .pos_from_cursor(range.primary)
                                .translate(output.galley_pos.to_vec2());
                            ctx.data_mut(|d| {
                                d.insert_temp(composer_text::widget_id(ctx, "composer-caret-rect"), caret)
                            });
                        }
                        output
                    });
                // egui 0.32 returns the pre-auto-shrink budget in inner_rect.
                // Anchor popups to the actual editor viewport, even for short drafts.
                let editor_rect = egui::Rect::from_min_size(
                    scroll.inner_rect.min,
                    egui::vec2(
                        scroll.inner_rect.width(),
                        scroll.inner_rect.height().min(scroll.content_size.y),
                    ),
                );
                #[cfg(test)]
                ctx.data_mut(|d| {
                    d.insert_temp(
                        composer_text::widget_id(ctx, "composer-scroll-metrics"),
                        (editor_rect, scroll.content_size, scroll.state.offset.y),
                    )
                });
                let output = scroll.inner;
                let response = &output.response;
                if response.changed() {
                    self.context_chips.clear();
                }
                // The full editor rect may extend well beyond its viewport.
                completion_anchor = Some((editor_rect, ui.layer_id()));
                if self.focus_composer {
                    response.request_focus();
                    self.focus_composer = false;
                }
                self.composer_hints(ui, &output);
                if response.has_focus()
                    && !self.attention.open
                    && !self.model_picker_open
                    && !self.effort_picker_open
                    && ctx.input(|i| i.key_pressed(egui::Key::Enter) && !i.modifiers.shift)
                {
                    submit = true;
                    alternate = ctx.input(|i| i.modifiers.command);
                }
                ui.add_space(4.0);
                let toolbar = self.composer_toolbar_layout(ui, 4.0);
                if toolbar.wrap {
                    ui.horizontal(|ui| {
                        ui.set_height(theme::COMPACT_HEIGHT);
                        theme::compact_controls(ui);
                        ui.spacing_mut().item_spacing.x = 4.0;
                        self.composer_pickers(ui, &toolbar);
                    });
                }
                ui.horizontal(|ui| {
                    ui.set_height(theme::COMPACT_HEIGHT);
                    theme::compact_controls(ui);
                    ui.spacing_mut().item_spacing.x = 4.0;
                    let attach = icons::button(
                        ui,
                        Icon::Plus,
                        "Attach files · right-click to paste an image",
                    );
                    if attach.clicked() {
                        self.pick_images();
                    }
                    attach.context_menu(|ui| {
                        theme::menu_controls(ui);
                        if ui.button("Paste image from clipboard").clicked() {
                            self.draft_images.clipboard(ctx);
                            ui.close();
                        }
                    });

                    if !toolbar.wrap {
                        self.composer_pickers(ui, &toolbar);
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
                        #[cfg(test)]
                        ctx.data_mut(|d| d.insert_temp(composer_text::widget_id(ctx, "composer-send-rect"), rect));
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
                            .on_hover_text({
                                let command = if ctx.os() == egui::os::OperatingSystem::Mac {
                                    "⌘"
                                } else {
                                    "Ctrl+"
                                };
                                if busy {
                                    format!(
                                        "{} · Enter\n{command}Enter to {}\nShift+Enter for a new line",
                                        self.prefs.settings.delivery.label(),
                                        self.prefs.settings.delivery.opposite().label().to_lowercase()
                                    )
                                } else {
                                    "Send · Enter\nShift+Enter for a new line".to_owned()
                                }
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
                                    theme::menu_controls(ui);
                                    if ui.button("Stop tree").clicked() {
                                        let active = self.agents.active.clone();
                                        self.stop_agent_tree(&active);
                                        ui.close();
                                    }
                                });
                            }
                        }
                        self.composer_context_ring(ui);
                        self.composer_mic_button(ui, ctx);
                    });
                });
            });
        if let Some((anchor, layer)) = completion_anchor {
            self.completion_popup(ctx, anchor, layer);
        }
        if submit && !self.model_picker_open && !self.effort_picker_open {
            self.dictation.stop();
            if alternate {
                self.submit_mode(ctx, self.prefs.settings.delivery.opposite());
            } else {
                self.submit(ctx);
            }
        }
    }

    fn usage_limit_banner(&mut self, ui: &mut egui::Ui) {
        if self.demo {
            return;
        }
        let Some(warning) = self.usage.warning_banner(&self.state.provider) else {
            return;
        };
        if self.dismissed_usage_warning.as_deref() == Some(warning.key.as_str()) {
            return;
        }
        let colors = crate::theme::palette(ui.ctx());
        let accent = if warning.critical {
            colors.red
        } else {
            colors.amber
        };
        let mut switch_account = false;
        let mut dismiss = false;
        egui::Frame::new()
            .fill(colors.sidebar)
            .stroke(Stroke::new(1.0, accent.linear_multiply(0.55)))
            .corner_radius(12)
            .inner_margin(egui::Margin {
                left: 12,
                right: 8,
                top: 8,
                bottom: 8,
            })
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    icons::inline(ui, Icon::Clock, accent);
                    ui.label(
                        RichText::new(&warning.title)
                            .size(11.5)
                            .strong()
                            .color(colors.text),
                    );
                    if !warning.detail.is_empty() {
                        ui.label(
                            RichText::new(&warning.detail)
                                .size(11.0)
                                .color(colors.muted),
                        );
                    }
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        let close = icons::button(ui, Icon::Close, "Dismiss usage warning");
                        if close.clicked() {
                            dismiss = true;
                        }
                        if ui
                            .add(
                                egui::Button::new(
                                    RichText::new("Switch account")
                                        .size(11.0)
                                        .color(colors.text),
                                )
                                .fill(colors.surface)
                                .stroke(Stroke::new(1.0, colors.border))
                                .corner_radius(6),
                            )
                            .on_hover_text(format!(
                                "Open Providers settings ({})",
                                warning.provider_name
                            ))
                            .clicked()
                        {
                            switch_account = true;
                        }
                    });
                });
            });
        ui.add_space(6.0);
        if dismiss {
            self.dismissed_usage_warning = Some(warning.key);
        }
        if switch_account {
            self.settings_view.select("Providers");
            self.settings_open = true;
        }
    }

    fn composer_mic_button(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        let colors = crate::theme::palette(ui.ctx());
        let listening = self.dictation.active();
        let (rect, response) = ui.allocate_exact_size(Vec2::splat(24.0), Sense::click());
        let response = response.on_hover_cursor(egui::CursorIcon::PointingHand);
        if listening {
            ui.painter()
                .rect_filled(rect, 6, colors.red.linear_multiply(0.22));
            ui.painter().rect_stroke(
                rect,
                6,
                Stroke::new(1.0_f32, colors.red),
                egui::StrokeKind::Inside,
            );
        } else if response.hovered() {
            ui.painter().rect_filled(rect, 6, colors.hover);
        }
        icons::draw(
            ui,
            rect,
            Icon::Mic,
            if listening {
                colors.red
            } else if response.hovered() {
                colors.text
            } else {
                colors.muted
            },
        );
        response.widget_info(|| {
            egui::WidgetInfo::labeled(
                egui::WidgetType::Button,
                !self.demo,
                if listening {
                    "Stop voice dictation"
                } else {
                    "Start voice dictation"
                },
            )
        });
        let tip = if listening {
            "Listening… click to stop dictation"
        } else {
            "Voice dictation (macOS Speech Recognition)"
        };
        if response.on_hover_text(tip).clicked() && !self.demo {
            if let Err(err) = self.dictation.toggle(ctx, &self.draft) {
                self.state.notice = Some(err);
            } else {
                self.focus_composer = true;
            }
        }
    }

    fn composer_context_ring(&mut self, ui: &mut egui::Ui) {
        let fraction = if self.demo {
            Some(0.45_f32)
        } else {
            crate::context::usage_fraction(&self.state)
        };
        let Some(fraction) = fraction else {
            return;
        };
        let colors = crate::theme::palette(ui.ctx());
        let compact_threshold =
            (self.prefs.settings.compact_percent as f32 / 100.0).clamp(0.1, 0.99);
        let ring_color = if fraction >= 0.90 {
            colors.red
        } else if fraction >= compact_threshold {
            colors.amber
        } else {
            colors.accent
        };

        let size = Vec2::splat(24.0);
        let (rect, response) = ui.allocate_exact_size(size, Sense::click());
        let response = response.on_hover_cursor(egui::CursorIcon::PointingHand);
        if response.hovered() {
            ui.painter().rect_filled(rect, 6, colors.hover);
        }

        let center = rect.center();
        let radius = 7.5_f32;
        let stroke_w = 2.0_f32;
        ui.painter()
            .circle_stroke(center, radius, Stroke::new(stroke_w, colors.border));

        if fraction > 0.005 {
            let sweep = fraction.clamp(0.0, 1.0) * std::f32::consts::TAU;
            let segments = ((sweep / std::f32::consts::TAU) * 28.0).ceil().max(4.0) as usize;
            let start = -std::f32::consts::FRAC_PI_2;
            let points: Vec<egui::Pos2> = (0..=segments)
                .map(|i| {
                    let t = i as f32 / segments as f32;
                    let angle = start + sweep * t;
                    egui::pos2(
                        center.x + radius * angle.cos(),
                        center.y + radius * angle.sin(),
                    )
                })
                .collect();
            ui.painter()
                .add(egui::Shape::line(points, Stroke::new(stroke_w, ring_color)));
        }

        let pct = (fraction * 100.0).round() as u32;
        let tooltip = if self.demo {
            "Context: 45% (450k / 1.0m) · Click to inspect".into()
        } else {
            let used = self
                .state
                .context_tokens
                .or_else(|| self.state.context_breakdown.as_ref().map(|p| p.total()));
            match used {
                Some(u) if self.state.context_window > 0.0 => format!(
                    "Context: {pct}% ({} / {}) · Click to inspect · Right-click to compact",
                    crate::context::number(u),
                    crate::context::number(self.state.context_window)
                ),
                _ => format!("Context: {pct}% · Click to inspect"),
            }
        };

        response.widget_info(|| {
            egui::WidgetInfo::labeled(
                egui::WidgetType::Button,
                true,
                format!("Context usage {pct}%"),
            )
        });
        let response = response.on_hover_text(tooltip);
        if response.clicked() {
            self.prefs.hide_inspector = false;
            self.prefs.cards.hidden.remove(&inspector::Card::Context);
            self.prefs.cards.collapsed.remove(&inspector::Card::Context);
        }
        response.context_menu(|ui| {
            theme::menu_controls(ui);
            if ui.button("Inspect context").clicked() {
                self.prefs.hide_inspector = false;
                self.prefs.cards.hidden.remove(&inspector::Card::Context);
                self.prefs.cards.collapsed.remove(&inspector::Card::Context);
                ui.close();
            }
            if !self.demo
                && ui
                    .add_enabled(
                        self.connected && self.state.ready(),
                        egui::Button::new("Compact context"),
                    )
                    .clicked()
            {
                self.send(protocol::command("compact"));
                ui.close();
            }
        });
    }

    fn cancel_git_open_for_file_navigation(&mut self) {
        if self.files.is_open() {
            // Navigation wins permanently, not just until the pane is hidden.
            // Covers toolbar, menus, slash commands, history, links and Pui open.
            self.diff_rx = None;
        }
    }

    fn open_diff_review(&mut self, document: diff_viewer::Document, recorded: bool) {
        self.diff_rx = None; // explicit replacement wins over any earlier working-tree open
        if recorded && self.output_revision != self.state.outputs_revision {
            self.outputs = activity::recorded_files(&self.state.messages);
            self.output_revision = self.state.outputs_revision;
        }
        let review_owner = self.review_comment_owner();
        self.changes.open_review(
            &self.ctx,
            document,
            recorded,
            &self.prefs.cwd,
            &review_owner,
        );
        self.changes
            .bind_review_files(&self.outputs, self.output_revision);
        if !recorded {
            self.changes.bind_git_files(&self.workspace);
        }
        self.inspector_panel = true;
        self.files.close();
        self.settings_open = false;
        self.kanban.open = false;
        self.automation.open = false;
        self.focus_composer = false;
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
            created: Some(now - Duration::from_secs((i as u64 + 1) * 86400)),
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
    fn update(&mut self, ctx: &egui::Context, frame: &mut eframe::Frame) {
        self.update_idle.observe(ctx);
        // Apply recovered conditional keys to real storage, not just startup reads.
        if let Some(recovery) = self.update_recovery.take() {
            if let Some(storage) = frame.storage_mut() {
                recovery.copy_to(storage);
            }
            // The snapshot is deleted only at a later launch after disk acknowledgement.
        }
        crate::chromium::Runtime::pump(ctx);
        self.files.begin_frame();
        let side_keyboard = self.side_chat_owns_keyboard(ctx);
        ctx.data_mut(|data| {
            data.insert_temp(
                egui::Id::new("side-chat-main-keyboard-blocked"),
                side_keyboard,
            )
        });
        self.prefs.color_styles.apply(ctx, self.prefs.theme);
        self.frames += 1;
        self.remember_navigation();
        self.poll();
        self.sync_model_scope();
        self.panel_resize_inputs(ctx);
        self.chat_section_drag_inputs(ctx);
        self.section_drag_inputs(ctx);
        self.attention_inputs(ctx);
        let attention_was_open = self.attention.open;
        self.attention_panel(ctx);
        let attention_layout_deferred = self.attention.defer_layout;
        self.search_jump();
        self.model_picker_escape(ctx);
        context_capture::inputs(ctx);
        self.context_chip_inputs(ctx);
        self.checkpoint_inputs(ctx);
        let review_owner = self.review_comment_owner();
        self.changes
            .bind_inline_owner(ctx, &self.prefs.cwd, &review_owner);
        self.changes.index_writable =
            !self.demo && !self.is_mirror() && !self.readonly_worker() && !self.agents.exiting;
        self.changes.review_inputs(
            ctx,
            self.inspector_panel
                && !self.files.is_open()
                && !self.settings_open
                && !self.search_open
                && !self.tasks.open
                && !self.kanban.open
                && !self.automation.open
                && self.terminals.layout() != (true, true)
                && self.question_designs.is_none()
                && !self.transcript.gallery.is_open()
                && !self.draft_images.gallery.is_open(),
            &self.outputs,
            self.output_revision,
            &self.workspace,
        );
        self.files.context_inputs(
            ctx,
            !self.settings_open
                && !self.search_open
                && !self.kanban.open
                && !self.automation.open
                && !self.tasks.open
                && self.terminals.layout() != (true, true)
                && !self.transcript.gallery.is_open()
                && !self.draft_images.gallery.is_open(),
            !self.search_blocked(),
        );
        if let Some(command) = crate::markdown::take_terminal_command(ctx) {
            self.terminals
                .run_command(ctx, &self.prefs.cwd, self.demo, &command);
        }
        self.terminals.context_inputs(
            ctx,
            !self.settings_open && !self.search_open && !self.kanban.open && !self.automation.open,
        );
        self.sidebar_view_inputs(ctx);
        self.search_inputs(ctx);
        self.navigation_inputs(ctx);
        if !self.attention.open {
            self.menu_commands(ctx);
        }
        if self.screenshot.is_none() || self.agents.close_prompt {
            self.agent_close_guard(ctx);
        }
        let confirming = self.changes.review_comments.confirming()
            || self.section_view.confirming()
            || self.tasks.attachment_confirming()
            || self.task_operations_confirming()
            || self.agents.close_prompt
            || self.terminals.confirming_stop()
            || self.history_view.is_open()
            || self.deletion.confirming()
            || self.menu.about
            || self.resource_reload.confirming()
            || self.updates.open
            || self.settings_view.providers.any_modal_open()
            || self.search_open
            || self.attention.open;
        let terminal_input = !self.settings_open
            && !self.kanban.open
            && !self.automation.open
            && self.terminals.focused(ctx);
        if !confirming
            && !self.settings_open
            && !self.kanban.open
            && !self.tasks.open
            && !self.automation.open
            && !terminal_input
            && !self.side_chat_image_inputs(ctx)
        {
            self.image_inputs(ctx);
        }
        let side_theater = self.side_chat_gallery_viewer(ctx);
        let theater = side_theater
            || self.transcript.gallery.is_open()
            || self.draft_images.gallery.is_open();
        if theater {
            self.close_model_popovers();
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
        if !self.attention.open
            && ctx.input_mut(|i| i.consume_key(egui::Modifiers::COMMAND, egui::Key::Comma))
        {
            self.settings_open = !self.settings_open;
            self.close_model_popovers();
        }
        let composer_formatting =
            self.composer_formatting_available(ctx) || self.side_chat_owns_keyboard(ctx);
        if !self.attention.open
            && !self.settings_open
            && !composer_formatting
            && (!terminal_input || cfg!(target_os = "macos"))
            && ctx.input_mut(|i| i.consume_key(egui::Modifiers::COMMAND, egui::Key::B))
        {
            self.prefs.hide_sidebar = !self.prefs.hide_sidebar;
        }
        if !self.attention.open
            && !self.settings_open
            && !composer_formatting
            && (!terminal_input || cfg!(target_os = "macos"))
            && ctx.input_mut(|i| i.consume_key(egui::Modifiers::COMMAND, egui::Key::I))
        {
            self.kanban.open = false;
            self.automation.open = false;
            self.files.close();
            self.inspector_panel = false;
            self.prefs.hide_inspector = !self.prefs.hide_inspector;
        }
        if !self.settings_open
            && !confirming
            && (!terminal_input || cfg!(target_os = "macos"))
            && ctx.input_mut(|i| i.consume_key(egui::Modifiers::COMMAND, egui::Key::N))
        {
            self.new_session();
        }
        let searching = self.search_open;
        let leaving_board = !searching
            && !confirming
            && !self.settings_open
            && (self.kanban.open || self.automation.open)
            && ctx.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::Escape));
        if leaving_board {
            self.kanban.open = false;
            self.automation.open = false;
            self.focus_composer = true;
        }
        if !leaving_board
            && !self.model_picker_open
            && !self.effort_picker_open
            && !searching
            && !self.settings_open
            && !self.automation.open
            && !self.kanban.open
            && !confirming
            && !terminal_input
            && !self.side_chat_owns_keyboard(ctx)
            && !self.files.focused(ctx)
            && !self.diagnostics_open
            && self.rename_chat.is_none()
            && !self.section_view.editing()
            && self.state.dialogs.is_empty()
            && !self.completion_active(ctx)
            && ctx.input(|i| i.key_pressed(egui::Key::Escape))
            && !self.transcript.gallery.is_open()
            && !self.draft_images.gallery.is_open()
            && !theater
            && ((!self.tasks.open && !self.state.ready())
                || (self.tasks.open && self.tasks.running()))
        {
            self.stop();
        }
        #[cfg(not(target_os = "macos"))]
        self.fallback_menu_bar(ctx);
        if let Some(preview) = self.question_designs.as_mut() {
            preview.show(ctx);
        } else if self.settings_open {
            self.close_model_popovers();
            self.session_settings(ctx);
        } else {
            if !self.prefs.hide_sidebar {
                self.sidebar(ctx);
            }
            self.top_bar(ctx);
            self.side_chat_bar(ctx);
            if !attention_was_open
                || self.attention.popup_rect.is_none()
                || attention_layout_deferred
            {
                self.attention_panel(ctx);
            }
            let terminal_expanded = !self.kanban.open
                && !self.automation.open
                && self.terminals.is_open()
                && self.terminals.show(ctx, &self.prefs.cwd, self.demo);
            if self.terminals.focused(ctx) {
                self.focus_composer = false;
            }
            if self.files.is_open() {
                context_capture::browser::invalidate_changed(self);
                self.prepare_file_block_capture(ctx);
                let capture_owner = self.review_comment_owner();
                context_capture::bind_file_owner(ctx, &self.prefs.cwd, &capture_owner);
                self.files
                    .bind_browser_owner(&self.prefs.cwd, &capture_owner);
            }
            let side_chat_workspace = self.side_chat_workspace(ctx);
            let central = if side_chat_workspace {
                false
            } else if self.automation.open {
                self.automation_panel(ctx);
                true
            } else if self.kanban.open {
                self.agents_board(ctx);
                true
            } else if terminal_expanded {
                true
            } else if self.files.is_open() {
                self.files
                    .show(ctx, &self.prefs.cwd, &mut self.prefs.table_headers)
            } else if self.inspector_panel {
                let review_owner = self.review_comment_owner();
                self.changes.show(
                    ctx,
                    &self.prefs.cwd,
                    &review_owner,
                    &self.workspace,
                    (&self.outputs, self.output_revision),
                    self.workspace_rx.is_some(),
                )
            } else {
                if !self.prefs.hide_inspector && ctx.screen_rect().width() > 1110.0 {
                    self.inspector(ctx);
                }
                false
            };
            if self.changes.take_close() {
                self.inspector_panel = false;
                context_capture::clear_source(ctx);
            }
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
            if !central
                && (!self.tasks.open
                    || self
                        .tasks
                        .current()
                        .is_some_and(|task| self.task_in_chat(task)))
                && !confirming
                && !theater
            {
                self.model_picker(ctx);
            } else {
                self.close_model_popovers();
            }
            self.history_confirmation(ctx);
        }
        self.pi_folder_prompt(ctx);
        if !self.settings_open {
            self.settings_view.providers.show_modals(ctx);
        }
        if !theater {
            self.transcript.gallery.viewer(ctx);
            self.draft_images.gallery.viewer(ctx);
        }
        if self.web_preview_allowed(theater, confirming) {
            self.files.show_web(frame, ctx);
        } else {
            self.files.hide_web();
        }
        self.collect_context_capture();
        self.process_open_links(ctx);
        self.deletion_dialog(ctx);
        self.sidebar_section_confirmation(ctx);
        self.task_attachment_confirmation(ctx);
        self.task_operation_confirmation(ctx);
        self.spotlight(ctx);
        self.task_watch_source();
        self.menu_windows(ctx);
        self.cancel_git_open_for_file_navigation();
        self.remember_navigation();
        self.refresh_menu(ctx);
        self.update_attention_dock();
        self.maybe_install_update(ctx, frame);
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
            self.checkpoint_chats();
            self.save_workspace_layouts();
            self.changes.review_comments.save_checkpoint(storage);
            if let Some(wrap) = diff_viewer::wrap_preference::get(&self.ctx) {
                self.prefs.diff_wrap = wrap;
            }
            if let Some(cached) = self.usage.cached() {
                self.prefs.usage_cache = Some(cached);
            }
            eframe::set_value(
                storage,
                "attention-v1",
                &attention::Stored(self.attention.persisted()),
            );
            self.prefs.agent_tree = self.agent_records();
            self.prefs.chats.retain(|_, meta| !meta.is_empty());
            self.prefs.projects_view = self.projects;
            self.prefs.last_session = self.pending_session.clone().or_else(|| {
                (!self.new_chat)
                    .then(|| self.state.session_file.as_ref().map(PathBuf::from))
                    .flatten()
            });
            // An in-progress New section is a draft, not durable UI state.
            let draft_section = self
                .section_view
                .creating()
                .and_then(|id| {
                    self.prefs
                        .sidebar_view
                        .sections
                        .iter()
                        .position(|section| section.id == id)
                })
                .map(|index| (index, self.prefs.sidebar_view.sections.remove(index)));
            eframe::set_value(storage, "preferences", &self.prefs);
            if let Some((index, section)) = draft_section {
                self.prefs.sidebar_view.sections.insert(index, section);
            }
        }
    }
    fn auto_save_interval(&self) -> Duration {
        Duration::from_secs(10)
    }
    fn on_exit(&mut self, _gl: Option<&eframe::glow::Context>) {
        self.attention_dock.release();
        self.workspace_rx = None;
        self.diff_rx = None;
        self.changes.cancel_git_read();
        self.changes.cancel_inline_reads();
        self.changes.close_branches(&self.ctx);
        self.agents.exiting = true;
        self.terminals.shutdown();
        self.tasks.shutdown();
        self.automation.running_stop();
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
