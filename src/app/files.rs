//! Docked, read-only file and page viewer. Retained visible tabs restore automatically.
pub(crate) mod budget;
mod chrome;
mod delimited;
pub(crate) mod image_preview;
mod line_target;
mod loader;
#[cfg(test)]
mod open_tests;
mod quick_paths;
mod svg;
mod syntax;
pub(super) use syntax::file_icon;
mod table_preferences;
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
pub(super) use table_preferences::TableHeaders;

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
    Table(delimited::Preview),
    Svg(Box<svg::Preview>),
    Image(egui::ColorImage),
    UploadedImage,
}
struct OpenFile {
    path: PathBuf,
    content: Option<Result<Content, String>>,
    rx: Option<Receiver<Result<Content, String>>>,
    texture: Option<egui::TextureHandle>,
    source: bool,
    external: bool,
    line_target: Option<usize>,
    line_notice: Option<String>,
    _budget: Arc<budget::Lease>,
    _load: Option<loader::Ticket>,
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
    Load(PathBuf),
}

#[derive(Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub(super) struct SavedLayout {
    open: bool,
    expanded: bool,
    browser_mode: bool,
    selected: Option<PathBuf>,
    selected_url: Option<String>,
    paths: Vec<PathBuf>,
    external_paths: BTreeSet<PathBuf>,
    source_paths: BTreeSet<PathBuf>,
    urls: Vec<String>,
    directories: Vec<PathBuf>,
}

impl SavedLayout {
    fn bounded(mut self) -> Self {
        let valid_path = |path: &PathBuf| {
            let value = path.to_string_lossy();
            value.len() <= 8192 && !value.chars().any(char::is_control)
        };
        self.paths.retain(valid_path);
        self.paths.truncate(MAX_TABS);
        self.external_paths.retain(|path| self.paths.contains(path));
        self.source_paths.retain(|path| self.paths.contains(path));
        self.urls.retain(|url| {
            url.len() <= 8192
                && !url.chars().any(char::is_control)
                && (super::webview::web_url(url)
                    || self
                        .paths
                        .iter()
                        .any(|path| is_html(path) && path.to_string_lossy() == url.as_str()))
        });
        self.urls.truncate(MAX_TABS);
        self.directories.retain(valid_path);
        self.directories.truncate(MAX_DIRECTORIES - 1);
        self.selected = self.selected.filter(|path| self.paths.contains(path));
        self.selected_url = self.selected_url.filter(|url| self.urls.contains(url));
        self.expanded &= self.open;
        self
    }
}

#[derive(Default)]
pub(super) struct Browser {
    open: bool,
    expanded: bool,
    browser_mode: bool,
    tree_open: bool,
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
    restored: Option<SavedLayout>,
}

impl Drop for Browser {
    fn drop(&mut self) {
        self.search_cancel.store(true, Ordering::Relaxed);
    }
}

impl Browser {
    pub(super) fn demo_table_grid(&mut self, ctx: &egui::Context) {
        let path = self.root.join("table-keyboard-fixture.csv");
        self.selected = Some(path.clone());
        self.browser_mode = false;
        let preview = delimited::Preview::demo_grid(ctx, &path);
        let budget = budget::Lease::new(ctx);
        let content = Content::Table(preview);
        let content = budget.admit(&content).map(|()| content);
        self.tabs.push(OpenFile {
            path,
            content: Some(content),
            rx: None,
            texture: None,
            source: false,
            external: false,
            line_target: None,
            line_notice: None,
            _budget: budget,
            _load: None,
        });
    }

    pub(super) fn demo_table_projection(&mut self) {
        if let Some(tab) = self.tabs.last_mut()
            && let Some(Ok(Content::Table(preview))) = &mut tab.content
        {
            preview.demo_projection();
        }
    }
    pub(super) fn demo_source_context(&mut self, ctx: &egui::Context) {
        let path = self.root.join("source-context-fixture.rs");
        self.selected = Some(path.clone());
        self.browser_mode = false;
        let budget = budget::Lease::new(ctx);
        let content = Content::Text(context_capture::demo_source(ctx).into());
        let content = budget.admit(&content).map(|()| content);
        self.tabs.push(OpenFile {
            path,
            content: Some(content),
            rx: None,
            texture: None,
            source: true,
            external: false,
            line_target: None,
            line_notice: None,
            _budget: budget,
            _load: None,
        });
    }

