//! Chromium browser UI; files and the agent never get a privileged page bridge.
use super::*;
use std::collections::BTreeMap;

/// Native pages composite above egui, hiding anything painted beneath them.
/// Keep the pane's left/right hairlines (panel separators and resize handles)
/// visible along the full height by leaving them uncovered.
pub(super) const NATIVE_EDGE_INSET: f32 = 2.0;

pub(super) mod automation;
pub(super) mod chrome;
mod console;
mod history;
mod inspection;
mod privacy;
mod screenshot;
mod viewport;
use console::ConsoleLine;
#[cfg(target_os = "macos")]
mod chromium;

pub(super) fn web_url(value: &str) -> bool {
    url::Url::parse(value).is_ok_and(|url| {
        matches!(url.scheme(), "http" | "https")
            && url.host_str().is_some()
            && url.username().is_empty()
            && url.password().is_none()
    })
}

/// Only development previews are captured from ordinary clicked web links.
pub(super) fn local_preview_url(value: &str) -> bool {
    web_url(value)
        && url::Url::parse(value).is_ok_and(|url| match url.host() {
            Some(url::Host::Domain(host)) => {
                let host = host.trim_end_matches('.');
                host.eq_ignore_ascii_case("localhost")
                    || host.to_ascii_lowercase().ends_with(".localhost")
            }
            Some(url::Host::Ipv4(ip)) => ip.is_loopback(),
            Some(url::Host::Ipv6(ip)) => ip.is_loopback(),
            None => false,
        })
}

/// Interactive local HTML has an opaque data origin and no network/file access.
#[cfg(any(test, target_os = "macos"))]
pub(super) fn safe_html(source: &str) -> String {
    format!(
        "<!doctype html><meta http-equiv=\"Content-Security-Policy\" content=\"default-src 'none'; script-src 'unsafe-inline'; connect-src 'none'; style-src 'unsafe-inline'; img-src data:; font-src data:; base-uri 'none'; form-action 'none'; frame-src 'none'\"><meta charset=\"utf-8\">{source}"
    )
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
enum Device {
    #[default]
    Responsive,
    Desktop,
    Tablet,
    Mobile,
    Custom,
}
#[derive(Clone, Debug, PartialEq)]
struct Options {
    device: Device,
    width: u32,
    height: u32,
    rotate: bool,
    zoom: f32,
    console: bool,
    console_errors_only: bool,
    console_query: String,
    console_paused: Option<std::collections::VecDeque<console::ConsoleLine>>,
}
fn loading_indicator(ui: &mut egui::Ui) {
    let colors = theme::palette(ui.ctx());
    ui.label(theme::small(ui, "Loading…"));
    let (rect, _) = ui.allocate_exact_size(Vec2::splat(12.0), Sense::hover());
    theme::status::indicator(ui, rect.center(), "Working", colors.accent, 3.5);
}

impl Default for Options {
    fn default() -> Self {
        Self {
            device: Device::Responsive,
            width: 390,
            height: 844,
            rotate: false,
            zoom: 1.0,
            console: false,
            console_errors_only: false,
            console_query: String::new(),
            console_paused: None,
        }
    }
}
impl Options {
    fn dimensions(&self) -> Option<(u32, u32)> {
        let (w, h) = match self.device {
            Device::Responsive => return None,
            Device::Desktop => (1440, 900),
            Device::Tablet => (768, 1024),
            Device::Mobile => (390, 844),
            Device::Custom => viewport::bounded_dimensions(self.width, self.height),
        };
        Some(if self.rotate { (h, w) } else { (w, h) })
    }
    fn mobile(&self) -> bool {
        matches!(self.device, Device::Mobile | Device::Tablet)
    }
}

#[derive(Clone, Copy)]
struct PageEpoch(u64);
impl Default for PageEpoch {
    fn default() -> Self {
        static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);
        Self(NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed))
    }
}
#[derive(Clone, Default)]
struct PageState {
    document: PageEpoch,
    location: String,
    title: String,
    loading: bool,
    back: bool,
    forward: bool,
    error: Option<String>,
    console: std::collections::VecDeque<ConsoleLine>,
    inspection: inspection::State,
    screenshot: screenshot::State,
    preview_active: bool,
    preview_request: Option<(i32, u64)>,
}
impl PageState {
    /// Any child navigation invalidates frozen viewport/element provenance too.
    /// CEF frame IDs are not assumed interchangeable with CDP frame IDs.
    fn navigation_changed(&mut self) {
        self.preview_request = None;
        self.inspection.cancel(Some(
            "Page or frame changed. Inspect again for a new snapshot.",
        ));
        self.screenshot.invalidate();
        self.document = PageEpoch::default();
    }
}
#[derive(Clone, Copy)]
enum Action {
    Back,
    Forward,
    Reload,
    Stop,
    DevTools,
}

