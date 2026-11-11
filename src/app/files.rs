//! Docked, read-only file and page viewer. Reads require a click or pui_open.
mod chrome;
use super::*;
use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    io::Read,
    path::{Component, Path},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
};

const MAX_ENTRIES: usize = 2_000;
const MAX_TEXT: usize = 512 * 1024;
const MAX_TABS: usize = 16;
const MAX_DIRECTORIES: usize = 128;
const MAX_SEARCH: usize = 20_000;
const MAX_MATCHES: usize = 200;

#[derive(Clone)]
struct Entry {
    path: PathBuf,
    directory: bool,
    blocked: Option<String>,
}
struct Listing {
    entries: Vec<Entry>,
    truncated: bool,
}
#[derive(Default)]
struct Directory {
    listing: Option<Result<Listing, String>>,
    rx: Option<Receiver<Result<Listing, String>>>,
}
enum Content {
    Text(String),
    Image(egui::ColorImage),
}
struct OpenFile {
    path: PathBuf,
    content: Option<Result<Content, String>>,
    rx: Option<Receiver<Result<Content, String>>>,
    texture: Option<egui::TextureHandle>,
    source: bool,
    external: bool,
}
struct WebPreview {
    rect: egui::Rect,
    key: String,
    url: Option<String>,
    html: Option<String>,
}
struct Search {
    entries: Vec<Entry>,
    limited: bool,
}
enum Action {
    Expand(PathBuf),
    Select(PathBuf),
}

#[derive(Default)]
pub(super) struct Browser {
    open: bool,
    expanded: bool,
    browser_mode: bool,
    focus_address: bool,
    input_focus: bool,
    pane_rect: Option<egui::Rect>,
    root: PathBuf,
    directories: BTreeMap<PathBuf, Directory>,
    expanded_dirs: BTreeSet<PathBuf>,
    tabs: Vec<OpenFile>,
    selected: Option<PathBuf>,
    query: String,
    search: Option<Result<Search, String>>,
    search_rx: Option<Receiver<Result<Search, String>>>,
    search_cancel: Arc<AtomicBool>,
    urls: Vec<String>,
    selected_url: Option<String>,
    address: String,
    address_error: Option<String>,
    web: super::webview::BrowserView,
    web_preview: Option<WebPreview>,
}

impl Drop for Browser {
    fn drop(&mut self) {
        self.search_cancel.store(true, Ordering::Relaxed);
    }
}

impl Browser {
    pub(super) fn is_open(&self) -> bool {
        self.open
    }

    pub(super) fn files_open(&self) -> bool {
        self.open && !self.browser_mode
    }

    pub(super) fn browser_open(&self) -> bool {
        self.open && self.browser_mode
    }

    pub(super) fn navigation_target(&self) -> Option<String> {
        if self.browser_mode {
            self.selected_url.clone()
        } else {
            self.selected
                .as_ref()
                .map(|path| path.display().to_string())
        }
    }

    pub(super) fn focused(&self, ctx: &egui::Context) -> bool {
        let composer_focused = ctx.memory(|m| m.has_focus(egui::Id::new("composer")));
        self.browser_open()
            && (self.web.focused()
                || ctx.input(|i| {
                    if i.pointer.any_pressed() {
                        i.pointer.interact_pos().is_some_and(|pos| {
                            self.pane_rect.is_some_and(|rect| rect.contains(pos))
                        })
                    } else {
                        self.input_focus && !composer_focused
                    }
                }))
    }

    pub(super) fn close_active_browser_tab(&mut self) {
        if let Some(index) = self
            .urls
            .iter()
            .position(|url| Some(url) == self.selected_url.as_ref())
        {
            self.close_url_tab(index);
        } else {
            self.close();
        }
    }

    fn close_url_tab(&mut self, index: usize) {
        let url = self.urls.remove(index);
        self.web.close(&url);
        self.tabs
            .retain(|tab| tab.path.to_string_lossy() != url.as_str());
        if self.selected_url.as_ref() == Some(&url) {
            self.selected_url = self
                .urls
                .get(index.min(self.urls.len().saturating_sub(1)))
                .cloned();
            self.restore_address();
        }
        self.web_preview = None;
        self.address_error = None;
        self.focus_address = self.selected_url.is_none();
    }

    pub(super) fn close(&mut self) {
        self.open = false;
        self.input_focus = false;
        self.pane_rect = None;
        self.web.hide();
    }

    pub(super) fn toggle(&mut self, ctx: &egui::Context, root: &Path) {
        self.toggle_pane(ctx, root, false);
    }

    pub(super) fn toggle_browser(&mut self, ctx: &egui::Context, root: &Path) {
        self.toggle_pane(ctx, root, true);
    }

    fn toggle_pane(&mut self, ctx: &egui::Context, root: &Path, browser: bool) {
        if self.open && self.browser_mode == browser {
            self.close();
            return;
        }
        self.workspace(ctx, root);
        self.web.hide();
        self.browser_mode = browser;
        self.open = true;
        self.address_error = None;
        self.restore_address();
        self.focus_address = browser && self.selected_url.is_none();
        self.input_focus = browser;
        if browser {
            ctx.memory_mut(|m| m.surrender_focus(egui::Id::new("composer")));
        }
    }

