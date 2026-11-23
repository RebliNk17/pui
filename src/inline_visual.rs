//! Sandboxed inline HTML/CSS/SVG visuals in chat + live Browser preview snapshots.
//! Uses pure `resvg` for standalone static SVGs and windowless CEF OSR (opaque CSP origin)
//! for rich HTML/CSS/SVG visuals on macOS, with a clean Source fallback everywhere.
#[cfg(target_os = "macos")]
mod cef_osr;

use crate::{
    icons::{self, Icon},
    protocol::InlineVisual,
};
use eframe::egui::{
    self, Align, ColorImage, FontId, Layout, RichText, Sense, Stroke, TextureHandle,
    TextureOptions, Vec2,
};
use std::{
    collections::HashMap,
    hash::{Hash, Hasher},
    sync::{Arc, Mutex, mpsc},
    thread,
};

const MAX_VISUAL_TEXTURES: usize = 24;
const MAX_PAGE_SNAPSHOTS: usize = 16;

/// Shared, bounded cache of Browser tab page screenshots + titles for inline `pui_open` cards.
#[derive(Clone, Default)]
pub struct PageSnapshots {
    inner: Arc<Mutex<HashMap<String, PageSnapshotData>>>,
}

#[derive(Clone)]
struct PageSnapshotData {
    title: String,
    image: Option<ColorImage>,
    revision: u64,
}

impl PageSnapshots {
    pub fn record_title(&self, target: &str, title: String) {
        if target.is_empty() || title.is_empty() {
            return;
        }
        if let Ok(mut map) = self.inner.lock() {
            if map.len() >= MAX_PAGE_SNAPSHOTS && !map.contains_key(target) {
                if let Some(first) = map.keys().next().cloned() {
                    map.remove(&first);
                }
            }
            let entry = map
                .entry(target.to_owned())
                .or_insert_with(|| PageSnapshotData {
                    title: String::new(),
                    image: None,
                    revision: 0,
                });
            if entry.title != title {
                entry.title = title;
            }
        }
    }

    pub fn record_image(&self, target: &str, title: Option<String>, image: ColorImage) {
        if target.is_empty() {
            return;
        }
        if let Ok(mut map) = self.inner.lock() {
            if map.len() >= MAX_PAGE_SNAPSHOTS && !map.contains_key(target) {
                if let Some(first) = map.keys().next().cloned() {
                    map.remove(&first);
                }
            }
            let entry = map
                .entry(target.to_owned())
                .or_insert_with(|| PageSnapshotData {
                    title: String::new(),
                    image: None,
                    revision: 0,
                });
            if let Some(title) = title.filter(|t| !t.is_empty()) {
                entry.title = title;
            }
            entry.image = Some(image);
            entry.revision = entry.revision.wrapping_add(1);
        }
    }

    pub fn title(&self, target: &str) -> Option<String> {
        self.inner
            .lock()
            .ok()
            .and_then(|map| map.get(target).map(|d| d.title.clone()))
            .filter(|t| !t.is_empty())
    }

    fn snapshot(&self, target: &str) -> Option<(u64, ColorImage)> {
        self.inner.lock().ok().and_then(|map| {
            let entry = map.get(target)?;
            entry
                .image
                .as_ref()
                .map(|img| (entry.revision, img.clone()))
        })
    }
}

