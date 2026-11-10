#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod activity;
mod app;
mod chromium;
mod context;
mod history;
mod icons;
mod inspector;
mod markdown;
mod media;
mod protocol;
mod pulls;
mod rpc;
mod section;
mod sessions;
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
    pub demo_expanded: bool,
    pub demo_collapsed: bool,
    pub demo_theater: bool,
    pub demo_model_picker: bool,
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
    pub demo_kanban: bool,
    pub demo_automation: bool,
    pub demo_task: bool,
    pub demo_task_proposal: bool,
    pub demo_files: bool,
    pub demo_changes: bool,
    pub demo_terminal: bool,
    pub demo_notifications: bool,
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
    let mut options = Options::default();
    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--demo" => options.demo = true,
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
            "--demo-confirmation" => {
                options.demo = true;
                options.demo_confirmation = true;
            }
            "--demo-notifications" => {
                options.demo = true;
                options.demo_notifications = true;
            }
            "--demo-send-now" => {
                options.demo = true;
                options.demo_send_now = true;
            }
            "--demo-terminal" => {
                options.demo = true;
                options.demo_terminal = true;
            }
            "--demo-changes" => {
                options.demo = true;
                options.demo_changes = true;
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
            "--demo-agents" => {
                options.demo = true;
                options.demo_agents = true;
                options.demo_model_picker = true;
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
                    "Pui — a native desktop UI for Pi\n\nUsage: pui [--demo | --demo-expanded | --demo-collapsed] [--light | --dark] [--cwd PATH] [--session PATH] [--open PATH_OR_URL] [--screenshot PATH]\n\nPi must be installed and authenticated separately. Set PUI_PI_BIN to override its executable."
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