    fn restore_address(&mut self) {
        self.address = if self.browser_mode {
            self.selected_url
                .as_ref()
                .map(|key| {
                    if super::webview::web_url(key) {
                        self.web.location(key).unwrap_or_else(|| key.clone())
                    } else {
                        key.clone()
                    }
                })
                .unwrap_or_default()
        } else {
            self.selected
                .as_ref()
                .map(|path| path.display().to_string())
                .unwrap_or_default()
        };
    }

    fn workspace(&mut self, ctx: &egui::Context, root: &Path) {
        if self.root != root {
            self.search_cancel.store(true, Ordering::Relaxed);
            self.root = root.to_path_buf();
            self.directories.clear();
            self.expanded_dirs.clear();
            self.tabs.clear();
            self.selected = None;
            self.urls.clear();
            self.selected_url = None;
            self.web.clear();
            self.address.clear();
            self.query.clear();
            self.search = None;
            self.search_rx = None;
        }
        if !self.directories.contains_key(root) {
            self.load_directory(ctx, root.to_path_buf());
        }
    }

    fn load_directory(&mut self, ctx: &egui::Context, path: PathBuf) {
        // Evict collapsed directory caches, never scan the whole workspace at startup.
        if self.directories.len() >= MAX_DIRECTORIES
            && !self.directories.contains_key(&path)
            && let Some(old) = self
                .directories
                .keys()
                .find(|p| **p != self.root && !self.expanded_dirs.contains(*p))
                .cloned()
        {
            self.directories.remove(&old);
        }
        let root = self.root.clone();
        let ctx = ctx.clone();
        let (tx, rx) = mpsc::channel();
        self.directories.insert(
            path.clone(),
            Directory {
                listing: None,
                rx: Some(rx),
            },
        );
        std::thread::spawn(move || {
            let _ = tx.send(list(&root, &path));
            ctx.request_repaint();
        });
    }

    fn expand(&mut self, ctx: &egui::Context, path: PathBuf) {
        if self.expanded_dirs.remove(&path) {
            return;
        }
        if self.expanded_dirs.len() >= MAX_DIRECTORIES - 1 {
            return;
        }
        self.expanded_dirs.insert(path.clone());
        if !self.directories.contains_key(&path) {
            self.load_directory(ctx, path);
        }
    }

    fn select(&mut self, ctx: &egui::Context, path: PathBuf) {
        self.select_file(ctx, path, false);
    }

    fn select_file(&mut self, ctx: &egui::Context, path: PathBuf, external: bool) {
        self.browser_mode = is_html(&path);
        if !self.browser_mode {
            self.selected = Some(path.clone());
        }
        let key = path.display().to_string();
        self.web.hide();
        if self.browser_mode {
            if !self.urls.contains(&key) {
                if self.urls.len() >= MAX_TABS {
                    self.close_url_tab(0);
                }
                self.urls.push(key.clone());
            }
            self.selected_url = Some(key.clone());
        }
        self.address = key;
        if self.tabs.iter().any(|tab| tab.path == path) {
            return;
        }
        if self.tabs.len() == MAX_TABS {
            let old = self.tabs.remove(0);
            let key = old.path.display().to_string();
            self.web.close(&key);
            self.urls.retain(|url| url != &key);
        }
        let root = self.root.clone();
        let ctx = ctx.clone();
        let (tx, rx) = mpsc::channel();
        self.tabs.push(OpenFile {
            path: path.clone(),
            content: None,
            rx: Some(rx),
            texture: None,
            source: false,
            external,
        });
        std::thread::spawn(move || {
            let _ = tx.send(read_content(&root, &path, external));
            ctx.request_repaint();
        });
    }

    fn close_tab(&mut self, index: usize) {
        let tab = self.tabs.remove(index);
        self.web.close(&tab.path.display().to_string());
        if self.selected.as_ref() == Some(&tab.path) {
            self.selected = self
                .tabs
                .iter()
                .find(|tab| !is_html(&tab.path))
                .map(|tab| tab.path.clone());
            self.restore_address();
        }
    }

    fn refresh(&mut self, ctx: &egui::Context) {
        self.web.reload();
        let paths: Vec<_> = std::iter::once(self.root.clone())
            .chain(self.expanded_dirs.iter().cloned())
            .collect();
        self.directories.clear();
        for path in paths {
            self.load_directory(ctx, path);
        }
        let path = if self.browser_mode {
            self.selected_url
                .as_ref()
                .filter(|key| !super::webview::web_url(key))
                .map(PathBuf::from)
        } else {
            self.selected.clone()
        };
        if let Some(path) = path {
            if let Some(index) = self.tabs.iter().position(|tab| tab.path == path) {
                let external = self.tabs.remove(index).external;
                self.select_file(ctx, path, external);
            }
        }
        self.search_files(ctx);
    }