/// Wrap inline visual markup in the exact same restrictive CSP as Pui's local HTML viewer,
/// plus subtle dark/light base typography defaults that agent `<style>` rules can override.
pub fn prepare_visual_html(source: &str, dark: bool) -> String {
    let (scheme, bg, fg, border) = if dark {
        ("dark", "#191919", "#e8e8e6", "#323230")
    } else {
        ("light", "#fafaf8", "#262624", "#d7d7d2")
    };
    format!(
        concat!(
            "<!doctype html>",
            "<meta http-equiv=\"Content-Security-Policy\" content=\"default-src 'none'; script-src 'unsafe-inline'; connect-src 'none'; style-src 'unsafe-inline'; img-src data:; font-src data:; base-uri 'none'; form-action 'none'; frame-src 'none'\">",
            "<meta charset=\"utf-8\">",
            "<style>",
            ":root{{color-scheme:{scheme};--pui-bg:{bg};--pui-fg:{fg};--pui-border:{border};}}",
            "html,body{{margin:0;padding:0;background:{bg};color:{fg};font-family:-apple-system,BlinkMacSystemFont,'Segoe UI',Roboto,sans-serif;font-size:13px;line-height:1.45;box-sizing:border-box;}}",
            "body{{padding:14px;overflow-x:auto;}}",
            "*,*::before,*::after{{box-sizing:border-box;}}",
            "svg{{max-width:100%;height:auto;display:block;}}",
            "::-webkit-scrollbar{{width:10px;height:10px;}}",
            "::-webkit-scrollbar-track{{background:transparent;}}",
            "::-webkit-scrollbar-thumb{{background:{border};border-radius:5px;border:2px solid {bg};}}",
            "::-webkit-scrollbar-thumb:hover{{background:{fg}55;}}",
            "</style>",
            "{source}"
        ),
        scheme = scheme,
        bg = bg,
        fg = fg,
        border = border,
        source = source,
    )
}

