#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod accounts;
mod activity;
mod app;
mod chromium;
mod context;
mod file_watch;
mod history;
mod icons;
mod inline_visual;
mod inspector;
mod markdown;
mod media;
mod pi_migration;
mod protocol;
mod pulls;
mod rpc;
mod runtime;
mod section;
mod sessions;
mod svg_preview;
mod task_output;
mod tasks;
mod terminal;
mod theme;
mod tool_view;
mod transcript;
mod usage;
mod workspace;

use std::path::PathBuf;

#[derive(Default)]
pub struct Options {
    pub demo: bool,
    pub demo_languages: bool,
    pub demo_markdown: bool,
    pub demo_quote_selection: bool,
    pub demo_context_chips: bool,
    pub demo_expanded: bool,
    pub demo_collapsed: bool,
    pub demo_theater: bool,
    pub demo_model_picker: bool,
    pub demo_effort_picker: bool,
    pub demo_search: bool,
    pub demo_search_attention: bool,
    pub demo_quick_open: bool,
    pub demo_sections: bool,
    pub demo_view_menu: bool,
    pub demo_compacting: bool,
    pub demo_working: bool,
    pub demo_settings: bool,
    pub settings: bool,
    pub settings_page: Option<String>,
    pub demo_about: bool,
    pub demo_updates: bool,
    pub demo_approval: bool,
    pub demo_questions: bool,
    pub demo_question_designs: bool,
    pub demo_agents: bool,
    pub demo_sidebar_density: bool,
    pub demo_kanban: bool,
    pub demo_automation: bool,
    pub demo_task: bool,
    pub demo_task_proposal: bool,
    pub demo_files: bool,
    pub demo_table_grid: bool,
    pub demo_side_chat: bool,
    pub demo_table_projection: bool,
    pub demo_inline_review: bool,
    pub demo_table_headerless: bool,
    pub demo_preview_budget: bool,
    pub demo_preview_queue: bool,
    pub demo_source_context: bool,
    pub demo_changes: bool,
    pub demo_branches: bool,
    pub demo_git_history: bool,
    pub demo_history_filter: bool,
    pub demo_commit_detail: bool,
    pub demo_diff_context: bool,
    pub demo_diff_hunks: bool,
    pub demo_diff_wrap: bool,
    pub demo_review_files: bool,
    pub demo_review_comments: bool,
    pub demo_review_note_delete: bool,
    pub demo_review_note_edit: bool,
    pub demo_review_note_files: bool,
    pub demo_git_files: bool,
    pub demo_terminal: bool,
    pub demo_terminal_tabs: bool,
    pub demo_terminal_context: bool,
    pub demo_notifications: bool,
    pub demo_attention: bool,
    pub demo_attention_dock: bool,
    pub open: Option<String>,
    pub browser_device: Option<String>,
    pub browser_console: bool,
    pub demo_confirmation: bool,
    pub demo_send_now: bool,
    pub cwd: Option<PathBuf>,
    pub session: Option<PathBuf>,
    pub screenshot: Option<PathBuf>,
    pub theme: Option<theme::Theme>,
}

fn app_icon() -> eframe::egui::IconData {
    let image = image::load_from_memory(include_bytes!("../assets/icon.png"))
        .expect("bundled app icon is a valid PNG")
        .into_rgba8();
    eframe::egui::IconData {
        width: image.width(),
        height: image.height(),
        rgba: image.into_raw(),
    }
}