    fn search_files(&mut self, ctx: &egui::Context) {
        self.search_cancel.store(true, Ordering::Relaxed);
        self.search_cancel = Arc::new(AtomicBool::new(false));
        self.search = None;
        self.search_rx = None;
        let query = self.query.trim().to_lowercase();
        if query.is_empty() {
            return;
        }
        let root = self.root.clone();
        let cancel = self.search_cancel.clone();
        let ctx = ctx.clone();
        let (tx, rx) = mpsc::channel();
        self.search_rx = Some(rx);
        std::thread::spawn(move || {
            let result = search_paths(&root, &query, &cancel);
            if !cancel.load(Ordering::Relaxed) {
                let _ = tx.send(result);
                ctx.request_repaint();
            }
        });
    }

    pub(super) fn open_target(
        &mut self,
        ctx: &egui::Context,
        root: &Path,
        target: &str,
    ) -> Result<(), String> {
        if target.is_empty() || target.len() > 8192 || target.chars().any(char::is_control) {
            return Err("Invalid file path or URL.".into());
        }
        self.workspace(ctx, root);
        if super::webview::web_url(target) {
            if !cfg!(target_os = "macos") {
                return Err("Bundled Chromium is currently packaged for macOS only.".into());
            }
            if !self.urls.iter().any(|url| url == target) {
                if self.urls.len() >= MAX_TABS {
                    let old = self.urls.remove(0);
                    self.web.close(&old);
                }
                self.urls.push(target.into());
            }
            self.browser_mode = true;
            self.selected_url = Some(target.into());
        } else {
            if target.contains("://")
                || target.starts_with("javascript:")
                || target.starts_with("data:")
                || target.starts_with("mailto:")
            {
                return Err("Only HTTP/HTTPS pages and local file paths are supported.".into());
            }
            let path = if let Some(relative) = target.strip_prefix("~/") {
                dirs::home_dir()
                    .ok_or("Cannot resolve home directory.")?
                    .join(relative)
            } else {
                root.join(target.strip_prefix('@').unwrap_or(target))
            };
            if path.components().any(|part| {
                matches!(part, Component::ParentDir)
                    || matches!(part, Component::Normal(name) if sensitive(name))
            }) {
                return Err(
                    "Sensitive files and parent-directory traversal cannot be previewed.".into(),
                );
            }
            self.select_file(ctx, path, true);
        }
        self.open = true;
        self.address = target.into();
        self.address_error = None;
        self.input_focus = self.browser_mode;
        if self.browser_mode {
            ctx.memory_mut(|m| m.surrender_focus(egui::Id::new("composer")));
        }
        Ok(())
    }

    pub(super) fn configure_browser(&mut self, device: &str, console: bool) -> Result<(), String> {
        let key = self
            .selected_url
            .as_deref()
            .ok_or("Open a web page before selecting a device.")?;
        self.web.configure(key, device, console)
    }

    pub(super) fn begin_frame(&mut self) {
        self.web_preview = None;
    }

    pub(super) fn hide_web(&mut self) {
        self.web.hide();
    }

    fn web_obscured(&self, ctx: &egui::Context) -> bool {
        let Some(preview) = &self.web_preview else {
            return false;
        };
        // Native Chromium views sit above egui. Only hide for floating surfaces
        // that actually cover the page, not unrelated popups in the chat pane.
        let overlays = ctx.memory(|memory| {
            memory
                .areas()
                .visible_layer_ids()
                .into_iter()
                .filter(|layer| layer.order >= egui::Order::Middle)
                .filter_map(|layer| memory.area_rect(layer.id).map(|rect| (layer, rect)))
                .collect::<Vec<_>>()
        });
        overlays.into_iter().any(|(layer, rect)| {
            let rect = ctx
                .layer_transform_to_global(layer)
                .map_or(rect, |transform| transform * rect);
            preview.rect.intersects(rect)
        })
    }

    pub(super) fn show_web(&mut self, frame: &eframe::Frame, ctx: &egui::Context) {
        if !self.open || self.web_obscured(ctx) {
            self.web.hide();
            return;
        }
        if let Some(preview) = &self.web_preview {
            let result = self.web.show(
                frame,
                preview.rect,
                &preview.key,
                preview.url.as_deref(),
                preview.html.as_deref(),
                ctx,
            );
            if let Err(error) = result {
                self.address_error = Some(error);
            }
        } else {
            self.web.hide();
        }
    }

    fn poll(&mut self) {
        if self.browser_mode
            && let Some(location) = self.web.take_location()
            && self
                .selected_url
                .as_deref()
                .is_some_and(super::webview::web_url)
        {
            self.address = location;
        }
        for directory in self.directories.values_mut() {
            if let Some(result) = directory.rx.as_ref().and_then(|rx| rx.try_recv().ok()) {
                directory.listing = Some(result);
                directory.rx = None;
            }
        }
        for tab in &mut self.tabs {
            if let Some(result) = tab.rx.as_ref().and_then(|rx| rx.try_recv().ok()) {
                tab.content = Some(result);
                tab.rx = None;
            }
        }
        if let Some(result) = self.search_rx.as_ref().and_then(|rx| rx.try_recv().ok()) {
            self.search = Some(result);
            self.search_rx = None;
        }
    }