#[derive(Default)]
pub(super) struct BrowserView {
    #[cfg(target_os = "macos")]
    engine: chromium::Engine,
    options: BTreeMap<String, Options>,
    history: history::History,
    privacy: privacy::Privacy,
    active: String,
    last_location: String,
    viewport_editor: Option<viewport::Editor>,
    inspection: Option<inspection::Editor>,
    screenshot: Option<screenshot::Editor>,
    automation: automation::Manager,
    unavailable: bool,
    #[cfg(test)]
    test_pages: std::collections::BTreeMap<String, PageState>,
}
impl BrowserView {
    pub(super) fn set_snapshots(&mut self, snapshots: crate::inline_visual::PageSnapshots) {
        #[cfg(target_os = "macos")]
        self.engine.set_snapshots(snapshots);
        #[cfg(not(target_os = "macos"))]
        let _ = snapshots;
    }
    pub(super) fn focused(&self) -> bool {
        // App-level Escape handling runs before browser UI: an open dimension
        // editor owns cancellation even when the native page has lost focus.
        if self.viewport_editor.is_some()
            || self.inspection.is_some()
            || self.privacy.confirming()
            || self.screenshot.is_some()
        {
            return true;
        }
        #[cfg(target_os = "macos")]
        return self.engine.focused();
        #[cfg(not(target_os = "macos"))]
        false
    }
    pub(super) fn hide(&mut self) {
        // Dialog/approval surfaces temporarily hide native CEF each frame. Keep
        // the native editor (and its keyboard ownership), but cancel its data.
        #[cfg(target_os = "macos")]
        self.engine.hide();
    }
    pub(super) fn clear(&mut self) {
        self.automation
            .fail_all("Browser tabs were closed while automation was in progress.");
        #[cfg(target_os = "macos")]
        self.engine.close_all();
        self.options.clear();
        self.history.reset();
        self.privacy.reset();
        self.active.clear();
        self.last_location.clear();
        self.viewport_editor = None;
        self.inspection = None;
        self.screenshot = None;
        self.unavailable = false;
    }
    pub(super) fn close(&mut self, key: &str) {
        self.automation.fail_tab(
            key,
            "Browser tab was closed while automation was in progress.",
        );
        if self.screenshot.as_ref().is_some_and(|e| e.key == key) {
            self.cancel_screenshot();
        }
        if self
            .inspection
            .as_ref()
            .is_some_and(|editor| editor.key == key)
        {
            self.inspection = None;
        }
        #[cfg(target_os = "macos")]
        self.engine.close(key);
        if let Some(context) = self.privacy.close(key) {
            #[cfg(target_os = "macos")]
            self.engine.drop_context(context);
            self.history.remove_context(context);
        }
        self.options.remove(key);
        if self
            .viewport_editor
            .as_ref()
            .is_some_and(|editor| editor.key == key)
        {
            self.viewport_editor = None;
        }
        if self.active == key {
            self.active.clear();
            self.last_location.clear();
        }
    }
    pub(super) fn reload(&mut self) {
        self.action(Action::Reload);
    }
    pub(super) fn configure(
        &mut self,
        key: &str,
        device: &str,
        console: bool,
    ) -> Result<(), String> {
        // Explicit CLI/demo presentation seed; never derives dimensions from a page.
        let custom = device
            .strip_prefix("custom:")
            .map(|value| {
                let (width, height) = value.split_once('x').ok_or_else(|| {
                    "Custom viewport must use custom:WIDTHxHEIGHT (CSS pixels).".to_owned()
                })?;
                viewport::dimensions(width, height)
            })
            .transpose()?;
        let device = match device {
            _ if custom.is_some() => Device::Custom,
            "responsive" => Device::Responsive,
            "desktop" => Device::Desktop,
            "tablet" => Device::Tablet,
            "mobile" => Device::Mobile,
            _ => {
                return Err(
                    "Browser device must be responsive, desktop, tablet, mobile, or custom:WIDTHxHEIGHT.".into(),
                );
            }
        };
        let options = self.options.entry(key.into()).or_default();
        options.device = device;
        if let Some((width, height)) = custom {
            options.width = width;
            options.height = height;
            options.rotate = false;
        }
        options.console = console;
        Ok(())
    }
    fn state(&self, key: &str) -> PageState {
        #[cfg(test)]
        if let Some(state) = self.test_pages.get(key) {
            return state.clone();
        }
        #[cfg(target_os = "macos")]
        {
            self.engine.state(key)
        }
        #[cfg(not(target_os = "macos"))]
        {
            let _ = key;
            PageState::default()
        }
    }
    fn action(&mut self, action: Action) {
        if self.privacy.suspended(&self.active) {
            if matches!(action, Action::Reload) {
                let key = self.active.clone();
                self.resume_session(&key);
            }
            return;
        }
        if self.privacy.confirming() {
            return;
        }
        #[cfg(target_os = "macos")]
        self.engine.action(&self.active, action);
        #[cfg(not(target_os = "macos"))]
        let _ = action;
    }
    pub(super) fn matches_active(&self, key: &str) -> bool {
        self.active == key
    }
    #[cfg(test)]
    pub(super) fn seed_capture_page(&mut self, key: &str) -> u64 {
        self.active = key.into();
        let state = PageState {
            location: key.into(),
            ..Default::default()
        };
        let epoch = state.document.0;
        self.test_pages.insert(key.into(), state);
        epoch
    }
    pub(super) fn capture_matches(&self, page: &str, epoch: u64) -> bool {
        let state = self.state(&self.active);
        !self.active.is_empty()
            && !state.loading
            && !self.session_suspended(&self.active)
            && !self.privacy_confirming()
            && state.document.0 == epoch
            && !state.location.is_empty()
            && state.location == page
    }
    pub(super) fn tab_title(&self, key: &str) -> String {
        let state = self.state(key);
        if !state.title.is_empty() {
            return state.title;
        }
        url::Url::parse(key)
            .ok()
            .and_then(|url| url.host_str().map(str::to_owned))
            .unwrap_or_else(|| {
                std::path::Path::new(key)
                    .file_name()
                    .map(|name| name.to_string_lossy().into_owned())
                    .unwrap_or_else(|| key.into())
            })
    }
    pub(super) fn location(&self, key: &str) -> Option<String> {
        let location = self.state(key).location;
        (!location.is_empty()).then_some(location)
    }
    pub(super) fn navigation(&mut self, ui: &mut egui::Ui, key: &str) {
        if self.active != key {
            self.cancel_screenshot();
            self.last_location.clear();
        }
        self.active = key.into();
        let state = self.state(key);
        if ui
            .add_enabled_ui(state.back, |ui| {
                chrome::icon_button(ui, Icon::ArrowLeft, "Back", false)
            })
            .inner
            .clicked()
        {
            self.action(Action::Back);
        }
        if ui
            .add_enabled_ui(state.forward, |ui| {
                chrome::icon_button(ui, Icon::ArrowRight, "Forward", false)
            })
            .inner
            .clicked()
        {
            self.action(Action::Forward);
        }
        if chrome::reload_button(ui, state.loading).clicked() {
            self.action(if state.loading {
                Action::Stop
            } else {
                Action::Reload
            });
        }
    }
    pub(super) fn tools(&mut self, ui: &mut egui::Ui, key: &str) {
        self.active = key.into();
        let inspecting = self
            .inspection
            .as_ref()
            .is_some_and(|editor| editor.key == key);
        if chrome::icon_button(ui, Icon::Inspect, "Inspect an element", inspecting).clicked() {
            self.cancel_screenshot();
            self.viewport_editor = None;
            #[cfg(target_os = "macos")]
            self.engine.cancel_inspection(key);
            #[cfg(target_os = "macos")]
            if !inspecting {
                self.engine.prepare_inspection(key);
            }
            self.inspection = if inspecting {
                None
            } else {
                Some(inspection::Editor::new(
                    key,
                    self.state(key).inspection.epoch,
                ))
            };
        }
        let more = chrome::icon_button(ui, Icon::Details, "More browser tools", false);
        egui::Popup::from_toggle_button_response(&more)
            .close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside)
            .frame(prompt_ui::popup(ui.ctx()))
            .width((ui.ctx().screen_rect().width() - 40.0).clamp(120.0, 270.0))
            .show(|ui| {
                egui::ScrollArea::vertical()
                    .id_salt(("browser-tools-scroll", key))
                    .max_height((ui.ctx().screen_rect().height() - 100.0).max(80.0))
                    .show(ui, |ui| {
                        theme::menu_controls(ui);
                        privacy::guard_keys(ui.ctx());
                        prompt_ui::compact_heading(ui, "Browser tools");
                        let shot = prompt_ui::button(
                            ui,
                            "Screenshot & annotate…",
                            prompt_ui::Button::Secondary,
                            !self.unavailable && !self.state(key).loading,
                        );
                        context_capture::browser::register(&shot);
                        if shot.clicked() {
                            if let Some(token) = context_capture::browser::freeze(ui.ctx()) {
                                if let Err(error) = self.begin_screenshot(ui.ctx(), key, token) {
                                    context_capture::browser::failed(ui.ctx(), token, error);
                                }
                            }
                            ui.close();
                        }
                        if prompt_ui::button(
                            ui,
                            "Chrome DevTools",
                            prompt_ui::Button::Secondary,
                            !self.unavailable,
                        )
                        .clicked()
                        {
                            self.action(Action::DevTools);
                            ui.close();
                        }
                        ui.separator();
                        self.history_menu(ui, key);
                        ui.separator();
                        self.privacy_menu(ui, key);
                    });
            });
    }
    pub(super) fn toolbar(&mut self, ui: &mut egui::Ui, key: &str, source: Option<&mut bool>) {
        let colors = theme::palette(ui.ctx());
        let width = ui.available_width() - if source.is_some() { 40.0 } else { 0.0 };
        let overflow = width < 210.0;
        let narrow = width < 360.0;
        let compact = width < 300.0
            || (width < 360.0
                && self
                    .options
                    .get(key)
                    .is_some_and(|options| options.device == Device::Custom));
        let available = cfg!(target_os = "macos") && !self.unavailable;
        let loading = self.state(key).loading;
        let automation_label = self.automation.active_label(key);
        if self
            .viewport_editor
            .as_ref()
            .is_some_and(|editor| editor.key != key)
        {
            self.viewport_editor = None;
        }
        let options = self.options.entry(key.into()).or_default();
        ui.horizontal_centered(|ui| {
            ui.spacing_mut().item_spacing.x = 3.0;
            ui.spacing_mut().interact_size.y = 28.0;
            ui.spacing_mut().button_padding.y = 3.0;
            if let Some(source) = source {
                chrome::preview_toggle(ui, source);
                if *source {
                    self.viewport_editor = None;
                    return;
                }
                if !overflow {
                    chrome::divider(ui);
                }
            }
            if !compact {
                egui::Frame::new()
                    .fill(colors.hover.gamma_multiply(0.45))
                    .corner_radius(7)
                    .inner_margin(0)
                    .show(ui, |ui| {
                        ui.horizontal_centered(|ui| {
                            ui.spacing_mut().item_spacing.x = 1.0;
                            for (device, icon, label) in [
                                (Device::Desktop, Icon::Desktop, "Desktop: 1440 × 900"),
                                (Device::Tablet, Icon::Tablet, "Tablet: 768 × 1024"),
                                (Device::Mobile, Icon::Phone, "Mobile: 390 × 844"),
                            ] {
                                if chrome::icon_button(ui, icon, label, options.device == device)
                                    .clicked()
                                {
                                    options.device = device;
                                }
                            }
                        });
                    });
                chrome::divider(ui);
            }
            let dimensions = options
                .dimensions()
                .map(|(w, h)| format!("{w} × {h}"))
                .unwrap_or_else(|| "Responsive".into());
            let size = if overflow {
                chrome::icon_button(ui, Icon::Sliders, "Preview settings", false)
            } else {
                chrome::label_button(ui, &dimensions, false)
                    .on_hover_text("Viewport size and device settings")
            };
            let popup = egui::Popup::from_toggle_button_response(&size)
                .close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside)
                .frame(prompt_ui::popup(ui.ctx()))
                .width((ui.ctx().screen_rect().width() - 40.0).clamp(120.0, 270.0))
                .show(|ui| {
                    viewport::settings(ui, key, options, &mut self.viewport_editor, available)
                });
            if popup.is_none()
                || !egui::Popup::is_id_open(ui.ctx(), egui::Popup::default_response_id(&size))
            {
                self.viewport_editor = None;
            }
            if overflow {
                return;
            }
            if !narrow
                && options.dimensions().is_some()
                && chrome::icon_button(ui, Icon::Rotate, "Rotate viewport", options.rotate)
                    .clicked()
            {
                options.rotate = !options.rotate;
            }
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                let console = options.console;
                let response = chrome::icon_button(
                    ui,
                    Icon::Terminal,
                    if console {
                        "Hide console"
                    } else {
                        "Show console"
                    },
                    console,
                );
                if response.clicked() {
                    options.console = !console;
                }
                let zoom =
                    chrome::label_button(ui, &format!("{}%", (options.zoom * 100.0) as u32), false)
                        .on_hover_text("Page zoom");
                egui::Popup::menu(&zoom).show(|ui| {
                    theme::menu_controls(ui);
                    for value in [0.5, 0.75, 1.0, 1.25, 1.5, 2.0] {
                        if ui
                            .selectable_value(
                                &mut options.zoom,
                                value,
                                format!("{}%", (value * 100.0) as u32),
                            )
                            .clicked()
                        {
                            ui.close();
                        }
                    }
                });
                // Loading and agent activity live in the toolbar: the native page
                // composites above egui, and reserving space below it would resize it.
                if let Some(label) = automation_label {
                    if !narrow {
                        chrome::agent_activity_indicator(ui, label);
                    }
                } else if loading && !narrow {
                    loading_indicator(ui);
                }
            });
        });
    }
    pub(super) fn viewport(&mut self, ui: &mut egui::Ui, key: &str) -> egui::Rect {
        let state = self.state(key);
        ui.ctx().data_mut(|d| {
            d.insert_temp(
                egui::Id::new("browser-current-page"),
                if state.location.is_empty() {
                    key.to_owned()
                } else {
                    state.location.clone()
                },
            )
        });
        self.viewport_with_state(ui, key, state)
    }

    fn viewport_with_state(
        &mut self,
        ui: &mut egui::Ui,
        key: &str,
        state: PageState,
    ) -> egui::Rect {
        if self
            .inspection
            .as_ref()
            .is_some_and(|editor| editor.key != key)
        {
            if let Some(editor) = self.inspection.take() {
                #[cfg(target_os = "macos")]
                self.engine.cancel_inspection(&editor.key);
                #[cfg(not(target_os = "macos"))]
                let _ = editor;
            }
        }
        self.privacy_dialog(ui.ctx(), key);
        if self.session_notice(ui, key) {
            return egui::Rect::from_min_size(ui.cursor().min, Vec2::ZERO);
        }
        if let Some(editor) = self.inspection.as_mut() {
            editor.page = if state.location.is_empty() {
                key.into()
            } else {
                state.location.clone()
            };
            editor.page_title = state.title.clone();
            editor.document = state.document.0;
            let colors = theme::palette(ui.ctx());
            let decision = egui::Frame::new()
                .fill(colors.surface)
                .inner_margin(12)
                .show(ui, |ui| {
                    egui::ScrollArea::vertical()
                        .id_salt(("element-inspection", key))
                        .max_height((ui.available_height() * 0.55).clamp(80.0, 320.0))
                        .show(ui, |ui| {
                            inspection::show(
                                ui,
                                editor,
                                &state.inspection,
                                cfg!(target_os = "macos")
                                    && state.inspection.ready
                                    && !self.unavailable
                                    && !state.loading,
                            )
                        })
                        .inner
                })
                .inner;
            if decision.close {
                self.inspection = None;
                #[cfg(target_os = "macos")]
                self.engine.cancel_inspection(key);
            } else if decision.cancel_pick {
                #[cfg(target_os = "macos")]
                self.engine.cancel_inspection(key);
            } else if decision.inventory {
                #[cfg(target_os = "macos")]
                self.engine.inspect_frames(key, state.inspection.epoch);
            } else if decision.pick {
                #[cfg(target_os = "macos")]
                self.engine
                    .pick_scoped(key, state.inspection.epoch, &decision.scope);
            } else if let Some(query) = decision.inspect {
                #[cfg(target_os = "macos")]
                self.engine
                    .inspect_scoped(key, state.inspection.epoch, &query, &decision.scope);
                #[cfg(not(target_os = "macos"))]
                let _ = query;
            }
        }
        let token = context_capture::browser::token(ui.ctx());
        if token.is_none() {
            self.cancel_screenshot();
        }
        if let Some(token) = token
            && let Some(image) = self.screenshot_ui(ui.ctx(), key, token, ui.is_enabled())
        {
            debug_assert_eq!(image.page, key);
            debug_assert_ne!(image.epoch, 0);
            debug_assert_ne!(image.request, 0);
            context_capture::browser::queue(
                ui.ctx(),
                context_capture::browser::Handoff::Image {
                    token: image.draft_token,
                    page: state.location.clone(),
                    epoch: state.document.0,
                    image: image.image,
                },
            );
        }
        if let Some(notice) = context_capture::browser::notice(ui.ctx()) {
            ui.add(egui::Label::new(theme::small(ui, notice)).wrap());
        }
        let options = self.options.entry(key.into()).or_default();
        let mut state = state;
        if let Some(lines) = &options.console_paused {
            state.console = lines.clone();
        }
        let mut toggle_pause = false;
        if options.console {
            let colors = theme::palette(ui.ctx());
            egui::TopBottomPanel::bottom(ui.make_persistent_id(("browser-console", key)))
                .resizable(true)
                .default_height(180.0)
                .height_range(90.0..=360.0)
                .frame(egui::Frame::new().fill(colors.code_bg).inner_margin(10))
                .show_inside(ui, |ui| {
                    ui.horizontal(|ui| {
                        ui.label(RichText::new("Console").size(12.0).strong());
                        let errors = state.console.iter().filter(|line| line.error).count();
                        if errors > 0 {
                            ui.label(
                                RichText::new(format!(
                                    "{errors} {}",
                                    if errors == 1 { "error" } else { "errors" }
                                ))
                                .size(11.0)
                                .color(colors.red),
                            );
                        }
                        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                            if chrome::icon_button(ui, Icon::Close, "Hide console", false).clicked()
                            {
                                options.console = false;
                            }
                            console::batch::show(ui, key, &state.console);
                            if ui
                                .add_enabled(
                                    options.console_paused.is_none(),
                                    egui::Button::new(theme::small(ui, "Clear")).frame(false),
                                )
                                .on_hover_text(
                                    "Clear the live retained buffer. Resume first when paused.",
                                )
                                .clicked()
                            {
                                #[cfg(target_os = "macos")]
                                self.engine.clear_console(key);
                            }
                        });
                    });
                    toggle_pause=console::pause::show(ui,key,options.console_paused.is_some());
                    if options.console_paused.is_some(){ui.add(egui::Label::new(theme::small(ui,"Paused snapshot · captures use these frozen records. The page keeps collecting; Clear is disabled until Resume.")).wrap());}
                    console::filter::show(ui, &mut options.console_errors_only, &state.console);
                    console::search::show(
                        ui,
                        key,
                        &mut options.console_query,
                        &state.console,
                        options.console_errors_only,
                    );
                    ui.add(egui::Label::new(theme::small(ui,
                        "Local only unless added to your draft · review for secrets before sending."
                    )).wrap());
                    ui.separator();
                    egui::ScrollArea::vertical()
                        .id_salt(("browser-console-lines", key))
                        .stick_to_bottom(true)
                        .show(ui, |ui| {
                            if state.console.is_empty() {
                                ui.label(theme::small(
                                    ui,
                                    "No messages yet. Open DevTools for the interactive console.",
                                ));
                            }
                            if options.console_errors_only
                                && !state.console.is_empty()
                                && !state.console.iter().any(|line| line.error)
                            {
                                ui.label(theme::small(
                                    ui,
                                    "No retained errors. Other messages remain in All.",
                                ));
                            }
                            if !options.console_query.is_empty()
                                && !state.console.iter().any(|line| {
                                    (!options.console_errors_only || line.error)
                                        && console::search::matches(line, &options.console_query)
                                })
                            {
                                ui.label(theme::small(
                                    ui,
                                    "No retained messages match this literal search.",
                                ));
                            }
                            for line in state.console.iter().filter(|line| {
                                (!options.console_errors_only || line.error)
                                    && console::search::matches(line, &options.console_query)
                            }) {
                                console::show(ui, key, line);
                            }
                        });
                });
        }
        if toggle_pause {
            console::pause::toggle(&mut options.console_paused, &state.console, ui.ctx());
        }
        // Native views composite above egui: reserve bottom status space without shifting the page top.
        let mut available = ui
            .available_rect_before_wrap()
            .intersect(ui.clip_rect())
            .shrink2(Vec2::new(NATIVE_EDGE_INSET, 0.0));
        if !available.is_positive() {
            return egui::Rect::NOTHING;
        }
        if state.error.is_some() {
            let status_h = 26.0_f32.min(available.height() * 0.4);
            let status_rect = egui::Rect::from_min_max(
                egui::pos2(available.left() + 8.0, available.bottom() - status_h),
                egui::pos2(
                    (available.right() - 8.0).max(available.left() + 8.0),
                    available.bottom() - 2.0,
                ),
            );
            available.max.y = (available.max.y - status_h - 2.0).max(available.min.y);
            let colors = theme::palette(ui.ctx());
            ui.scope_builder(egui::UiBuilder::new().max_rect(status_rect), |ui| {
                ui.with_layout(Layout::left_to_right(Align::Center), |ui| {
                    if let Some(error) = state.error {
                        let resp = egui::Frame::new()
                            .fill(colors.error_bg)
                            .stroke(egui::Stroke::new(1.0_f32, colors.red))
                            .corner_radius(6)
                            .inner_margin(egui::Margin::symmetric(8, 3))
                            .show(ui, |ui| {
                                ui.label(RichText::new(error).size(11.0).color(colors.red));
                            })
                            .response;
                        resp.on_hover_text(
                            "Check the URL and connection, then Reload. Certificate checks remain enabled.",
                        );
                    }
                });
            });
        }
        if !available.is_positive() {
            return egui::Rect::NOTHING;
        }
        let Some((width, height)) = options.dimensions() else {
            return available;
        };
        let colors = theme::palette(ui.ctx());
        ui.painter().rect_filled(available, 0, colors.sidebar);
        let inset = 14.0_f32
            .min(available.width() * 0.25)
            .min(available.height() * 0.25);
        let available = available.shrink(inset);
        let scale = (available.width() / width as f32)
            .min(available.height() / height as f32)
            .min(1.0);
        let size = egui::vec2(width as f32 * scale, height as f32 * scale);
        // Fixed CSS viewports always fit; a scroll container can reserve gutters
        // and clip one axis, distorting the native emulation's aspect ratio.
        let rect = egui::Rect::from_center_size(available.center(), size);
        ui.painter().rect_stroke(
            rect.expand(1.0),
            2,
            Stroke::new(1.0, colors.border),
            egui::StrokeKind::Outside,
        );
        rect.intersect(available).intersect(ui.clip_rect())
    }
    #[allow(clippy::too_many_arguments)]
    pub(super) fn show(
        &mut self,
        frame: &eframe::Frame,
        rect: egui::Rect,
        key: &str,
        url: Option<&str>,
        html: Option<&str>,
        holes: &[egui::Rect],
        ctx: &egui::Context,
    ) -> Result<(), String> {
        #[cfg(all(debug_assertions, target_os = "macos"))]
        if self.privacy_smoke_tick(frame, rect, key, ctx) {
            return Ok(());
        }
        if self.active != key {
            self.last_location.clear();
        }
        self.active = key.into();
        if self.privacy.suspended(key) || self.privacy.confirming() {
            self.hide();
            return Ok(());
        }
        let context = self.privacy.ensure(key);
        #[cfg(target_os = "macos")]
        let result = {
            self.engine.show(
                frame,
                rect,
                key,
                url,
                html,
                self.options.entry(key.into()).or_default(),
                context,
                &self.history,
                holes,
                ctx,
            )
        };
        #[cfg(not(target_os = "macos"))]
        let result = {
            let _ = (frame, rect, url, html, holes, ctx, context);
            Err("Bundled Chromium is currently packaged for macOS only. File/Markdown/image viewing and HTML Source remain available.".into())
        };
        self.unavailable = result.is_err();
        result
    }
    pub(super) fn take_location(&mut self) -> Option<String> {
        let location = self.state(&self.active).location;
        if location.is_empty() || self.last_location == location {
            return None;
        }
        self.last_location = location.clone();
        Some(location)
    }
    pub(super) fn page_info(&self, key: &str) -> (String, String, bool) {
        let state = self.state(key);
        let url = if state.location.is_empty() {
            key.to_owned()
        } else {
            state.location
        };
        #[cfg(target_os = "macos")]
        let loading = if self.engine.has_tab(key) {
            state.loading
        } else {
            true
        };
        #[cfg(not(target_os = "macos"))]
        let loading = state.loading;
        (url, state.title, loading)
    }
    pub(super) fn navigate_existing(&mut self, key: &str, url: &str) {
        #[cfg(target_os = "macos")]
        {
            if self.engine.has_tab(key)
                && self.active == key
                && !self.privacy.suspended(key)
                && !self.privacy.confirming()
            {
                let current = self.state(key).location;
                if current != url {
                    self.engine.navigate(key, url);
                }
            }
        }
        #[cfg(not(target_os = "macos"))]
        let _ = (key, url);
    }
    fn check_automation_ready(&self, key: &str) -> Result<(), String> {
        if self.privacy.suspended(key) || self.privacy.confirming() {
            return Err(
                "This browser tab is paused for a privacy or group change. Wait for the user to resume it."
                    .into(),
            );
        }
        #[cfg(not(target_os = "macos"))]
        {
            let _ = key;
            Err(automation::UNAVAILABLE_NON_MACOS.into())
        }
        #[cfg(target_os = "macos")]
        {
            if self.unavailable {
                return Err(
                    "Built-in Chromium is unavailable. Browser automation requires the bundled macOS Chromium runtime."
                        .into(),
                );
            }
            if !self.engine.has_tab(key) {
                return Err(
                    "No active Chromium browser tab is open. Call pui_browser_navigate first."
                        .into(),
                );
            }
            Ok(())
        }
    }
    pub(super) fn start_automation_read(
        &mut self,
        source: &str,
        rpc_id: &str,
        key: &str,
        params: automation::ReadPageParams,
    ) -> Result<(), String> {
        self.check_automation_ready(key)?;
        let mailbox = self.automation.push_cdp(
            source.to_owned(),
            rpc_id.to_owned(),
            key.to_owned(),
            "Agent inspecting…",
            automation::REQUEST_TIMEOUT,
        )?;
        #[cfg(target_os = "macos")]
        {
            let script = automation::build_read_page_script(&params);
            if let Err(err) = self.engine.automation_eval(key, script, mailbox.clone()) {
                mailbox.complete(Err(err));
            }
        }
        #[cfg(not(target_os = "macos"))]
        {
            let _ = (params, mailbox);
        }
        Ok(())
    }
    pub(super) fn start_automation_act(
        &mut self,
        source: &str,
        rpc_id: &str,
        key: &str,
        params: automation::ActParams,
    ) -> Result<(), String> {
        if let automation::ActParams::Wait { ms } = params {
            #[cfg(not(target_os = "macos"))]
            {
                let _ = (source, rpc_id, key, ms);
                return Err(automation::UNAVAILABLE_NON_MACOS.into());
            }
            #[cfg(target_os = "macos")]
            {
                return self.automation.push_wait(
                    source.to_owned(),
                    rpc_id.to_owned(),
                    key.to_owned(),
                    ms,
                );
            }
        }
        self.check_automation_ready(key)?;
        let mailbox = self.automation.push_cdp(
            source.to_owned(),
            rpc_id.to_owned(),
            key.to_owned(),
            "Agent acting…",
            automation::REQUEST_TIMEOUT,
        )?;
        #[cfg(target_os = "macos")]
        {
            let res = match params {
                automation::ActParams::Click { selector } => {
                    let script = automation::build_click_script(&selector);
                    self.engine.automation_eval(key, script, mailbox.clone())
                }
                automation::ActParams::Type {
                    selector,
                    text,
                    clear,
                } => {
                    let script = automation::build_type_script(&selector, &text, clear);
                    self.engine.automation_eval(key, script, mailbox.clone())
                }
                automation::ActParams::Press { key: spec } => {
                    self.engine.automation_press(key, spec, mailbox.clone())
                }
                automation::ActParams::Scroll { selector, delta_y } => {
                    let script = automation::build_scroll_script(selector.as_deref(), delta_y);
                    self.engine.automation_eval(key, script, mailbox.clone())
                }
                automation::ActParams::Wait { .. } => unreachable!(),
            };
            if let Err(err) = res {
                mailbox.complete(Err(err));
            }
        }
        #[cfg(not(target_os = "macos"))]
        {
            let _ = (params, mailbox);
        }
        Ok(())
    }
    pub(super) fn start_automation_screenshot(
        &mut self,
        source: &str,
        rpc_id: &str,
        key: &str,
        params: automation::ScreenshotParams,
    ) -> Result<(), String> {
        self.check_automation_ready(key)?;
        let mailbox = self.automation.push_cdp(
            source.to_owned(),
            rpc_id.to_owned(),
            key.to_owned(),
            "Agent inspecting…",
            automation::REQUEST_TIMEOUT,
        )?;
        #[cfg(target_os = "macos")]
        {
            if let Err(err) =
                self.engine
                    .automation_screenshot(key, params.full_page, mailbox.clone())
            {
                mailbox.complete(Err(err));
            }
        }
        #[cfg(not(target_os = "macos"))]
        {
            let _ = (params, mailbox);
        }
        Ok(())
    }
    pub(super) fn automation_console(
        &self,
        key: &str,
        params: &automation::ConsoleParams,
    ) -> Value {
        let state = self.state(key);
        let url = if state.location.is_empty() {
            key
        } else {
            state.location.as_str()
        };
        automation::filter_console(&state.console, url, params)
    }
    pub(super) fn poll_automation(&mut self, ctx: &egui::Context) -> Vec<automation::Completion> {
        if self.automation.is_empty() {
            return Vec::new();
        }
        // Snapshot tab info without borrowing `self` mutably during `poll`.
        let mut mgr = std::mem::take(&mut self.automation);
        let ready = mgr.poll(ctx, |key| self.page_info(key));
        self.automation = mgr;
        ready
    }
    pub(super) fn automation_pending(&self) -> bool {
        !self.automation.is_empty()
    }
    pub(super) fn cancel_automation_for_source(&mut self, source: &str) {
        self.automation.fail_source(
            source,
            "Agent stopped while browser automation was in progress.",
        );
    }
}

