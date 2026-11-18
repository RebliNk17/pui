//! One menu definition for AppKit and the non-macOS menu bar. No placeholder actions.
use super::*;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(isize)]
pub(super) enum Command {
    About = 1,
    Updates,
    Settings,
    NewChat,
    NewWindow,
    OpenProject,
    Close,
    Quit,
    Undo,
    Redo,
    Cut,
    Copy,
    Paste,
    SelectAll,
    Search,
    Sidebar,
    Inspector,
    Files,
    Changes,
    ZoomReset,
    ZoomIn,
    ZoomOut,
    Fullscreen,
    Minimize,
    Maximize,
    Shortcuts,
    DarkTheme,
    LightTheme,
    CompletionSound,
    DesktopNotification,
    Steer,
    Queue,
    Terminal,
    Browser,
    AgentsBoard,
    Automation,
    QuickOpen,
    Bold,
    Italic,
    Underline,
    TranscriptNormal,
    TranscriptVerbose,
}
impl Command {
    #[cfg(any(target_os = "macos", test))]
    pub const ALL: &'static [Self] = &[
        Self::About,
        Self::Updates,
        Self::Settings,
        Self::NewChat,
        Self::NewWindow,
        Self::OpenProject,
        Self::Close,
        Self::Quit,
        Self::Undo,
        Self::Redo,
        Self::Cut,
        Self::Copy,
        Self::Paste,
        Self::SelectAll,
        Self::Search,
        Self::Sidebar,
        Self::Inspector,
        Self::Files,
        Self::Changes,
        Self::ZoomReset,
        Self::ZoomIn,
        Self::ZoomOut,
        Self::Fullscreen,
        Self::Minimize,
        Self::Maximize,
        Self::Shortcuts,
        Self::DarkTheme,
        Self::LightTheme,
        Self::CompletionSound,
        Self::DesktopNotification,
        Self::Steer,
        Self::Queue,
        Self::Terminal,
        Self::Browser,
        Self::AgentsBoard,
        Self::Automation,
        Self::QuickOpen,
        Self::Bold,
        Self::Italic,
        Self::Underline,
        Self::TranscriptNormal,
        Self::TranscriptVerbose,
    ];
    #[cfg(any(target_os = "macos", test))]
    pub fn from_tag(tag: isize) -> Option<Self> {
        Self::ALL.iter().copied().find(|c| *c as isize == tag)
    }
    pub fn supported(self) -> bool {
        match self {
            Self::Updates => updates::configured(),
            Self::CompletionSound | Self::DesktopNotification => {
                notifications::Notifications::supported()
            }
            _ => true,
        }
    }
    pub fn shortcut(self) -> Option<egui::KeyboardShortcut> {
        use egui::{Key, Modifiers as M};
        let (modifiers, key) = match self {
            Self::Settings => (M::COMMAND, Key::Comma),
            Self::NewChat => (M::COMMAND, Key::N),
            Self::NewWindow => (M::COMMAND | M::SHIFT, Key::N),
            Self::OpenProject => (M::COMMAND, Key::O),
            Self::Close => (M::COMMAND, Key::W),
            Self::Quit => (M::COMMAND, Key::Q),
            Self::Undo => (M::COMMAND, Key::Z),
            Self::Redo => (M::COMMAND | M::SHIFT, Key::Z),
            Self::Cut => (M::COMMAND, Key::X),
            Self::Copy => (M::COMMAND, Key::C),
            Self::Paste => (M::COMMAND, Key::V),
            Self::SelectAll => (M::COMMAND, Key::A),
            Self::Search => (M::COMMAND, Key::K),
            Self::QuickOpen => (M::COMMAND, Key::P),
            Self::Sidebar => (M::COMMAND, Key::B),
            Self::Inspector => (M::COMMAND, Key::I),
            Self::Bold => (M::COMMAND, Key::B),
            Self::Italic => (M::COMMAND, Key::I),
            Self::Underline => (M::COMMAND, Key::U),
            Self::ZoomReset => (M::COMMAND, Key::Num0),
            Self::ZoomIn => (M::COMMAND, Key::Plus),
            Self::ZoomOut => (M::COMMAND, Key::Minus),
            Self::Fullscreen if cfg!(target_os = "macos") => (M::COMMAND | M::CTRL, Key::F),
            Self::Minimize => (M::COMMAND, Key::M),
            _ => return None,
        };
        Some(egui::KeyboardShortcut::new(modifiers, key))
    }
    pub fn contextual_shortcut(self, composer: bool) -> Option<egui::KeyboardShortcut> {
        if (composer && matches!(self, Self::Sidebar | Self::Inspector))
            || (!composer && matches!(self, Self::Bold | Self::Italic | Self::Underline))
        {
            None
        } else {
            self.shortcut()
        }
    }
}

