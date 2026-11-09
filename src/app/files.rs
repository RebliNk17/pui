//! Docked, read-only file browser. Only explicit file clicks read content.
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
struct OpenFile {
    path: PathBuf,
    content: Option<Result<String, String>>,
    rx: Option<Receiver<Result<String, String>>>,
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
    root: PathBuf,
    directories: BTreeMap<PathBuf, Directory>,
    expanded_dirs: BTreeSet<PathBuf>,
    tabs: Vec<OpenFile>,
    selected: Option<PathBuf>,
    query: String,
    search: Option<Result<Search, String>>,
    search_rx: Option<Receiver<Result<Search, String>>>,
    search_cancel: Arc<AtomicBool>,
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

    pub(super) fn close(&mut self) {
        self.open = false;
    }

    pub(super) fn toggle(&mut self, ctx: &egui::Context, root: &Path) {
        self.open = !self.open;
        if self.open {
            self.workspace(ctx, root);
        }
    }

    fn workspace(&mut self, ctx: &egui::Context, root: &Path) {
        if self.root != root {
            self.search_cancel.store(true, Ordering::Relaxed);
            self.root = root.to_path_buf();
            self.directories.clear();
            self.expanded_dirs.clear();
            self.tabs.clear();
            self.selected = None;
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
        self.selected = Some(path.clone());
        if self.tabs.iter().any(|tab| tab.path == path) {
            return;
        }
        if self.tabs.len() == MAX_TABS {
            self.tabs.remove(0);
        }
        let root = self.root.clone();
        let ctx = ctx.clone();
        let (tx, rx) = mpsc::channel();
        self.tabs.push(OpenFile {
            path: path.clone(),
            content: None,
            rx: Some(rx),
        });
        std::thread::spawn(move || {
            let _ = tx.send(read_preview(&root, &path));
            ctx.request_repaint();
        });
    }

    fn close_tab(&mut self, index: usize) {
        let tab = self.tabs.remove(index);
        if self.selected.as_ref() == Some(&tab.path) {
            self.selected = self
                .tabs
                .get(index.min(self.tabs.len().saturating_sub(1)))
                .map(|tab| tab.path.clone());
        }
    }

    fn refresh(&mut self, ctx: &egui::Context) {
        let paths: Vec<_> = std::iter::once(self.root.clone())
            .chain(self.expanded_dirs.iter().cloned())
            .collect();
        self.directories.clear();
        for path in paths {
            self.load_directory(ctx, path);
        }
        if let Some(path) = self.selected.clone() {
            if let Some(index) = self.tabs.iter().position(|tab| tab.path == path) {
                self.tabs.remove(index);
            }
            self.select(ctx, path);
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

    fn poll(&mut self) {
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
        let colors = theme::palette(ui.ctx());
        let ctx = ui.ctx().clone();
        egui::TopBottomPanel::top("files-heading")
            .exact_height(44.0)
            .frame(egui::Frame::new().inner_margin(egui::Margin::symmetric(16, 0)))
            .show_inside(ui, |ui| {
                ui.horizontal_centered(|ui| {
                    ui.label(RichText::new("Files").size(13.0).color(colors.muted));
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        if icons::button(
                            ui,
                            if self.expanded {
                                Icon::Collapse
                            } else {
                                Icon::Expand
                            },
                            if self.expanded {
                                "Restore files panel"
                            } else {
                                "Expand files panel"
                            },
                        )
                        .clicked()
                        {
                            self.expanded = !self.expanded;
                        }
                        if icons::button(ui, Icon::Refresh, "Refresh files and selected preview")
                            .clicked()
                        {
                            self.refresh(&ctx);
                        }
                        if self.directories.values().any(|d| d.rx.is_some())
                            || self.tabs.iter().any(|t| t.rx.is_some())
                            || self.search_rx.is_some()
                        {
                            ui.label(theme::small(ui, "Loading…"));
                        }
                    });
                });
            });
        let tree_width = (ui.available_width() * 0.38).clamp(170.0, 300.0);
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
        if !self.tabs.is_empty() {
            let mut select = None;
            let mut close = None;
            egui::TopBottomPanel::top("open-file-tabs")
                .exact_height(36.0)
                .frame(egui::Frame::new().inner_margin(egui::Margin::symmetric(8, 0)))
                .show_inside(ui, |ui| {
                    egui::ScrollArea::horizontal()
                        .id_salt("file-tabs-scroll")
                        .show(ui, |ui| {
                            ui.horizontal(|ui| {
                                for (index, tab) in self.tabs.iter().enumerate() {
                                    ui.push_id(&tab.path, |ui| {
                                        let active = self.selected.as_ref() == Some(&tab.path);
                                        if ui
                                            .selectable_label(
                                                active,
                                                tab.path
                                                    .file_name()
                                                    .unwrap_or_default()
                                                    .to_string_lossy(),
                                            )
                                            .on_hover_text(tab.path.display().to_string())
                                            .clicked()
                                        {
                                            select = Some(tab.path.clone());
                                        }
                                        if icons::button(ui, Icon::Close, "Close file").clicked() {
                                            close = Some(index);
                                        }
                                    });
                                }
                            });
                        });
                });
            if let Some(path) = select {
                self.selected = Some(path);
            }
            if let Some(index) = close {
                self.close_tab(index);
            }
        }
        let Some(tab) = self
            .tabs
            .iter()
            .find(|tab| Some(&tab.path) == self.selected.as_ref())
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
                    "Explicit reads only. Sensitive, binary, oversized and symlinked files are blocked."))
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
            ui.add_space(12.0);
            match &tab.content {
                None => {
                    ui.label(theme::small(ui, "Loading file…"));
                }
                Some(Err(error)) => {
                    ui.add(egui::Label::new(theme::small(ui, error)).wrap());
                }
                Some(Ok(content)) => {
                    egui::ScrollArea::both()
                        .id_salt(("file-content", &tab.path))
                        .auto_shrink([false, false])
                        .show(ui, |ui| {
                            ui.add(
                                egui::Label::new(RichText::new(content).monospace().size(12.0))
                                    .wrap_mode(egui::TextWrapMode::Extend)
                                    .selectable(true),
                            );
                        });
                }
            }
        });
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