#[cfg(test)]
mod layout_tests;

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn only_http_pages_without_embedded_credentials() {
        for value in [
            "https://example.com/a",
            "http://localhost:3000",
            "http://127.0.0.1:8000",
        ] {
            assert!(web_url(value));
        }
        for value in [
            "javascript:alert(1)",
            "file:///etc/passwd",
            "data:text/html,hello",
            "https://user:pass@example.com",
            "https://",
            "mailto:a@b.com",
        ] {
            assert!(!web_url(value));
        }
    }
    #[test]
    fn clicked_preview_links_are_strictly_local() {
        for value in [
            "http://localhost:3000",
            "https://LOCALHOST.:3000/a",
            "http://app.localhost:8080",
            "http://127.0.0.1:8000",
            "http://127.1.2.3:8000",
            "http://[::1]:8080",
        ] {
            assert!(local_preview_url(value), "{value}");
        }
        for value in [
            "https://github.com/a/b/pull/40",
            "https://localhost.example.com",
            "https://notlocalhost",
            "https://127.0.0.1.example.com",
            "https://localhost@example.com",
            "https://user:pass@localhost:3000",
            "http://192.168.1.2:3000",
            "file:///tmp/preview.html",
            "javascript:alert(1)",
        ] {
            assert!(!local_preview_url(value), "{value}");
        }
    }
    #[test]
    fn local_preview_policy_precedes_untrusted_markup() {
        let html = safe_html("<script>alert(1)</script><img src='file:///etc/passwd'>");
        assert!(html.find("Content-Security-Policy").unwrap() < html.find("<script>").unwrap());
        assert!(html.contains("script-src 'unsafe-inline'"));
        assert!(html.contains("connect-src 'none'"));
    }
    #[test]
    fn device_toolbar_stays_one_row_and_controls_work_at_narrow_widths() {
        for theme in [theme::Theme::Light, theme::Theme::Dark] {
            for width in [340.0, 420.0, 720.0] {
                let ctx = egui::Context::default();
                theme::install(&ctx);
                theme.apply(&ctx);
                let mut browser = BrowserView::default();
                let key = "https://example.com";
                browser.configure(key, "mobile", false).unwrap();
                let mut row = egui::Rect::NOTHING;
                let draw =
                    |ctx: &egui::Context, browser: &mut BrowserView, row: &mut egui::Rect| {
                        *row = egui::TopBottomPanel::top("test-browser-toolbar")
                            .exact_height(42.0)
                            .show(ctx, |ui| browser.toolbar(ui, key, None))
                            .response
                            .rect;
                    };
                for _ in 0..2 {
                    let _ = ctx.run(
                        egui::RawInput {
                            screen_rect: Some(egui::Rect::from_min_size(
                                egui::Pos2::ZERO,
                                Vec2::new(width, 180.0),
                            )),
                            ..Default::default()
                        },
                        |ctx| draw(ctx, &mut browser, &mut row),
                    );
                }
                assert!(row.height() < 45.0, "toolbar wrapped at {width}: {row:?}");
                let phone = ctx
                    .data(|data| {
                        data.get_temp::<egui::Rect>(egui::Id::new((
                            "browser-control",
                            "Mobile: 390 × 844",
                        )))
                    })
                    .unwrap();
                let console = ctx
                    .data(|data| {
                        data.get_temp::<egui::Rect>(egui::Id::new((
                            "browser-control",
                            "Show console",
                        )))
                    })
                    .unwrap();
                assert!(
                    (phone.center().y - console.center().y).abs() < 1.0,
                    "controls are not aligned at {width}: phone={phone:?} console={console:?}"
                );
                assert!(
                    phone.right() < console.left(),
                    "overlapping controls at {width}"
                );
                assert!(
                    console.right() <= width,
                    "console outside toolbar at {width}: {console:?}"
                );
                for position in [phone.center(), console.center()] {
                    for pressed in [true, false] {
                        let _ = ctx.run(
                            egui::RawInput {
                                screen_rect: Some(egui::Rect::from_min_size(
                                    egui::Pos2::ZERO,
                                    Vec2::new(width, 180.0),
                                )),
                                events: vec![
                                    egui::Event::PointerMoved(position),
                                    egui::Event::PointerButton {
                                        pos: position,
                                        button: egui::PointerButton::Primary,
                                        pressed,
                                        modifiers: egui::Modifiers::NONE,
                                    },
                                ],
                                ..Default::default()
                            },
                            |ctx| draw(ctx, &mut browser, &mut row),
                        );
                    }
                }
                assert_eq!(browser.options[key].device, Device::Mobile);
                assert!(browser.options[key].console);
            }
        }
    }

    #[test]
    fn device_profiles_rotate_and_bound_custom_dimensions() {
        let mut options = Options::default();
        assert_eq!(options.dimensions(), None);
        options.device = Device::Mobile;
        assert_eq!(options.dimensions(), Some((390, 844)));
        assert!(options.mobile());
        options.rotate = true;
        assert_eq!(options.dimensions(), Some((844, 390)));
        options.device = Device::Custom;
        options.width = u32::MAX;
        options.height = 0;
        assert_eq!(options.dimensions(), Some((240, 3840)));
        assert!(!options.mobile());
    }
}