pub fn is_standalone_svg(source: &str) -> bool {
    let trimmed = source.trim();
    let body = trimmed
        .strip_prefix("<?xml")
        .and_then(|rest| rest.find("?>").map(|end| rest[end + 2..].trim_start()))
        .unwrap_or(trimmed);
    body.starts_with("<svg") && body.ends_with("</svg>")
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
struct VisualKey {
    content_hash: u64,
    width: u32,
    dark: bool,
}

fn hash_visual(html: &str, height: Option<u32>) -> u64 {
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    html.hash(&mut hasher);
    height.hash(&mut hasher);
    hasher.finish()
}

fn quantize_width(width: f32) -> u32 {
    // Quantize to 16pt increments so tiny window resizes don't thrash captures.
    let clamped = width.clamp(220.0, 960.0);
    ((clamped / 16.0).round() as u32 * 16).max(220)
}

enum VisualEntry {
    Pending,
    Ready(TextureHandle),
    Failed(String),
}

struct CachedVisual {
    entry: VisualEntry,
    used: u64,
}

struct SvgCompleted {
    key: VisualKey,
    html: String,
    width: u32,
    initial_height: u32,
    scale: f32,
    result: Result<ColorImage, String>,
}

pub struct VisualOutcome {
    pub changed: bool,
    pub expand: bool,
}

/// The one visual currently running as a live, interactive page.
#[cfg(target_os = "macos")]
struct LiveSession {
    tool_id: String,
    visual: cef_osr::LiveVisual,
    texture: Option<TextureHandle>,
    focused: bool,
    hovered: bool,
    left_down: bool,
    last_shown: std::time::Instant,
    last_active: std::time::Instant,
    last_pos: (i32, i32),
}

/// Close a live page scrolled out of view, or idle (not hovered/focused) this long.
#[cfg(target_os = "macos")]
const LIVE_HIDDEN: std::time::Duration = std::time::Duration::from_millis(1500);
#[cfg(target_os = "macos")]
const LIVE_IDLE: std::time::Duration = std::time::Duration::from_secs(30);

pub struct InlineVisualHost {
    cache: HashMap<VisualKey, CachedVisual>,
    page_textures: HashMap<String, (u64, TextureHandle)>,
    pub page_snapshots: PageSnapshots,
    svg_tx: mpsc::Sender<SvgCompleted>,
    svg_rx: mpsc::Receiver<SvgCompleted>,
    svg_pending: usize,
    #[cfg(target_os = "macos")]
    osr: cef_osr::OsrPool,
    #[cfg(target_os = "macos")]
    live: Option<LiveSession>,
    tick: u64,
}

impl Default for InlineVisualHost {
    fn default() -> Self {
        let (svg_tx, svg_rx) = mpsc::channel();
        Self {
            cache: HashMap::new(),
            page_textures: HashMap::new(),
            page_snapshots: PageSnapshots::default(),
            svg_tx,
            svg_rx,
            svg_pending: 0,
            #[cfg(target_os = "macos")]
            osr: cef_osr::OsrPool::default(),
            #[cfg(target_os = "macos")]
            live: None,
            tick: 0,
        }
    }
}

impl InlineVisualHost {
    pub fn poll(&mut self, ctx: &egui::Context) -> bool {
        let mut changed = false;
        #[cfg(target_os = "macos")]
        {
            let expired = self.live.as_ref().is_some_and(|live| {
                live.last_shown.elapsed() > LIVE_HIDDEN
                    || (!live.focused && !live.hovered && live.last_active.elapsed() > LIVE_IDLE)
            });
            if expired {
                self.live = None; // Drop closes the offscreen browser.
                changed = true;
            }
            if let Some(live) = &mut self.live {
                live.visual.poll();
                if let Some(image) = live.visual.take_frame() {
                    match &mut live.texture {
                        Some(texture) => texture.set(image, TextureOptions::LINEAR),
                        None => {
                            live.texture = Some(ctx.load_texture(
                                format!("inline-visual-live-{}", live.tool_id),
                                image,
                                TextureOptions::LINEAR,
                            ))
                        }
                    }
                    changed = true;
                }
                if live.visual.error().is_some() {
                    self.live = None;
                }
            }
        }
        for done in self.svg_rx.try_iter() {
            self.svg_pending = self.svg_pending.saturating_sub(1);
            match done.result {
                Ok(image) => {
                    let texture = ctx.load_texture(
                        format!(
                            "inline-svg-{}-{}-{}",
                            done.key.content_hash, done.key.width, done.key.dark
                        ),
                        image,
                        TextureOptions::LINEAR,
                    );
                    self.cache.insert(
                        done.key,
                        CachedVisual {
                            entry: VisualEntry::Ready(texture),
                            used: self.tick,
                        },
                    );
                    changed = true;
                }
                Err(error) => {
                    #[cfg(target_os = "macos")]
                    {
                        let key = cef_osr::CaptureKey {
                            content_hash: done.key.content_hash,
                            width: done.key.width,
                            dark: done.key.dark,
                        };
                        let html = prepare_visual_html(&done.html, done.key.dark);
                        if let Err(osr_err) = self.osr.enqueue(cef_osr::CaptureRequest {
                            key,
                            html,
                            width: done.width,
                            initial_height: done.initial_height,
                            scale: done.scale,
                        }) {
                            self.cache.insert(
                                done.key,
                                CachedVisual {
                                    entry: VisualEntry::Failed(format!("{error} ({osr_err})")),
                                    used: self.tick,
                                },
                            );
                        }
                        changed = true;
                    }
                    #[cfg(not(target_os = "macos"))]
                    {
                        self.cache.insert(
                            done.key,
                            CachedVisual {
                                entry: VisualEntry::Failed(error),
                                used: self.tick,
                            },
                        );
                        changed = true;
                    }
                }
            }
        }
        #[cfg(target_os = "macos")]
        for done in self.osr.poll(ctx) {
            let key = VisualKey {
                content_hash: done.key.content_hash,
                width: done.key.width,
                dark: done.key.dark,
            };
            let entry = match done.result {
                Ok(image) => VisualEntry::Ready(ctx.load_texture(
                    format!(
                        "inline-visual-{}-{}-{}",
                        key.content_hash, key.width, key.dark
                    ),
                    image,
                    TextureOptions::LINEAR,
                )),
                Err(error) => VisualEntry::Failed(error),
            };
            self.cache.insert(
                key,
                CachedVisual {
                    entry,
                    used: self.tick,
                },
            );
            changed = true;
        }
        changed
    }

    pub fn release_textures(&mut self) {
        let snapshots = self.page_snapshots.clone();
        #[cfg(target_os = "macos")]
        {
            self.live = None;
            self.osr.release_all();
        }
        *self = Self::default();
        self.page_snapshots = snapshots;
    }

    /// Fetch or upload a cached GPU texture for a Browser preview card (`pui_open`).
    pub fn page_texture(&mut self, ctx: &egui::Context, target: &str) -> Option<TextureHandle> {
        let (revision, image) = self.page_snapshots.snapshot(target)?;
        if let Some((cached_rev, tex)) = self.page_textures.get(target)
            && *cached_rev == revision
        {
            return Some(tex.clone());
        }
        if self.page_textures.len() >= MAX_PAGE_SNAPSHOTS {
            if let Some(first) = self.page_textures.keys().next().cloned() {
                self.page_textures.remove(&first);
            }
        }
        let tex = ctx.load_texture(
            format!("page-preview-{target}-{revision}"),
            image,
            TextureOptions::LINEAR,
        );
        self.page_textures
            .insert(target.to_owned(), (revision, tex.clone()));
        Some(tex)
    }

    fn request(
        &mut self,
        ui: &egui::Ui,
        visual: &InlineVisual,
        width: u32,
        dark: bool,
    ) -> VisualKey {
        self.tick = self.tick.wrapping_add(1);
        let key = VisualKey {
            content_hash: hash_visual(&visual.html, visual.height),
            width,
            dark,
        };
        if let Some(entry) = self.cache.get_mut(&key) {
            entry.used = self.tick;
            return key;
        }
        if self.cache.len() >= MAX_VISUAL_TEXTURES {
            let oldest = self
                .cache
                .iter()
                .filter(|(_, v)| !matches!(v.entry, VisualEntry::Pending))
                .min_by_key(|(_, v)| v.used)
                .map(|(k, _)| k.clone());
            if let Some(oldest) = oldest {
                self.cache.remove(&oldest);
            }
        }
        let initial_height = visual.height.unwrap_or(360).clamp(120, 820);
        let scale = ui.ctx().pixels_per_point().clamp(1.0, 3.0);
        if is_standalone_svg(&visual.html) && self.svg_pending < 2 {
            self.cache.insert(
                key.clone(),
                CachedVisual {
                    entry: VisualEntry::Pending,
                    used: self.tick,
                },
            );
            self.svg_pending += 1;
            let html = visual.html.clone();
            let tx = self.svg_tx.clone();
            let ctx = ui.ctx().clone();
            let key_clone = key.clone();
            thread::spawn(move || {
                let result = crate::svg_preview::render(&html, None, |_| Ok(())).map(|raster| {
                    // Convert premultiplied RGBA from tiny-skia to unmultiplied ColorImage.
                    let mut unmul = raster.rgba;
                    for px in unmul.chunks_exact_mut(4) {
                        let a = px[3];
                        if a > 0 && a < 255 {
                            let a_u16 = u16::from(a);
                            px[0] = ((u16::from(px[0]) * 255) / a_u16).min(255) as u8;
                            px[1] = ((u16::from(px[1]) * 255) / a_u16).min(255) as u8;
                            px[2] = ((u16::from(px[2]) * 255) / a_u16).min(255) as u8;
                        }
                    }
                    ColorImage::from_rgba_unmultiplied(raster.size, &unmul)
                });
                let _ = tx.send(SvgCompleted {
                    key: key_clone,
                    html,
                    width,
                    initial_height,
                    scale,
                    result,
                });
                ctx.request_repaint();
            });
            return key;
        }
        #[cfg(target_os = "macos")]
        {
            if let Some(reason) = self.osr.unavailable_reason() {
                self.cache.insert(
                    key.clone(),
                    CachedVisual {
                        entry: VisualEntry::Failed(reason.to_owned()),
                        used: self.tick,
                    },
                );
                return key;
            }
            let osr_key = cef_osr::CaptureKey {
                content_hash: key.content_hash,
                width: key.width,
                dark: key.dark,
            };
            let html = prepare_visual_html(&visual.html, dark);
            match self.osr.enqueue(cef_osr::CaptureRequest {
                key: osr_key,
                html,
                width,
                initial_height,
                scale,
            }) {
                Ok(()) => {
                    self.cache.insert(
                        key.clone(),
                        CachedVisual {
                            entry: VisualEntry::Pending,
                            used: self.tick,
                        },
                    );
                }
                Err(error) => {
                    self.cache.insert(
                        key.clone(),
                        CachedVisual {
                            entry: VisualEntry::Failed(error),
                            used: self.tick,
                        },
                    );
                }
            }
        }
        #[cfg(not(target_os = "macos"))]
        {
            self.cache.insert(
                key.clone(),
                CachedVisual {
                    entry: VisualEntry::Failed(
                        "Inline HTML visuals require bundled Chromium (macOS). Toggle Source to inspect HTML."
                            .into(),
                    ),
                    used: self.tick,
                },
            );
        }
        key
    }

    /// Run the hovered visual as a live page: forward pointer, wheel and (once
    /// clicked) keyboard input, and return its latest frame for painting.
    #[cfg(target_os = "macos")]
    fn interact(
        &mut self,
        ui: &mut egui::Ui,
        tool_id: &str,
        visual: &InlineVisual,
        key: &VisualKey,
        dark: bool,
        rect: egui::Rect,
    ) -> Option<egui::TextureId> {
        use egui::{Event, PointerButton};
        let ctx = ui.ctx().clone();
        let osr_key = cef_osr::CaptureKey {
            content_hash: key.content_hash,
            width: key.width,
            dark,
        };
        let pointer = ctx.input(|i| i.pointer.hover_pos());
        let inside = pointer.is_some_and(|p| rect.contains(p))
            && ui.is_rect_visible(rect)
            && ui.ui_contains_pointer()
            && !egui::Popup::is_any_open(&ctx);
        let (width, height) = (rect.width().round() as i32, rect.height().round() as i32);
        let current = self
            .live
            .as_ref()
            .is_some_and(|live| live.tool_id == tool_id && live.visual.key == osr_key);
        if inside && !current {
            self.live = None;
            let html = prepare_visual_html(&visual.html, dark);
            let scale = ctx.pixels_per_point().clamp(1.0, 3.0);
            if let Ok(visual) = self
                .osr
                .start_live(&ctx, osr_key, &html, width, height, scale)
            {
                let now = std::time::Instant::now();
                self.live = Some(LiveSession {
                    tool_id: tool_id.to_owned(),
                    visual,
                    texture: None,
                    focused: false,
                    hovered: false,
                    left_down: false,
                    last_shown: now,
                    last_active: now,
                    last_pos: (0, 0),
                });
            }
        }
        let live = self.live.as_mut().filter(|live| live.tool_id == tool_id)?;
        let now = std::time::Instant::now();
        live.last_shown = now;
        live.visual.resize(width, height);
        let focus_id = ui.make_persistent_id(("inline-visual-focus", tool_id));
        let local = |p: egui::Pos2| {
            (
                (p.x - rect.min.x).round() as i32,
                (p.y - rect.min.y).round() as i32,
            )
        };
        let modifiers = ctx.input(|i| i.modifiers);
        let events = ctx.input(|i| i.events.clone());
        for event in &events {
            match event {
                Event::PointerMoved(pos) if rect.contains(*pos) => {
                    let (x, y) = local(*pos);
                    live.last_pos = (x, y);
                    live.hovered = true;
                    live.last_active = now;
                    live.visual.mouse_move(
                        x,
                        y,
                        cef_osr::event_flags(modifiers, live.left_down),
                        false,
                    );
                }
                Event::PointerButton {
                    pos,
                    button,
                    pressed,
                    modifiers,
                } if rect.contains(*pos) => {
                    let (x, y) = local(*pos);
                    if *button == PointerButton::Primary {
                        live.left_down = *pressed;
                    }
                    if *pressed && !live.focused {
                        live.focused = true;
                        live.visual.focus(true);
                        ctx.memory_mut(|m| m.request_focus(focus_id));
                    }
                    let count = if ctx.input(|i| i.pointer.button_double_clicked(*button)) {
                        2
                    } else {
                        1
                    };
                    live.last_active = now;
                    live.visual.click(
                        x,
                        y,
                        *button,
                        !*pressed,
                        count,
                        cef_osr::event_flags(*modifiers, false),
                    );
                }
                Event::PointerButton { pressed: true, .. } if live.focused => {
                    live.focused = false;
                    live.visual.focus(false);
                }
                Event::PointerButton {
                    pressed: false,
                    button: PointerButton::Primary,
                    ..
                } if live.left_down => {
                    // Drag released outside: finish the page's selection/drag.
                    live.left_down = false;
                    let (x, y) = live.last_pos;
                    live.visual.click(x, y, PointerButton::Primary, true, 1, 0);
                }
                _ => {}
            }
        }
        if live.hovered && !inside {
            live.hovered = false;
            let (x, y) = live.last_pos;
            live.visual.mouse_move(x, y, 0, true);
        }
        // Wheel scrolls the page only when it has more to show; otherwise the chat scrolls.
        if inside && live.visual.scrollable() {
            let delta = ctx.input(|i| i.smooth_scroll_delta);
            if delta != Vec2::ZERO {
                let (x, y) = live.last_pos;
                live.visual
                    .wheel(x, y, delta.x.round() as i32, delta.y.round() as i32, 0);
                live.last_active = now;
                ctx.input_mut(|i| {
                    i.smooth_scroll_delta = Vec2::ZERO;
                    i.raw_scroll_delta = Vec2::ZERO;
                });
            }
        }
        if live.focused
            && !ctx.memory(|m| m.has_focus(focus_id))
            && ctx.memory(|m| m.focused().is_some())
        {
            // Another control (e.g. the composer) took keyboard focus.
            live.focused = false;
            live.visual.focus(false);
        }
        if live.focused {
            ctx.memory_mut(|m| {
                m.request_focus(focus_id);
                m.set_focus_lock_filter(
                    focus_id,
                    egui::EventFilter {
                        tab: true,
                        horizontal_arrows: true,
                        vertical_arrows: true,
                        escape: true,
                    },
                )
            });
            let typed = ctx.input_mut(|i| {
                let taken: Vec<Event> = i
                    .events
                    .iter()
                    .filter(|e| {
                        matches!(
                            e,
                            Event::Text(_)
                                | Event::Paste(_)
                                | Event::Copy
                                | Event::Cut
                                | Event::Key { .. }
                        )
                    })
                    .cloned()
                    .collect();
                i.events.retain(|e| {
                    !matches!(
                        e,
                        Event::Text(_)
                            | Event::Paste(_)
                            | Event::Copy
                            | Event::Cut
                            | Event::Key { .. }
                    )
                });
                taken
            });
            for event in typed {
                match event {
                    Event::Key {
                        key: egui::Key::Escape,
                        pressed: true,
                        ..
                    } => {
                        live.focused = false;
                        live.visual.focus(false);
                        ctx.memory_mut(|m| m.surrender_focus(focus_id));
                    }
                    Event::Text(text) => live.visual.text(&text),
                    Event::Paste(_) => live.visual.edit(cef_osr::Edit::Paste),
                    Event::Copy => live.visual.edit(cef_osr::Edit::Copy),
                    Event::Cut => live.visual.edit(cef_osr::Edit::Cut),
                    Event::Key {
                        key: egui::Key::A,
                        pressed: true,
                        modifiers,
                        ..
                    } if modifiers.mac_cmd => live.visual.edit(cef_osr::Edit::SelectAll),
                    Event::Key {
                        key,
                        pressed,
                        modifiers,
                        ..
                    } => {
                        live.visual.key(key, pressed, modifiers);
                    }
                    _ => {}
                }
                live.last_active = now;
            }
        }
        if inside {
            ctx.set_cursor_icon(egui::CursorIcon::Default);
        }
        live.texture.as_ref().map(TextureHandle::id)
    }

    pub fn show(
        &mut self,
        ui: &mut egui::Ui,
        tool_id: &str,
        visual: &InlineVisual,
    ) -> VisualOutcome {
        let colors = crate::theme::palette(ui.ctx());
        let dark = ui.ctx().style().visuals.dark_mode;
        let open_id = ui.make_persistent_id(("inline-visual-open", tool_id));
        let source_id = ui.make_persistent_id(("inline-visual-src", tool_id));
        let mut open = ui
            .ctx()
            .data(|d| d.get_temp::<bool>(open_id))
            .unwrap_or(true);
        let mut show_source = ui
            .ctx()
            .data(|d| d.get_temp::<bool>(source_id))
            .unwrap_or(false);
        let before = (open, show_source);
        let mut expand = false;

        ui.add_space(6.0);
        // Header row matching "Made a visual  ⤢  v"
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 6.0;
            let header_label = if visual.title.is_empty() {
                "Made a visual".to_owned()
            } else {
                format!("Made a visual · {}", visual.title)
            };
            let label_resp = ui
                .add(
                    egui::Label::new(RichText::new(header_label).size(12.0).color(colors.muted))
                        .truncate()
                        .sense(Sense::click()),
                )
                .on_hover_cursor(egui::CursorIcon::PointingHand);
            if label_resp.clicked() {
                open = !open;
            }
            if icons::button(ui, Icon::Expand, "Open interactive visual in Browser").clicked() {
                expand = true;
            }
            if icons::button(
                ui,
                Icon::Code,
                if show_source {
                    "Show rendered visual"
                } else {
                    "View HTML source"
                },
            )
            .clicked()
            {
                show_source = !show_source;
                open = true;
            }
            let (chev_rect, chev_resp) = ui.allocate_exact_size(Vec2::splat(20.0), Sense::click());
            icons::chevron(
                ui,
                egui::Rect::from_center_size(chev_rect.center(), Vec2::splat(12.0)),
                open,
                if chev_resp.hovered() {
                    colors.text
                } else {
                    colors.dim
                },
            );
            if chev_resp
                .on_hover_cursor(egui::CursorIcon::PointingHand)
                .on_hover_text(if open {
                    "Collapse visual"
                } else {
                    "Expand visual"
                })
                .clicked()
            {
                open = !open;
            }
        });

        if open {
            ui.add_space(4.0);
            let card_width = ui.available_width().max(180.0);
            egui::Frame::new()
                .fill(colors.surface)
                .stroke(Stroke::new(1.0_f32, colors.border))
                .corner_radius(10)
                .inner_margin(if show_source { 10 } else { 0 })
                .show(ui, |ui| {
                    ui.set_width((card_width - if show_source { 22.0 } else { 2.0 }).max(160.0));
                    if show_source {
                        ui.horizontal(|ui| {
                            ui.label(
                                RichText::new("HTML")
                                    .size(10.5)
                                    .monospace()
                                    .color(colors.dim),
                            );
                            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                                if icons::button(ui, Icon::Copy, "Copy HTML source").clicked() {
                                    ui.ctx().copy_text(visual.html.clone());
                                }
                            });
                        });
                        egui::ScrollArea::both()
                            .id_salt(("inline-visual-source-scroll", tool_id))
                            .max_height(320.0)
                            .auto_shrink([false, true])
                            .show(ui, |ui| {
                                ui.add(
                                    egui::Label::new(
                                        RichText::new(visual.html.trim_end())
                                            .font(FontId::monospace(11.5))
                                            .color(colors.code_text),
                                    )
                                    .selectable(true),
                                );
                            });
                    } else {
                        let inner_width = ui.available_width().max(160.0);
                        let key = self.request(ui, visual, quantize_width(inner_width), dark);
                        let entry = self.cache.get(&key).map(|c| match &c.entry {
                            VisualEntry::Ready(tex) => (Some((tex.id(), tex.size_vec2())), None),
                            VisualEntry::Failed(err) => (None, Some(err.clone())),
                            VisualEntry::Pending => (None, None),
                        });
                        match entry {
                            Some((Some((tex_id, tex_size)), _)) => {
                                let ppp = ui.ctx().pixels_per_point().max(1.0);
                                let logical = tex_size / ppp;
                                let scale = (inner_width / logical.x.max(1.0)).min(1.0);
                                let display = Vec2::new(
                                    (logical.x * scale).min(inner_width),
                                    (logical.y * scale).clamp(80.0, 820.0),
                                );
                                let (rect, _resp) = ui.allocate_exact_size(display, Sense::hover());
                                #[cfg(target_os = "macos")]
                                let painted = self.interact(ui, tool_id, visual, &key, dark, rect);
                                #[cfg(not(target_os = "macos"))]
                                let painted = None;
                                if ui.is_rect_visible(rect) {
                                    egui::Image::new((painted.unwrap_or(tex_id), display))
                                        .corner_radius(9)
                                        .paint_at(ui, rect);
                                }
                            }
                            Some((None, Some(error))) => {
                                egui::Frame::new().inner_margin(12).show(ui, |ui| {
                                    ui.label(RichText::new(error).size(11.5).color(colors.muted));
                                    ui.add_space(6.0);
                                    ui.horizontal(|ui| {
                                        if ui.button("Open in Browser").clicked() {
                                            expand = true;
                                        }
                                        if ui.button("View Source").clicked() {
                                            show_source = true;
                                        }
                                    });
                                });
                            }
                            _ => {
                                let h = visual.height.unwrap_or(240).clamp(120, 420) as f32;
                                let (rect, _) = ui
                                    .allocate_exact_size(Vec2::new(inner_width, h), Sense::hover());
                                if ui.is_rect_visible(rect) {
                                    ui.painter().text(
                                        rect.center(),
                                        egui::Align2::CENTER_CENTER,
                                        "Rendering visual…",
                                        FontId::proportional(11.5),
                                        colors.muted,
                                    );
                                }
                            }
                        }
                    }
                });
        }
        ui.add_space(6.0);
        if (open, show_source) != before {
            ui.ctx().data_mut(|d| {
                d.insert_temp(open_id, open);
                d.insert_temp(source_id, show_source);
            });
        }
        VisualOutcome {
            changed: (open, show_source) != before,
            expand,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn prepared_visual_html_enforces_strict_csp_and_theme_defaults() {
        let dark = prepare_visual_html("<div>Hello</div>", true);
        assert!(dark.contains("default-src 'none'"));
        assert!(dark.contains("connect-src 'none'"));
        assert!(dark.contains("frame-src 'none'"));
        assert!(dark.contains("form-action 'none'"));
        assert!(dark.contains("color-scheme:dark"));
        assert!(dark.ends_with("<div>Hello</div>"));

        let light = prepare_visual_html("<div>Hello</div>", false);
        assert!(light.contains("color-scheme:light"));
        // Taller visuals show a visible scrollbar instead of being cut off silently.
        assert!(dark.contains("::-webkit-scrollbar{width:10px"));
        assert!(dark.contains("::-webkit-scrollbar-thumb{background:#323230"));
    }

    #[test]
    fn standalone_svg_detection_handles_xml_prolog_and_rejects_html() {
        assert!(is_standalone_svg("<svg width=\"10\" height=\"10\"></svg>"));
        assert!(is_standalone_svg(
            "<?xml version=\"1.0\"?>\n<svg width=\"10\" height=\"10\"></svg>"
        ));
        assert!(!is_standalone_svg("<div><svg></svg></div>"));
    }

    #[test]
    fn page_snapshots_store_title_and_image_bounded() {
        let snapshots = PageSnapshots::default();
        snapshots.record_title("http://127.0.0.1:8765/", "Error response".into());
        assert_eq!(
            snapshots.title("http://127.0.0.1:8765/").as_deref(),
            Some("Error response")
        );
        snapshots.record_image(
            "http://127.0.0.1:8765/",
            None,
            ColorImage::filled([16, 12], egui::Color32::BLACK),
        );
        let (rev, img) = snapshots.snapshot("http://127.0.0.1:8765/").unwrap();
        assert_eq!(rev, 1);
        assert_eq!(img.size, [16, 12]);
    }
}