    /// Returns true only when this frame used the central workspace (expanded mode).
    pub(super) fn show(&mut self, ctx: &egui::Context, root: &Path) -> bool {
        if !self.open {
            return false;
        }
        self.workspace(ctx, root);
        self.poll();
        let colors = theme::palette(ctx);
        let expanded = self.expanded;
        if expanded {
            egui::CentralPanel::default()
                .frame(egui::Frame::new().fill(colors.bg))
                .show(ctx, |ui| self.contents(ui));
        } else {
            let available = ctx.available_rect().width();
            let max_width = (available - 220.0).max(360.0);
            egui::SidePanel::right("workspace-files-dock")
                .default_width((available * 0.58).clamp(420.0, 760.0))
                .width_range(360.0..=max_width)
                .resizable(true)
                .frame(egui::Frame::new().fill(colors.bg))
                .show(ctx, |ui| self.contents(ui));
        }
        expanded
    }

    fn contents(&mut self, ui: &mut egui::Ui) {
        let rect = ui.max_rect();
        self.pane_rect = Some(rect);
        if let Some(pos) =
            ui.input(|i| i.pointer.interact_pos().filter(|_| i.pointer.any_pressed()))
        {
            self.input_focus = rect.contains(pos);
            if self.browser_mode && self.input_focus {
                ui.ctx()
                    .memory_mut(|m| m.surrender_focus(egui::Id::new("composer")));
            }
        }
        let colors = theme::palette(ui.ctx());
        let ctx = ui.ctx().clone();
        self.chrome(ui);
        let tree_width = (ui.available_width() * 0.38).clamp(170.0, 300.0);
        if !self.browser_mode {
            egui::SidePanel::right("files-tree")
                .default_width(tree_width)
                .width_range(150.0..=(ui.available_width() * 0.65).max(150.0))
                .frame(egui::Frame::new().inner_margin(egui::Margin::symmetric(12, 16)))
                .show_inside(ui, |ui| {
                    let search = egui::Frame::new()
                        .fill(colors.surface)
                        .stroke(Stroke::new(1.0, colors.border))
                        .corner_radius(7)
                        .inner_margin(egui::Margin::symmetric(10, 7))
                        .show(ui, |ui| {
                            ui.add_sized(
                                [ui.available_width(), 18.0],
                                egui::TextEdit::singleline(&mut self.query)
                                    .frame(false)
                                    .hint_text("Search files…")
                                    .id_salt("workspace-file-search"),
                            )
                        })
                        .inner;
                    if search.changed() {
                        self.search_files(&ctx);
                    }
                    ui.add_space(18.0);
                    let mut actions = Vec::new();
                    egui::ScrollArea::both()
                        .id_salt("workspace-file-tree")
                        .auto_shrink([false, false])
                        .show(ui, |ui| {
                            if self.query.trim().is_empty() {
                                self.tree(ui, &self.root, 0, &mut actions);
                            } else {
                                match &self.search {
                                    None => {
                                        ui.label(theme::small(ui, "Searching file names…"));
                                    }
                                    Some(Err(error)) => {
                                        ui.label(theme::small(ui, error));
                                    }
                                    Some(Ok(search)) => {
                                        if search.entries.is_empty() {
                                            ui.label(theme::small(ui, "No matching files."));
                                        }
                                        for entry in &search.entries {
                                            self.row(ui, entry, 0, true, &mut actions);
                                        }
                                        if search.limited {
                                            ui.label(theme::small(
                                                ui,
                                                "Search limited. Browse folders for more files.",
                                            ));
                                        }
                                    }
                                }
                            }
                        });
                    for action in actions {
                        match action {
                            Action::Expand(path) => self.expand(&ctx, path),
                            Action::Select(path) => self.select(&ctx, path),
                        }
                    }
                });
        }
        egui::CentralPanel::default()
            .frame(egui::Frame::new().inner_margin(0))
            .show_inside(ui, |ui| self.preview(ui));
    }

    fn tree(&self, ui: &mut egui::Ui, path: &Path, depth: usize, actions: &mut Vec<Action>) {
        match self.directories.get(path).and_then(|d| d.listing.as_ref()) {
            None => {
                ui.label(theme::small(ui, "Loading…"));
            }
            Some(Err(error)) => {
                ui.label(theme::small(ui, error));
            }
            Some(Ok(listing)) => {
                if listing.entries.is_empty() {
                    ui.label(theme::small(ui, "Empty folder"));
                }
                for entry in &listing.entries {
                    self.row(ui, entry, depth, false, actions);
                    if entry.directory && self.expanded_dirs.contains(&entry.path) {
                        self.tree(ui, &entry.path, depth + 1, actions);
                    }
                }
                if listing.truncated {
                    ui.label(theme::small(ui, "Showing the first 2,000 entries."));
                }
            }
        }
    }