pub(super) enum Entry {
    Action(&'static str, Command),
    Submenu(&'static str, &'static [Entry]),
    Separator,
}
impl Entry {
    pub fn supported(&self) -> bool {
        match self {
            Self::Action(_, command) => command.supported(),
            Self::Submenu(_, entries) => entries
                .iter()
                .any(|e| !matches!(e, Self::Separator) && e.supported()),
            Self::Separator => true,
        }
    }
}
use Entry::{Action as A, Separator as S, Submenu as M};
pub(super) const APP: &[Entry] = &[
    A("About Pui", Command::About),
    A("Check for Updates…", Command::Updates),
    S,
    A("Settings…", Command::Settings),
    M(
        "Completion Alerts",
        &[
            A("Completion Sound", Command::CompletionSound),
            A("Desktop Notifications", Command::DesktopNotification),
        ],
    ),
    M(
        "Default Message Delivery",
        &[A("Steer", Command::Steer), A("Queue", Command::Queue)],
    ),
];
pub(super) const GROUPS: &[(&str, &[Entry])] = &[
    (
        "File",
        &[
            A("New Chat", Command::NewChat),
            A("New Window", Command::NewWindow),
            S,
            A("Open Project…", Command::OpenProject),
            S,
            A("Close Window", Command::Close),
        ],
    ),
    (
        "Edit",
        &[
            A("Undo", Command::Undo),
            A("Redo", Command::Redo),
            S,
            A("Cut", Command::Cut),
            A("Copy", Command::Copy),
            A("Paste", Command::Paste),
            A("Select All", Command::SelectAll),
            S,
            A("Bold", Command::Bold),
            A("Italic", Command::Italic),
            A("Underline", Command::Underline),
        ],
    ),
    (
        "View",
        &[
            M(
                "Theme",
                &[
                    A("Light", Command::LightTheme),
                    A("Dark", Command::DarkTheme),
                ],
            ),
            S,
            A("Agents board", Command::AgentsBoard),
            A("Scheduled tasks", Command::Automation),
            S,
            A("Search Sessions…", Command::Search),
            A("Quick Open File…", Command::QuickOpen),
            A("Session Sidebar", Command::Sidebar),
            A("Session Details", Command::Inspector),
            A("Workspace Files", Command::Files),
            A("Browser", Command::Browser),
            A("Changes", Command::Changes),
            A("Terminal", Command::Terminal),
            M(
                "Transcript view",
                &[
                    A("Normal", Command::TranscriptNormal),
                    A("Verbose", Command::TranscriptVerbose),
                ],
            ),
            S,
            A("Actual Size", Command::ZoomReset),
            A("Zoom In", Command::ZoomIn),
            A("Zoom Out", Command::ZoomOut),
            S,
            A("Full Screen", Command::Fullscreen),
        ],
    ),
    (
        "Window",
        &[
            A("Minimize", Command::Minimize),
            A("Zoom", Command::Maximize),
        ],
    ),
    ("Help", &[A("Keyboard Shortcuts", Command::Shortcuts)]),
];

#[derive(Clone, Copy, PartialEq)]
pub(super) struct MenuState {
    pub theme: theme::Theme,
    pub alerts: notifications::NotificationSettings,
    pub delivery: settings::DeliveryMode,
    pub sidebar: bool,
    pub agents_board: bool,
    pub automation: bool,
    pub inspector: bool,
    pub inspector_available: bool,
    pub files: bool,
    pub browser: bool,
    pub changes: bool,
    pub terminal: bool,
    pub terminal_focused: bool,
    pub text_focused: bool,
    pub composer_focused: bool,
    pub text_selected: bool,
    pub fullscreen: bool,
    pub zoom: f32,
    pub transcript_view: crate::transcript::View,
}
impl MenuState {
    pub fn checked(self, command: Command) -> Option<bool> {
        Some(match command {
            Command::TranscriptNormal => self.transcript_view == crate::transcript::View::Normal,
            Command::TranscriptVerbose => self.transcript_view == crate::transcript::View::Verbose,
            Command::DarkTheme => self.theme == theme::Theme::Dark,
            Command::LightTheme => self.theme == theme::Theme::Light,
            Command::CompletionSound => self.alerts.sound,
            Command::DesktopNotification => self.alerts.desktop,
            Command::Steer => self.delivery == settings::DeliveryMode::Steer,
            Command::Queue => self.delivery == settings::DeliveryMode::Queue,
            Command::Sidebar => self.sidebar,
            Command::AgentsBoard => self.agents_board,
            Command::Automation => self.automation,
            Command::Inspector => self.inspector && self.inspector_available,
            Command::Files => self.files,
            Command::Browser => self.browser,
            Command::Changes => self.changes,
            Command::Terminal => self.terminal,
            Command::Fullscreen => self.fullscreen,
            _ => return None,
        })
    }
    pub fn enabled(self, command: Command) -> bool {
        command.supported()
            && match command {
                Command::Inspector => self.inspector_available,
                Command::Bold | Command::Italic | Command::Underline => self.composer_focused,
                Command::Undo | Command::Redo | Command::SelectAll => {
                    self.text_focused && !self.terminal_focused
                }
                Command::Cut => self.text_focused && self.text_selected && !self.terminal_focused,
                Command::Paste => self.text_focused || self.terminal_focused,
                Command::ZoomIn => self.zoom < 1.4,
                Command::ZoomOut => self.zoom > 0.8,
                _ => true,
            }
    }
}