    pub(super) fn saved_layout(&self) -> SavedLayout {
        if let Some(saved) = &self.restored {
            return SavedLayout {
                open: self.open,
                expanded: self.expanded,
                browser_mode: self.browser_mode,
                ..saved.clone()
            };
        }
        SavedLayout {
            open: self.open,
            expanded: self.expanded,
            browser_mode: self.browser_mode,
            selected: self.selected.clone(),
            selected_url: self.selected_url.clone(),
            paths: self
                .tabs
                .iter()
                .take(MAX_TABS)
                .map(|tab| tab.path.clone())
                .collect(),
            external_paths: self
                .tabs
                .iter()
                .filter(|tab| tab.external)
                .map(|tab| tab.path.clone())
                .collect(),
            source_paths: self
                .tabs
                .iter()
                .filter(|tab| tab.source)
                .map(|tab| tab.path.clone())
                .collect(),
            urls: self
                .urls
                .iter()
                .take(MAX_TABS)
                .map(|url| url.chars().take(8192).collect())
                .collect(),
            directories: self
                .expanded_dirs
                .iter()
                .take(MAX_DIRECTORIES)
                .cloned()
                .collect(),
        }
    }
    pub(super) fn restore_layout(&mut self, root: &Path, saved: &SavedLayout) {
        let saved = saved.clone().bounded();
        let pending = saved.open || !saved.paths.is_empty() || !saved.urls.is_empty();
        *self = Self::default();
        self.root = root.into();
        self.open = saved.open;
        self.expanded = saved.expanded;
        self.browser_mode = saved.browser_mode;
        self.restored = pending.then_some(saved);
    }