    fn row(
        &self,
        ui: &mut egui::Ui,
        entry: &Entry,
        depth: usize,
        search: bool,
        actions: &mut Vec<Action>,
    ) {
        let colors = theme::palette(ui.ctx());
        let (_, rect) = ui.allocate_space(Vec2::new(ui.available_width().max(120.0), 30.0));
        let response = ui.interact(
            rect,
            ui.make_persistent_id(("workspace-file", &entry.path)),
            Sense::click(),
        );
        if !ui.is_rect_visible(rect) {
            return;
        }
        let selected = self.selected.as_ref() == Some(&entry.path);
        if selected || response.hovered() {
            ui.painter().rect_filled(
                rect,
                4,
                if selected {
                    colors.accent_bg
                } else {
                    colors.hover
                },
            );
        }
        let x = rect.left() + 8.0 + depth as f32 * 16.0;
        if entry.directory {
            icons::chevron(
                ui,
                egui::Rect::from_center_size(egui::pos2(x, rect.center().y), Vec2::splat(12.0)),
                self.expanded_dirs.contains(&entry.path),
                colors.dim,
            );
        }
        icons::draw(
            ui,
            egui::Rect::from_center_size(egui::pos2(x + 20.0, rect.center().y), Vec2::splat(16.0)),
            if entry.directory {
                Icon::Folder
            } else {
                Icon::File
            },
            colors.muted,
        );
        let name = if search {
            entry
                .path
                .strip_prefix(&self.root)
                .unwrap_or(&entry.path)
                .as_os_str()
        } else {
            entry.path.file_name().unwrap_or_default()
        }
        .to_string_lossy();
        let mut job = egui::text::LayoutJob::simple_singleline(
            name.into_owned(),
            FontId::proportional(12.0),
            if entry.blocked.is_some() {
                colors.dim
            } else {
                colors.text
            },
        );
        job.wrap.max_width = (rect.right() - x - 40.0).max(20.0);
        job.wrap.max_rows = 1;
        let galley = ui.painter().layout_job(job);
        ui.painter().galley(
            egui::pos2(x + 36.0, rect.center().y - galley.size().y / 2.0),
            galley,
            colors.text,
        );
        let response = response.on_hover_text(
            entry
                .blocked
                .as_deref()
                .map(str::to_owned)
                .unwrap_or_else(|| entry.path.display().to_string()),
        );
        if entry.blocked.is_none()
            && response
                .on_hover_cursor(egui::CursorIcon::PointingHand)
                .clicked()
        {
            actions.push(if entry.directory {
                Action::Expand(entry.path.clone())
            } else {
                Action::Select(entry.path.clone())
            });
        }
    }