fn main() -> eframe::Result {
    // Diagnostics only: no GUI, agent prompt, network or provider credentials.
    if std::env::args().nth(1).as_deref() == Some("--runtime-check") {
        if let Err(error) = runtime::check() {
            eprintln!("{error}");
            std::process::exit(1);
        }
        println!("Bundled Node/SDK subprocess check passed.");
        return Ok(());
    }
    if std::env::args().nth(1).as_deref() == Some("--runtime-info") {
        println!(
            "{}",
            serde_json::json!({
                "bundled": runtime::bundled(), "pi": runtime::pi_binary(),
                "node": runtime::node_binary(), "sdk": runtime::sdk(),
            })
        );
        return Ok(());
    }
    if svg_preview::worker_requested() {
        std::process::exit(svg_preview::worker_main());
    }
    let mut options = Options::default();
    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--demo" => options.demo = true,
            "--demo-search" => {
                options.demo = true;
                options.demo_search = true;
            }
            "--demo-search-attention" => {
                options.demo = true;
                options.demo_search_attention = true;
            }
            "--demo-quick-open" => {
                options.demo = true;
                options.demo_quick_open = true;
            }
            "--demo-sections" => {
                options.demo = true;
                options.demo_sections = true;
            }
            "--demo-view-menu" => {
                options.demo = true;
                options.demo_view_menu = true;
            }
            "--demo-languages" => {
                options.demo = true;
                options.demo_languages = true;
            }
            "--demo-expanded" => {
                options.demo = true;
                options.demo_expanded = true;
            }
            "--demo-collapsed" => {
                options.demo = true;
                options.demo_collapsed = true;
            }
            "--demo-files" => {
                options.demo = true;
                options.demo_files = true;
            }
            "--demo-preview-queue" => {
                options.demo = true;
                options.demo_files = true;
                options.demo_preview_queue = true;
            }
            "--demo-preview-budget" => {
                options.demo = true;
                options.demo_files = true;
                options.demo_preview_budget = true;
            }
            "--demo-side-chat" => {
                options.demo = true;
                options.demo_side_chat = true;
            }
            "--demo-table-grid" => {
                options.demo = true;
                options.demo_files = true;
                options.demo_table_grid = true;
            }
            "--demo-table-projection" => {
                options.demo = true;
                options.demo_files = true;
                options.demo_table_grid = true;
                options.demo_table_projection = true;
            }
            "--demo-inline-review" => {
                options.demo = true;
                options.demo_changes = true;
                options.demo_inline_review = true;
            }
            "--demo-table-headerless" => {
                options.demo = true;
                options.demo_files = true;
                options.demo_table_grid = true;
                options.demo_table_headerless = true;
            }
            "--demo-source-context" => {
                options.demo = true;
                options.demo_files = true;
                options.demo_source_context = true;
            }
            "--demo-confirmation" => {
                options.demo = true;
                options.demo_confirmation = true;
            }
            "--demo-notifications" => {
                options.demo = true;
                options.demo_notifications = true;
            }
            "--demo-attention" | "--demo-attention-dock" => {
                options.demo = true;
                options.demo_attention = true;
                options.demo_attention_dock |= arg == "--demo-attention-dock";
            }
            "--demo-send-now" => {
                options.demo = true;
                options.demo_send_now = true;
            }
            "--demo-terminal-tabs" => {
                options.demo = true;
                options.demo_terminal = true;
                options.demo_terminal_tabs = true;
            }
            "--demo-terminal" => {
                options.demo = true;
                options.demo_terminal = true;
            }
            "--demo-terminal-context" => {
                options.demo = true;
                options.demo_terminal = true;
                options.demo_terminal_context = true;
            }
            "--demo-changes" => {
                options.demo = true;
                options.demo_changes = true;
            }
            "--demo-branches"
            | "--demo-git-history"
            | "--demo-commit-detail"
            | "--demo-history-filter" => {
                options.demo = true;
                options.demo_changes = true;
                options.demo_branches = true;
                options.demo_git_history |= arg != "--demo-branches";
                options.demo_commit_detail |= arg == "--demo-commit-detail";
                options.demo_history_filter |= arg == "--demo-history-filter";
            }
            "--demo-git-files" => {
                options.demo = true;
                options.demo_changes = true;
                options.demo_git_files = true;
            }
            "--demo-review-files" => {
                options.demo = true;
                options.demo_changes = true;
                options.demo_review_files = true;
            }
            "--demo-review-comments"
            | "--demo-review-note-delete"
            | "--demo-review-note-edit"
            | "--demo-review-note-files" => {
                options.demo = true;
                options.demo_changes = true;
                options.demo_review_files = true;
                options.demo_review_comments = true;
                options.demo_review_note_delete |= arg == "--demo-review-note-delete";
                options.demo_review_note_edit |= arg == "--demo-review-note-edit";
                options.demo_review_note_files |= arg == "--demo-review-note-files";
            }
            "--demo-diff-wrap" => {
                options.demo = true;
                options.demo_diff_hunks = true;
                options.demo_diff_wrap = true;
            }
            "--demo-diff-hunks" => {
                options.demo = true;
                options.demo_diff_hunks = true;
            }
            "--demo-diff-context" => {
                options.demo = true;
                options.demo_diff_context = true;
            }
            "--demo-theater" => {
                options.demo = true;
                options.demo_theater = true;
            }
            "--demo-task" => {
                options.demo = true;
                options.demo_task = true;
            }
            "--demo-automation" => {
                options.demo = true;
                options.demo_automation = true;
            }
            "--demo-kanban" => {
                options.demo = true;
                options.demo_kanban = true;
            }
            "--demo-task-proposal" => {
                options.demo = true;
                options.demo_task = true;
                options.demo_task_proposal = true;
            }
            "--demo-sidebar-density" => {
                options.demo = true;
                options.demo_agents = true;
                options.demo_sidebar_density = true;
            }
            "--demo-agents" => {
                options.demo = true;
                options.demo_agents = true;
                options.demo_model_picker = true;
            }
            "--demo-effort-picker" => {
                options.demo = true;
                options.demo_effort_picker = true;
            }
            "--demo-model-picker" => {
                options.demo = true;
                options.demo_model_picker = true;
            }
            "--demo-settings" => {
                options.demo = true;
                options.demo_settings = true;
                options.demo_compacting = true;
            }
            "--settings" => options.settings = true,
            "--settings-page" => {
                options.settings = true;
                options.settings_page = args.next();
            }
            "--demo-question-designs" => {
                options.demo = true;
                options.demo_question_designs = true;
            }
            "--demo-questions" => {
                options.demo = true;
                options.demo_questions = true;
            }
            "--demo-about" => {
                options.demo = true;
                options.demo_about = true;
            }
            "--demo-updates" => {
                options.demo = true;
                options.demo_updates = true;
            }
            "--demo-approval" => {
                options.demo = true;
                options.demo_approval = true;
            }
            "--demo-markdown" => {
                options.demo = true;
                options.demo_markdown = true;
            }
            "--demo-context-chips" => {
                options.demo = true;
                options.demo_context_chips = true;
            }
            "--demo-quote-selection" => {
                options.demo = true;
                options.demo_quote_selection = true;
            }
            "--demo-working" => {
                options.demo = true;
                options.demo_working = true;
            }
            "--demo-compacting" => {
                options.demo = true;
                options.demo_compacting = true;
            }
            "--light" => options.theme = Some(theme::Theme::Light),
            "--dark" => options.theme = Some(theme::Theme::Dark),
            "--open" => options.open = args.next(),
            "--browser-device" => options.browser_device = args.next(),
            "--browser-console" => options.browser_console = true,
            "--cwd" => options.cwd = args.next().map(PathBuf::from),
            "--session" => options.session = args.next().map(PathBuf::from),
            "--screenshot" => options.screenshot = args.next().map(PathBuf::from),
            "--help" | "-h" => {
                println!(
                    "Pui — a native desktop UI for Pi\n\nUsage: pui [--demo | --demo-expanded | --demo-collapsed] [--light | --dark] [--cwd PATH] [--session PATH] [--open PATH_OR_URL] [--screenshot PATH]\n\nPackaged macOS apps include Pi and Node.js; configure your model provider to begin. Development builds need Pi and Node on PATH. Set PUI_PI_BIN (and PUI_PI_SDK when needed) to use an external Pi."
                );
                return Ok(());
            }
            _ => {
                eprintln!("Unknown argument: {arg}. Use --help.");
                return Ok(());
            }
        }
    }
    chromium::prepare_application();
    let native = eframe::NativeOptions {
        viewport: eframe::egui::ViewportBuilder::default()
            .with_title(if options.demo_question_designs {
                "Pui · Question design playground"
            } else {
                "Pui"
            })
            .with_icon(app_icon())
            .with_inner_size([1480.0, 960.0])
            .with_min_inner_size([860.0, 620.0])
            .with_titlebar_shown(false)
            .with_title_shown(false)
            .with_fullsize_content_view(true),
        ..Default::default()
    };
    eframe::run_native(
        "Pui",
        native,
        Box::new(move |cc| Ok(Box::new(app::Pui::new(cc, options)))),
    )
}