    fn activate_restored_tabs(&mut self, ctx: &egui::Context) {
        let Some(saved) = self.restored.take() else {
            return;
        };
        let root = self.root.clone();
        let browser_mode = self.browser_mode;
        self.workspace(ctx, &root);
        // Queue the visible tab first, while retaining the user's original tab order.
        let visible = if browser_mode {
            saved.selected_url.as_deref().map(Path::new)
        } else {
            saved.selected.as_deref()
        };
        let paths = saved
            .paths
            .iter()
            .filter(|path| Some(path.as_path()) == visible)
            .chain(
                saved
                    .paths
                    .iter()
                    .filter(|path| Some(path.as_path()) != visible),
            );
        for path in paths {
            self.select_file_mode(
                ctx,
                path.clone(),
                saved.external_paths.contains(path),
                saved.source_paths.contains(path),
            );
            if let Some(tab) = self.tabs.iter_mut().find(|tab| tab.path == *path) {
                tab.source = saved.source_paths.contains(path);
            }
        }
        self.tabs
            .sort_by_key(|tab| saved.paths.iter().position(|path| path == &tab.path));
        self.urls = saved.urls;
        self.selected = saved.selected;
        self.selected_url = saved.selected_url;
        self.expanded_dirs = saved.directories.into_iter().collect();
        self.browser_mode = browser_mode;
        self.restore_address();
    }

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
        if self.root == root {
            self.activate_restored_tabs(ctx);
        }
        self.workspace(ctx, root);
        self.web.hide();
        self.browser_mode = browser;
        self.open = true;
        self.expanded = false;
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
            self.restored = None;
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
        self.select_file_mode(ctx, path, external, false);
    }

    fn select_file_mode(
        &mut self,
        ctx: &egui::Context,
        path: PathBuf,
        external: bool,
        source: bool,
    ) {
        self.activate_restored_tabs(ctx);
        // Quick Open may have reached this call before hidden tabs existed.
        // Never reuse a restored external read for a workspace-only request.
        if !external
            && let Some(index) = self
                .tabs
                .iter()
                .position(|tab| tab.path == path && tab.external)
        {
            self.close_tab(index);
        }
        context_capture::clear_source(ctx);
        let mut selection = egui::text_selection::LabelSelectionState::load(ctx);
        selection.clear_selection();
        selection.store(ctx);
        self.browser_mode = is_html(&path) && !source;
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
        let budget = budget::Lease::new(ctx);
        let (rx, load) = loader::start(
            ctx,
            self.root.clone(),
            path.clone(),
            external,
            budget.clone(),
        );
        self.tabs.push(OpenFile {
            path,
            content: None,
            rx: Some(rx),
            texture: None,
            source: false,
            external,
            line_target: None,
            line_notice: None,
            _budget: budget,
            _load: load,
        });
    }

    fn file_tab(&self, tab: &OpenFile) -> bool {
        !is_html(&tab.path) || tab.source || !self.urls.iter().any(|url| Path::new(url) == tab.path)
    }

    fn close_tab(&mut self, index: usize) {
        let tab = self.tabs.remove(index);
        self.web.close(&tab.path.display().to_string());
        if self.selected.as_ref() == Some(&tab.path) {
            self.selected = self
                .tabs
                .iter()
                .find(|tab| self.file_tab(tab))
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
                let tab = self.tabs.remove(index);
                let (external, source) = (tab.external, tab.source);
                drop(tab); // cancel/release old request before queueing its replacement
                self.select_file_mode(ctx, path.clone(), external, !self.browser_mode && source);
                if let Some(fresh) = self.tabs.iter_mut().find(|tab| tab.path == path) {
                    fresh.source = source;
                }
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
        let resolved = preview_target(root, target)?;
        self.workspace(ctx, root);
        if super::webview::web_url(target) {
            self.activate_restored_tabs(ctx);
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
            self.select_file(ctx, PathBuf::from(resolved), true);
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

    pub(super) fn clear_path_menu(&self, ctx: &egui::Context) {
        path_menu::clear(ctx);
    }
    pub(super) fn context_inputs(&mut self, ctx: &egui::Context, visible: bool, enabled: bool) {
        let showing = visible && self.open && !self.browser_mode && self.restored.is_none();
        path_menu::inputs(ctx, &self.root, showing);
        for tab in &mut self.tabs {
            if (!showing || Some(&tab.path) != self.selected.as_ref())
                && let Some(Ok(Content::Table(preview))) = &mut tab.content
            {
                preview.clear_grid_menu(ctx);
            }
        }
        if !showing {
            return;
        }
        if let Some(tab) = self
            .tabs
            .iter_mut()
            .find(|tab| Some(&tab.path) == self.selected.as_ref())
            && let Some(Ok(Content::Svg(preview))) = &mut tab.content
        {
            preview.inputs(ctx, enabled, tab.source);
        }
        if let Some(tab) = self
            .tabs
            .iter_mut()
            .find(|tab| Some(&tab.path) == self.selected.as_ref())
            && let Some(Ok(Content::Table(preview))) = &mut tab.content
        {
            if tab.source {
                preview.clear_source_grid_menu(ctx);
                preview.toolbar_inputs(ctx, enabled);
            } else {
                preview.grid_inputs(ctx, &tab.path, enabled);
            }
        }
    }

    pub(super) fn table_export_owns_keyboard(&self, ctx: &egui::Context) -> bool {
        if path_menu::owns(ctx) {
            return true;
        }
        self.open
            && !self.browser_mode
            && self.tabs.iter().any(|tab| {
                Some(&tab.path) == self.selected.as_ref()
                    && matches!(&tab.content, Some(Ok(Content::Table(preview))) if preview.export_owns_keyboard(ctx))
            })
    }
    pub(super) fn svg_owns_keyboard(&self, ctx: &egui::Context) -> bool {
        self.open && !self.browser_mode && self.tabs.iter().any(|tab| Some(&tab.path)==self.selected.as_ref() && matches!(&tab.content,Some(Ok(Content::Svg(preview))) if preview.owns_keyboard(ctx)))
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
    pub(super) fn show(
        &mut self,
        ctx: &egui::Context,
        root: &Path,
        tables: &mut TableHeaders,
    ) -> bool {
        if !self.open {
            return false;
        }
        if self.root == root {
            self.activate_restored_tabs(ctx);
        }
        self.workspace(ctx, root);
        self.poll();
        let colors = theme::palette(ctx);
        let expanded = self.expanded;
        if expanded {
            egui::CentralPanel::default()
                .frame(egui::Frame::new().fill(colors.bg))
                .show(ctx, |ui| self.contents(ui, tables));
        } else {
            let available = ctx.available_rect();
            if available.width() < 700.0 {
                egui::TopBottomPanel::bottom("workspace-files-stack")
                    .default_height((available.height() * 0.45).clamp(160.0, 340.0))
                    .height_range(140.0..=(available.height() - 240.0).max(140.0))
                    .resizable(true)
                    .frame(egui::Frame::new().fill(colors.bg))
                    .show(ctx, |ui| self.contents(ui, tables));
            } else {
                egui::SidePanel::right("workspace-files-dock")
                    .default_width((available.width() * 0.52).clamp(320.0, 760.0))
                    .width_range(280.0..=(available.width() - 320.0).max(280.0))
                    .resizable(true)
                    .frame(egui::Frame::new().fill(colors.bg))
                    .show(ctx, |ui| self.contents(ui, tables));
            }
        }
        expanded
    }

    fn contents(&mut self, ui: &mut egui::Ui, tables: &mut TableHeaders) {
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
        if !self.browser_mode && (self.selected.is_none() || self.tree_open) {
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
                            Action::Load(path) => self.load_directory(&ctx, path),
                        }
                    }
                });
        }
        egui::CentralPanel::default()
            .frame(egui::Frame::new().inner_margin(0))
            .show_inside(ui, |ui| {
                self.preview_toolbar(ui);
                self.preview(ui, tables);
            });
    }

    fn tree(&self, ui: &mut egui::Ui, path: &Path, depth: usize, actions: &mut Vec<Action>) {
        match self.directories.get(path).and_then(|d| d.listing.as_ref()) {
            None => {
                ui.label(theme::small(ui, "Loading…"));
                // Restored folders are loaded lazily, in small visible batches.
                if !self.directories.contains_key(path) && actions.len() < 4 {
                    actions.push(Action::Load(path.into()));
                }
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
        let (_, rect) = ui.allocate_space(Vec2::new(ui.available_width().max(120.0), 22.0));
        let response = ui.interact(
            rect,
            ui.make_persistent_id(("workspace-file", &self.root, &entry.path)),
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
        let (icon, icon_color) = if entry.directory {
            (Icon::Folder, colors.muted)
        } else {
            syntax::file_icon(&entry.path, colors)
        };
        icons::draw(
            ui,
            egui::Rect::from_center_size(egui::pos2(x + 20.0, rect.center().y), Vec2::splat(15.0)),
            icon,
            icon_color,
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
        #[cfg(test)]
        ui.ctx().data_mut(|data| {
            data.insert_temp(
                egui::Id::new(("path-menu-test-row", &entry.path)),
                response.interact_rect,
            )
        });
        self.path_context_menu(&response, &entry.path);
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

    fn preview(&mut self, ui: &mut egui::Ui, tables: &mut TableHeaders) {
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
        let table = tab
            .path
            .extension()
            .is_some_and(|ext| ext.eq_ignore_ascii_case("csv") || ext.eq_ignore_ascii_case("tsv"));
        let html_preview = is_html(&tab.path) && !tab.source;
        egui::Frame::new()
            .inner_margin(if html_preview {
                0
            } else if table {
                10
            } else {
                12
            })
            .show(ui, |ui| {
                let extension = tab
                    .path
                    .extension()
                    .unwrap_or_default()
                    .to_string_lossy()
                    .to_ascii_lowercase();
                let markdown = matches!(extension.as_str(), "md" | "markdown" | "mdown");
                let html = matches!(extension.as_str(), "html" | "htm");
                let svg = extension == "svg";
                let pane_rect = ui.max_rect();
                if svg {
                    ui.horizontal(|ui| {
                        let preview_response =
                            ui.selectable_value(&mut tab.source, false, "Preview");
                        let source_response = ui.selectable_value(&mut tab.source, true, "Source");
                        if let Some(Ok(Content::Svg(preview))) = &mut tab.content {
                            preview.remember(0, &preview_response);
                            preview.remember(1, &source_response);
                        }
                    });
                }
                if let Some(notice) = &tab.line_notice {
                    ui.label(theme::small(ui, notice));
                }
                // Transfer decoded pixels once; don't retain a second CPU copy for
                // every open image after egui has accepted the texture upload.
                if matches!(tab.content, Some(Ok(Content::Image(_)))) {
                    if let Some(Ok(Content::Image(image))) = tab.content.take() {
                        tab.texture = Some(ui.ctx().load_texture(
                            tab.path.display().to_string(),
                            image,
                            egui::TextureOptions::LINEAR,
                        ));
                        tab.content = Some(Ok(Content::UploadedImage));
                    }
                }
                match &mut tab.content {
                    Some(Ok(Content::Svg(preview))) => {
                        preview.show(
                            ui,
                            &tab.path,
                            tab.source,
                            &mut tab.texture,
                            &mut tab.line_target,
                            &mut tab.line_notice,
                        );
                        preview.finish_inputs(ui.ctx());
                    }
                    Some(Ok(Content::Table(preview))) => {
                        preview.attach_projection_budget(&tab._budget);
                        let remember = tables.supports(&self.root, &tab.path);
                        if remember {
                            preview.set_header(tables.header(&self.root, &tab.path));
                        }
                        let header = preview.header();
                        if tab.source {
                            if let Some(line) = tab.line_target.take() {
                                let shown = preview.target_line(line);
                                tab.line_notice = Some(if shown == line {
                                    format!("Line {line}")
                                } else {
                                    format!("Requested line {line}; showing last line {shown}.")
                                });
                            }
                        }
                        preview.show_view(ui, &tab.path, &mut tab.source);
                        if remember && preview.header() != header {
                            tables.set(&self.root, &tab.path, preview.header());
                        }
                    }
                    None => {
                        ui.label(theme::small(ui, "Loading file…"));
                    }
                    Some(Err(error)) => {
                        ui.add(egui::Label::new(theme::small(ui, error.as_str())).wrap());
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
                                    context_capture::show_file_blocks(
                                        ui,
                                        &tab.path,
                                        content,
                                        tab._budget.document(),
                                        |ui| markdown::show_file(ui, content, &tab.path),
                                    );
                                } else {
                                    ui.push_id(("source-text", &tab.path), |ui| {
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
                                            let galley = syntax::galley(ui, &tab.path, content);
                                            let response = ui.add(
                                                egui::Label::new(galley.clone())
                                                    .extend()
                                                    .selectable(true),
                                            );
                                            line_target::reveal(
                                                ui,
                                                &response,
                                                &galley,
                                                &mut tab.line_target,
                                                &mut tab.line_notice,
                                            );
                                            context_capture::offer_source_selection(
                                                &response, &tab.path, content, 1,
                                            );
                                        });
                                    });
                                }
                            });
                    }
                    Some(Ok(Content::Image(_))) => unreachable!("image pixels transferred above"),
                    Some(Ok(Content::UploadedImage)) => {
                        if let Some(line) = tab.line_target.take() {
                            tab.line_notice =
                                Some(format!("Line {line} is unavailable for an image preview."));
                        }
                        let texture = tab.texture.as_ref().expect("uploaded image texture");
                        egui::ScrollArea::both()
                            .id_salt(("file-image", &tab.path))
                            .show(ui, |ui| {
                                ui.add(
                                    egui::Image::new(texture)
                                        .max_width(ui.available_width())
                                        .shrink_to_fit(),
                                );
                            });
                    }
                }
                if markdown {
                    let md_headings = if markdown && !tab.source {
                        if let Some(Ok(Content::Text(content))) = &tab.content {
                            markdown::headings(content)
                        } else {
                            Vec::new()
                        }
                    } else {
                        Vec::new()
                    };
                    let has_outline = !md_headings.is_empty();
                    let outline_open_id = ui.id().with(("md-outline-open", &tab.path));
                    let mut outline_open = has_outline
                        && ui
                            .ctx()
                            .data(|d| d.get_temp::<bool>(outline_open_id))
                            .unwrap_or(false);

                    let toggle_id = ui.id().with("preview-source-toggle");
                    let width_offset = if has_outline { 60.0 } else { 32.0 };
                    let pos = egui::pos2(pane_rect.right() - width_offset, pane_rect.top());
                    egui::Area::new(toggle_id)
                        .order(egui::Order::Foreground)
                        .fixed_pos(pos)
                        .show(ui.ctx(), |ui| {
                            egui::Frame::new()
                                .fill(colors.surface)
                                .stroke(egui::Stroke::new(1.0_f32, colors.border))
                                .corner_radius(6)
                                .inner_margin(2)
                                .show(ui, |ui| {
                                    ui.horizontal(|ui| {
                                        ui.spacing_mut().item_spacing.x = 2.0;
                                        if has_outline {
                                            let outline_btn = crate::icons::button(
                                                ui,
                                                crate::icons::Icon::Details,
                                                "Toggle document outline",
                                            );
                                            if outline_btn.clicked() {
                                                outline_open = !outline_open;
                                                ui.ctx().data_mut(|d| {
                                                    d.insert_temp(outline_open_id, outline_open);
                                                });
                                            }
                                        }
                                        let (icon, tip) = if tab.source {
                                            (crate::icons::Icon::File, "Show preview")
                                        } else {
                                            (crate::icons::Icon::Code, "Show source")
                                        };
                                        let response = crate::icons::button(ui, icon, tip);
                                        ui.ctx().data_mut(|data| {
                                            data.insert_temp(
                                                egui::Id::new("file-preview-source-toggle-rect"),
                                                response.rect,
                                            );
                                        });

                                        if response.clicked() {
                                            tab.source = !tab.source;
                                        }
                                    });
                                });
                        });

                    if outline_open {
                        let outline_pos = egui::pos2(
                            (pane_rect.right() - 248.0).max(pane_rect.left() + 8.0),
                            pane_rect.top() + 34.0,
                        );
                        egui::Area::new(ui.id().with("md-outline-popover"))
                            .order(egui::Order::Foreground)
                            .fixed_pos(outline_pos)
                            .show(ui.ctx(), |ui| {
                                egui::Frame::new()
                                    .fill(colors.sidebar)
                                    .stroke(egui::Stroke::new(1.0_f32, colors.border))
                                    .corner_radius(8)
                                    .inner_margin(8)
                                    .show(ui, |ui| {
                                        ui.set_width(232.0);
                                        ui.label(
                                            RichText::new("Outline")
                                                .size(11.0)
                                                .strong()
                                                .color(colors.muted),
                                        );
                                        ui.add_space(4.0);
                                        egui::ScrollArea::vertical()
                                            .max_height(
                                                (pane_rect.height() - 64.0).clamp(120.0, 360.0),
                                            )
                                            .show(ui, |ui| {
                                                for (idx, (level, title)) in
                                                    md_headings.iter().enumerate()
                                                {
                                                    let indent = (*level as f32 - 1.0) * 10.0;
                                                    ui.horizontal(|ui| {
                                                        ui.add_space(indent);
                                                        if ui
                                                            .add(
                                                                egui::Button::new(
                                                                    RichText::new(title)
                                                                        .size(11.5)
                                                                        .color(colors.text),
                                                                )
                                                                .frame(false),
                                                            )
                                                            .clicked()
                                                        {
                                                            markdown::request_scroll_to_heading(
                                                                ui.ctx(),
                                                                idx,
                                                            );
                                                        }
                                                    });
                                                }
                                            });
                                    });
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
        let result = serde_json::from_str::<Value>(protocol::string(record, "placeholder"))
            .map_err(|_| "Invalid open request.".to_owned())
            .and_then(|request| {
                if request["version"] != 1 {
                    return Err("Unsupported open request version.".into());
                }
                let target = request["target"]
                    .as_str()
                    .ok_or("Missing file path or URL.")?;
                let target = preview_target(&self.prefs.cwd, target)?;
                if self.agents.background {
                    // Background means non-selected, not necessarily a child agent.
                    // Keep intent with the owning Chat, never the visible Browser.
                    if !self.preview_opens.contains(&target) {
                        if self.preview_opens.len() >= MAX_TABS {
                            return Err(
                                "This chat has 16 queued previews. Select it before opening more."
                                    .into(),
                            );
                        }
                        self.preview_opens.push(target);
                    }
                    Ok(json!({"queued":true}))
                } else {
                    self.present_preview(&target)?;
                    Ok(json!({"opening":true}))
                }
            });
        let value = result.unwrap_or_else(|error| json!({"error":error}));
        self.send(
            json!({"type":"extension_ui_response","id":record["id"],"value":value.to_string()}),
        );
        true
    }

    fn present_preview(&mut self, target: &str) -> Result<(), String> {
        self.files.open_target(&self.ctx, &self.prefs.cwd, target)?;
        self.settings_open = false;
        self.inspector_panel = false;
        Ok(())
    }

    pub(super) fn open_queued_previews(&mut self) {
        if self.agents.background {
            return;
        }
        for target in std::mem::take(&mut self.preview_opens) {
            if let Err(error) = self.present_preview(&target) {
                self.state.notice = Some(error);
            }
        }
    }

    pub(super) fn web_preview_allowed(&self, theater: bool, confirming: bool) -> bool {
        // Agent questions and permissions are inline, not workspace overlays.
        // Actual floating surfaces are still checked by Browser::web_obscured.
        !self.settings_open
            && !self.kanban.open
            && !self.automation.open
            && !theater
            && !confirming
            && !self.section_view.confirming()
            && !self.search_open
    }

    pub(super) fn process_open_links(&mut self, ctx: &egui::Context) {
        // Keep remote web links with the OS/default browser. Only local
        // development previews and files are captured by the docked viewer.
        let mut targets: Vec<(String, Option<usize>)> = Vec::new();
        ctx.output_mut(|output| {
            output.commands.retain(|command| {
                if let egui::OutputCommand::OpenUrl(url) = command {
                    if cfg!(target_os = "macos") && super::webview::local_preview_url(&url.url) {
                        targets.push((url.url.clone(), None));
                        return false;
                    }
                    if let Ok(parsed) = url::Url::parse(&url.url)
                        && parsed.scheme() == "file"
                        && let Ok(path) = parsed.to_file_path()
                    {
                        let line = parsed.fragment().and_then(|frag| {
                            let trimmed = frag
                                .strip_prefix('L')
                                .or_else(|| frag.strip_prefix('l'))
                                .unwrap_or(frag);
                            trimmed
                                .split('-')
                                .next()
                                .and_then(|s| s.trim().parse::<usize>().ok())
                                .filter(|&n| n > 0)
                        });
                        targets.push((path.display().to_string(), line));
                        return false;
                    }
                    let (base, line) = crate::markdown::split_file_line(&url.url);
                    if !base.contains(':')
                        && !base.starts_with('#')
                        && std::path::Path::new(base).extension().is_some()
                    {
                        targets.push((base.to_owned(), line));
                        return false;
                    }
                }
                true
            })
        });
        for (target, line) in targets {
            let result = if let Some(line_num) = line {
                let candidate = Path::new(&target);
                let resolved = if candidate.is_absolute() {
                    candidate.to_path_buf()
                } else {
                    let stripped = target.strip_prefix("./").unwrap_or(&target);
                    self.prefs.cwd.join(stripped)
                };
                self.files
                    .open_workspace_source(ctx, &self.prefs.cwd, &resolved, Some(line_num))
                    .or_else(|_| self.files.open_target(ctx, &self.prefs.cwd, &target))
            } else {
                self.files.open_target(ctx, &self.prefs.cwd, &target)
            };
            if let Err(error) = result {
                self.state.notice = Some(error);
            } else {
                self.inspector_panel = false;
                self.files.hide_web();
            }
            ctx.request_repaint();
        }
    }
}

// Validate without touching viewer state or starting reads/Chromium. Queued and
// immediate opens use the same policy; deferred paths resolve in the owner's cwd.
fn preview_target(root: &Path, target: &str) -> Result<String, String> {
    if target.is_empty() || target.len() > 8192 || target.chars().any(char::is_control) {
        return Err("Invalid file path or URL.".into());
    }
    if super::webview::web_url(target) {
        if !cfg!(target_os = "macos") {
            return Err("Bundled Chromium is currently packaged for macOS only.".into());
        }
        return Ok(target.into());
    }
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
        return Err("Sensitive files and parent-directory traversal cannot be previewed.".into());
    }
    Ok(path.display().to_string())
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

#[cfg(test)]
fn read_content(root: &Path, path: &Path, external: bool) -> Result<Content, String> {
    read_content_budgeted(root, path, external, None, None)
}

fn read_content_budgeted(
    root: &Path,
    path: &Path,
    external: bool,
    budget: Option<&budget::Lease>,
    cancel: Option<&AtomicBool>,
) -> Result<Content, String> {
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
    let extension = path
        .extension()
        .unwrap_or_default()
        .to_string_lossy()
        .to_ascii_lowercase();
    if matches!(extension.as_str(), "png" | "jpg" | "jpeg" | "gif" | "webp") {
        let (file, metadata) = open_preview_file(&path)?;
        if metadata.len() > 16 * 1024 * 1024 {
            return Err("Image must be a regular file under 16 MiB.".into());
        }
        let mut bytes = Vec::new();
        file.take(16 * 1024 * 1024 + 1)
            .read_to_end(&mut bytes)
            .map_err(|e| e.to_string())?;
        if bytes.len() > 16 * 1024 * 1024 {
            return Err("Image exceeds 16 MiB.".into());
        }
        return image_preview::decode(&bytes, budget, cancel).map(Content::Image);
    }
    let source = read_preview_resolved(&path)?;
    Ok(match extension.as_str() {
        "csv" => Content::Table(delimited::Preview::new(source, b',')),
        "tsv" => Content::Table(delimited::Preview::new(source, b'\t')),
        "svg" => Content::Svg(Box::new(svg::Preview::load(source, budget, cancel))),
        _ => Content::Text(source),
    })
}

fn is_html(path: &Path) -> bool {
    path.extension().is_some_and(|extension| {
        extension.eq_ignore_ascii_case("html") || extension.eq_ignore_ascii_case("htm")
    })
}

fn open_preview_file(path: &Path) -> Result<(fs::File, fs::Metadata), String> {
    let expected = fs::symlink_metadata(path).map_err(|e| format!("Cannot read file: {e}"))?;
    open_preflighted_preview_file(path, &expected)
}

fn open_preflighted_preview_file(
    path: &Path,
    expected: &fs::Metadata,
) -> Result<(fs::File, fs::Metadata), String> {
    // Never open a known FIFO/device, even for images. Unix nonblocking opens
    // also cover replacement with a FIFO after this preflight.
    if !expected.is_file() {
        return Err("Only regular files can be previewed.".into());
    }
    let file = open_preview_handle(path)?;
    let metadata = file
        .metadata()
        .map_err(|e| format!("Cannot read file: {e}"))?;
    if !metadata.is_file() {
        return Err("Only regular files can be previewed.".into());
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        if expected.dev() != metadata.dev() || expected.ino() != metadata.ino() {
            return Err("File changed while opening its preview. Refresh to try again.".into());
        }
    }
    Ok((file, metadata))
}

#[cfg(unix)]
fn open_preview_handle(path: &Path) -> Result<fs::File, String> {
    use std::{
        ffi::CString,
        os::{
            fd::{AsRawFd, FromRawFd},
            unix::{ffi::OsStrExt, fs::OpenOptionsExt},
        },
    };
    // Policy resolution above returns absolute, alias-resolved paths. Walk
    // descriptors so a parent swapped for a symlink cannot bypass that policy.
    if !path.is_absolute() {
        return Err("Cannot safely open a relative preview path.".into());
    }
    let mut file = fs::OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_CLOEXEC)
        .open("/")
        .map_err(|e| format!("Cannot open file: {e}"))?;
    let mut parts = path.components().peekable();
    while let Some(part) = parts.next() {
        let name = match part {
            Component::RootDir => continue,
            Component::Normal(name) => {
                CString::new(name.as_bytes()).map_err(|_| "Invalid preview path.".to_owned())?
            }
            _ => return Err("Invalid preview path.".into()),
        };
        let flags = libc::O_RDONLY | libc::O_NOFOLLOW | libc::O_NONBLOCK | libc::O_CLOEXEC;
        let flags = if parts.peek().is_some() {
            flags | libc::O_DIRECTORY
        } else {
            flags
        };
        // SAFETY: name is NUL-terminated and file owns the live parent descriptor.
        let fd = unsafe { libc::openat(file.as_raw_fd(), name.as_ptr(), flags) };
        if fd < 0 {
            return Err(format!(
                "Cannot safely open file: {}",
                std::io::Error::last_os_error()
            ));
        }
        // SAFETY: openat returned a new descriptor, now owned solely by File.
        file = unsafe { fs::File::from_raw_fd(fd) };
    }
    Ok(file)
}

#[cfg(not(unix))]
fn open_preview_handle(path: &Path) -> Result<fs::File, String> {
    fs::File::open(path).map_err(|e| format!("Cannot open file: {e}"))
}

#[cfg(test)]
fn read_preview(root: &Path, path: &Path) -> Result<String, String> {
    let path = within_workspace(root, path)?;
    read_preview_resolved(&path)
}

fn read_preview_resolved(path: &Path) -> Result<String, String> {
    let (file, metadata) = open_preview_file(path)?;
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

pub(super) fn quick_paths(
    root: &Path,
    query: &str,
    cancel: &AtomicBool,
) -> Result<(Vec<PathBuf>, bool), String> {
    quick_paths::discover(root, query, cancel)
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
mod auto_restore_tests;
#[cfg(test)]
mod block_capture_tests;
#[cfg(all(test, target_os = "macos"))]
mod navigation_tests;
mod path_menu;
#[cfg(test)]
mod question_tests;
#[cfg(test)]
mod read_safety_tests;
#[cfg(test)]
mod restore_tests;
#[cfg(test)]
mod source_selection_tests;
#[cfg(test)]
pub(crate) mod tests;
#[cfg(test)]
mod toolbar_tests;