    fn preview(&mut self, ui: &mut egui::Ui) {
        let colors = theme::palette(ui.ctx());
        if self.browser_mode {
            if let Some(url) = self.selected_url.clone() {
                if super::webview::web_url(&url) {
                    self.web_preview = Some(WebPreview {
                        rect: self.web.viewport(ui, &url),
                        key: url.clone(),
                        url: Some(url),
                        html: None,
                    });
                    return;
                }
            } else {
                ui.vertical_centered(|ui| {
                    ui.add_space((ui.available_height() * 0.30).max(24.0));
                    let (rect, _) = ui.allocate_exact_size(Vec2::splat(36.0), Sense::hover());
                    icons::draw(ui, rect, Icon::Globe, colors.dim);
                    ui.add_space(14.0);
                    ui.label(RichText::new("Open a page").size(18.0).color(colors.text));
                    ui.add_space(6.0);
                    ui.label(theme::small(
                        ui,
                        "Enter a URL above, including localhost development servers.",
                    ));
                    ui.add_space(12.0);
                    ui.label(theme::small(
                        ui,
                        "Chromium · Console & DevTools · Desktop & mobile previews",
                    ));
                });
                return;
            }
        }
        let path = if self.browser_mode {
            self.selected_url.as_ref().map(PathBuf::from)
        } else {
            self.selected.clone()
        };
        let Some(tab) = self
            .tabs
            .iter_mut()
            .find(|tab| Some(&tab.path) == path.as_ref())
        else {
            ui.vertical_centered(|ui| {
                ui.add_space(((ui.available_height() - 180.0) / 2.0).max(20.0));
                let (rect, _) = ui.allocate_exact_size(Vec2::splat(42.0), Sense::hover());
                let c = rect.center();
                ui.painter().add(egui::Shape::closed_line(vec![
                    c + egui::vec2(-17.0, -12.0), c + egui::vec2(-5.0, -12.0),
                    c + egui::vec2(0.0, -7.0), c + egui::vec2(17.0, -7.0),
                    c + egui::vec2(17.0, 13.0), c + egui::vec2(-17.0, 13.0),
                ], Stroke::new(2.0, colors.muted)));
                ui.add_space(12.0);
                ui.label(RichText::new("Open files appear here").size(15.0).color(colors.muted));
                ui.add_space(6.0);
                ui.label(RichText::new("Pick a file in the tree to read it.").size(12.0).color(colors.muted));
                ui.add_space(8.0);
                ui.add(egui::Label::new(theme::small(ui,
                    "Open a file or HTTP/HTTPS page above, or select a workspace file. Sensitive and oversized files are blocked."))
                    .wrap().halign(Align::Center));
            });
            return;
        };
        egui::Frame::new().inner_margin(16).show(ui, |ui| {
            ui.add(
                egui::Label::new(theme::small(
                    ui,
                    tab.path
                        .strip_prefix(&self.root)
                        .unwrap_or(&tab.path)
                        .display()
                        .to_string(),
                ))
                .selectable(true),
            );
            let extension = tab
                .path
                .extension()
                .unwrap_or_default()
                .to_string_lossy()
                .to_ascii_lowercase();
            let markdown = matches!(extension.as_str(), "md" | "markdown" | "mdown");
            let html = matches!(extension.as_str(), "html" | "htm");
            if markdown || html {
                ui.horizontal(|ui| {
                    ui.selectable_value(&mut tab.source, false, "Preview");
                    ui.selectable_value(&mut tab.source, true, "Source");
                    if html {
                        ui.label(theme::small(
                            ui,
                            "Isolated preview · scripts enabled · network disabled",
                        ));
                    }
                });
            }
            ui.add_space(12.0);
            match &tab.content {
                None => {
                    ui.label(theme::small(ui, "Loading file…"));
                }
                Some(Err(error)) => {
                    ui.add(egui::Label::new(theme::small(ui, error)).wrap());
                }
                Some(Ok(Content::Text(content))) if html && !tab.source => {
                    self.web_preview = Some(WebPreview {
                        rect: self.web.viewport(ui, &tab.path.display().to_string()),
                        key: tab.path.display().to_string(),
                        url: None,
                        html: Some(content.clone()),
                    });
                }
                Some(Ok(Content::Text(content))) => {
                    egui::ScrollArea::both()
                        .id_salt(("file-content", &tab.path, tab.source))
                        .auto_shrink([false, false])
                        .show(ui, |ui| {
                            if markdown && !tab.source {
                                ui.set_max_width(ui.available_width().min(840.0));
                                markdown::show_file(ui, content, &tab.path);
                            } else {
                                ui.horizontal_top(|ui| {
                                    let numbers = (1..=content.lines().count().max(1))
                                        .map(|n| n.to_string())
                                        .collect::<Vec<_>>()
                                        .join("\n");
                                    ui.add(
                                        egui::Label::new(
                                            RichText::new(numbers)
                                                .monospace()
                                                .size(12.0)
                                                .color(colors.dim),
                                        )
                                        .extend(),
                                    );
                                    ui.add(
                                        egui::Label::new(
                                            RichText::new(content).monospace().size(12.0),
                                        )
                                        .extend()
                                        .selectable(true),
                                    );
                                });
                            }
                        });
                }
                Some(Ok(Content::Image(image))) => {
                    let texture = tab.texture.get_or_insert_with(|| {
                        ui.ctx().load_texture(
                            tab.path.display().to_string(),
                            image.clone(),
                            egui::TextureOptions::LINEAR,
                        )
                    });
                    egui::ScrollArea::both()
                        .id_salt(("file-image", &tab.path))
                        .show(ui, |ui| {
                            ui.add(
                                egui::Image::new(&*texture)
                                    .max_width(ui.available_width())
                                    .shrink_to_fit(),
                            );
                        });
                }
            }
        });
    }
}

impl Pui {
    pub(super) fn open_request(&mut self, record: &Value) -> bool {
        if record["type"] != "extension_ui_request"
            || record["method"] != "input"
            || record["title"] != "Pui open"
        {
            return false;
        }
        let result = if self.agents.background {
            Err("Cannot open a viewer from a background chat. Ask the parent to present the deliverable.".into())
        } else {
            serde_json::from_str::<Value>(protocol::string(record, "placeholder"))
                .map_err(|_| "Invalid open request.".to_owned())
                .and_then(|request| {
                    if request["version"] != 1 {
                        return Err("Unsupported open request version.".into());
                    }
                    let target = request["target"]
                        .as_str()
                        .ok_or("Missing file path or URL.")?;
                    self.files.open_target(&self.ctx, &self.prefs.cwd, target)
                })
        };
        let value = match result {
            Ok(()) => {
                self.settings_open = false;
                self.inspector_panel = false;
                self.terminals.close(&self.ctx);
                json!({"opening":true})
            }
            Err(error) => json!({"error":error}),
        };
        self.send(
            json!({"type":"extension_ui_response","id":record["id"],"value":value.to_string()}),
        );
        true
    }

    pub(super) fn process_open_links(&mut self, ctx: &egui::Context) {
        // Keep remote web links with the OS/default browser. Only local
        // development previews and files are captured by the docked viewer.
        let mut targets = Vec::new();
        ctx.output_mut(|output| {
            output.commands.retain(|command| {
                if let egui::OutputCommand::OpenUrl(url) = command {
                    if cfg!(target_os = "macos") && super::webview::local_preview_url(&url.url) {
                        targets.push(url.url.clone());
                        return false;
                    }
                    if let Ok(url) = url::Url::parse(&url.url)
                        && url.scheme() == "file"
                        && let Ok(path) = url.to_file_path()
                    {
                        targets.push(path.display().to_string());
                        return false;
                    }
                    if !url.url.contains(':')
                        && !url.url.starts_with('#')
                        && std::path::Path::new(&url.url).extension().is_some()
                    {
                        targets.push(url.url.clone());
                        return false;
                    }
                }
                true
            })
        });
        for target in targets {
            if let Err(error) = self.files.open_target(ctx, &self.prefs.cwd, &target) {
                self.state.notice = Some(error);
            } else {
                self.inspector_panel = false;
                self.terminals.close(ctx);
                self.files.hide_web();
            }
            ctx.request_repaint();
        }
    }
}

fn sensitive(name: &std::ffi::OsStr) -> bool {
    let name = name.to_string_lossy().to_lowercase();
    name == ".env"
        || name.starts_with(".env.")
        || name == ".git"
        || matches!(
            name.as_str(),
            ".ssh"
                | ".aws"
                | ".gnupg"
                | ".kube"
                | ".npmrc"
                | ".netrc"
                | ".pypirc"
                | "credentials"
                | "credentials.json"
                | "auth.json"
                | "secrets.json"
                | "secrets.yaml"
                | "secrets.yml"
                | "id_rsa"
                | "id_ed25519"
                | "id_ecdsa"
                | "id_dsa"
        )
        || [".pem", ".key", ".p12", ".pfx", ".p8", ".keystore", ".jks"]
            .iter()
            .any(|ext| name.ends_with(ext))
}

fn within_workspace(root: &Path, path: &Path) -> Result<PathBuf, String> {
    let relative = path
        .strip_prefix(root)
        .map_err(|_| "Path is outside the workspace.")?;
    let mut resolved = root
        .canonicalize()
        .map_err(|e| format!("Cannot open workspace: {e}"))?;
    for component in relative.components() {
        let Component::Normal(name) = component else {
            return Err("Path is outside the workspace.".into());
        };
        if sensitive(name) {
            return Err("Sensitive files and folders cannot be previewed.".into());
        }
        resolved.push(name);
        let metadata =
            fs::symlink_metadata(&resolved).map_err(|e| format!("Cannot open path: {e}"))?;
        if metadata.file_type().is_symlink() {
            return Err("Symlinked files and folders cannot be previewed.".into());
        }
    }
    Ok(resolved)
}

fn list(root: &Path, directory: &Path) -> Result<Listing, String> {
    let resolved = within_workspace(root, directory)?;
    let mut entries = Vec::new();
    let mut truncated = false;
    for entry in fs::read_dir(resolved).map_err(|e| format!("Cannot list folder: {e}"))? {
        let entry = entry.map_err(|e| format!("Cannot read folder entry: {e}"))?;
        if entry.file_name() == ".git" {
            continue;
        }
        if entries.len() == MAX_ENTRIES {
            truncated = true;
            break;
        }
        let kind = entry
            .file_type()
            .map_err(|e| format!("Cannot read file type: {e}"))?;
        let blocked = if sensitive(&entry.file_name()) {
            Some("Sensitive files and folders cannot be previewed.".into())
        } else if kind.is_symlink() {
            Some("Symlinked files and folders cannot be previewed.".into())
        } else if !kind.is_dir() && !kind.is_file() {
            Some("Only regular files can be previewed.".into())
        } else {
            None
        };
        entries.push(Entry {
            path: directory.join(entry.file_name()),
            directory: kind.is_dir(),
            blocked,
        });
    }
    entries.sort_by_cached_key(|e| {
        (
            !e.directory,
            e.path.file_name().unwrap_or_default().to_ascii_lowercase(),
        )
    });
    Ok(Listing { entries, truncated })
}

fn read_content(root: &Path, path: &Path, external: bool) -> Result<Content, String> {
    // Explicit opens may name reports outside the tree (e.g. /tmp). Resolve OS
    // aliases such as macOS /tmp, then recheck the canonical sensitive path.
    let path = if external {
        if fs::symlink_metadata(path)
            .map_err(|e| format!("Cannot open file: {e}"))?
            .file_type()
            .is_symlink()
        {
            return Err("Symlinked files cannot be previewed.".into());
        }
        let resolved = path
            .canonicalize()
            .map_err(|e| format!("Cannot open file: {e}"))?;
        if path
            .components()
            .chain(resolved.components())
            .any(|part| matches!(part, Component::Normal(name) if sensitive(name)))
        {
            return Err("Sensitive files and folders cannot be previewed.".into());
        }
        resolved
    } else {
        within_workspace(root, path)?
    };
    let parent = path.parent().ok_or("File has no parent folder.")?;
    let extension = path
        .extension()
        .unwrap_or_default()
        .to_string_lossy()
        .to_ascii_lowercase();
    if matches!(extension.as_str(), "png" | "jpg" | "jpeg" | "gif" | "webp") {
        let file = fs::File::open(&path).map_err(|e| format!("Cannot open image: {e}"))?;
        let metadata = file.metadata().map_err(|e| e.to_string())?;
        if !metadata.is_file() || metadata.len() > 16 * 1024 * 1024 {
            return Err("Image must be a regular file under 16 MiB.".into());
        }
        let mut bytes = Vec::new();
        file.take(16 * 1024 * 1024 + 1)
            .read_to_end(&mut bytes)
            .map_err(|e| e.to_string())?;
        if bytes.len() > 16 * 1024 * 1024 {
            return Err("Image exceeds 16 MiB.".into());
        }
        let mut reader = image::ImageReader::new(std::io::Cursor::new(bytes))
            .with_guessed_format()
            .map_err(|e| e.to_string())?;
        let mut limits = image::Limits::default();
        limits.max_image_width = Some(8192);
        limits.max_image_height = Some(8192);
        limits.max_alloc = Some(64 * 1024 * 1024);
        reader.limits(limits);
        let image = reader
            .decode()
            .map_err(|e| format!("Cannot decode image: {e}"))?
            .into_rgba8();
        return Ok(Content::Image(egui::ColorImage::from_rgba_unmultiplied(
            [image.width() as usize, image.height() as usize],
            image.as_raw(),
        )));
    }
    read_preview(parent, &path).map(Content::Text)
}

fn is_html(path: &Path) -> bool {
    path.extension().is_some_and(|extension| {
        extension.eq_ignore_ascii_case("html") || extension.eq_ignore_ascii_case("htm")
    })
}

fn read_preview(root: &Path, path: &Path) -> Result<String, String> {
    let path = within_workspace(root, path)?;
    let metadata = fs::symlink_metadata(&path).map_err(|e| format!("Cannot read file: {e}"))?;
    if !metadata.is_file() {
        return Err("Only regular files can be previewed.".into());
    }
    let file = fs::File::open(path).map_err(|e| format!("Cannot open file: {e}"))?;
    let metadata = file
        .metadata()
        .map_err(|e| format!("Cannot read file: {e}"))?;
    if !metadata.is_file() {
        return Err("Only regular files can be previewed.".into());
    }
    if metadata.len() > MAX_TEXT as u64 {
        return Err("File exceeds the 512 KiB preview limit.".into());
    }
    let mut bytes = Vec::new();
    file.take((MAX_TEXT + 1) as u64)
        .read_to_end(&mut bytes)
        .map_err(|e| format!("Cannot read file: {e}"))?;
    if bytes.len() > MAX_TEXT {
        return Err("File exceeds the 512 KiB preview limit.".into());
    }
    if bytes
        .iter()
        .any(|b| *b < 0x20 && !matches!(b, b'\n' | b'\r' | b'\t'))
    {
        return Err("Binary file — no text preview available.".into());
    }
    String::from_utf8(bytes)
        .map_err(|_| "Binary or non-UTF-8 file — no text preview available.".into())
}

/// Filename-only discovery for composer mentions; never opens file contents.
pub(super) fn mention_paths(
    root: &Path,
    query: &str,
    cancel: &AtomicBool,
) -> Result<(Vec<PathBuf>, bool), String> {
    search_paths(root, query, cancel).map(|search| {
        (
            search.entries.into_iter().map(|e| e.path).collect(),
            search.limited,
        )
    })
}

fn search_paths(root: &Path, query: &str, cancel: &AtomicBool) -> Result<Search, String> {
    let mut directories = vec![(root.to_path_buf(), 0)];
    let mut search = Search {
        entries: Vec::new(),
        limited: false,
    };
    let mut visited = 0;
    while let Some((directory, depth)) = directories.pop() {
        if cancel.load(Ordering::Relaxed) {
            break;
        }
        let listing = match list(root, &directory) {
            Ok(listing) => listing,
            Err(error) if directory == root => return Err(error),
            Err(_) => continue,
        };
        search.limited |= listing.truncated;
        for entry in listing.entries {
            if cancel.load(Ordering::Relaxed) {
                return Ok(search);
            }
            visited += 1;
            if visited > MAX_SEARCH || search.entries.len() == MAX_MATCHES {
                search.limited = true;
                return Ok(search);
            }
            if entry.blocked.is_some() {
                continue;
            }
            if entry.directory {
                let name = entry.path.file_name().unwrap_or_default().to_string_lossy();
                if depth < 32
                    && !matches!(
                        name.as_ref(),
                        "target" | "node_modules" | ".venv" | ".cache"
                    )
                {
                    directories.push((entry.path, depth + 1));
                } else {
                    search.limited = true;
                }
            } else if entry
                .path
                .strip_prefix(root)
                .unwrap_or(&entry.path)
                .to_string_lossy()
                .to_lowercase()
                .contains(query)
            {
                search.entries.push(entry);
            }
        }
    }
    search.entries.sort_by(|a, b| a.path.cmp(&b.path));
    Ok(search)
}

#[cfg(test)]
pub(crate) mod tests;
