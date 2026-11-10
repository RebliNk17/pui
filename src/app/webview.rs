//! Chromium browser UI; files and the agent never get a privileged page bridge.
use super::*;
use std::collections::BTreeMap;

pub(super) mod chrome;
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

/// Local HTML gets no file origin, scripts, frames, forms, or network resources.
#[cfg(any(test, target_os = "macos"))]
pub(super) fn safe_html(source: &str) -> String {
    format!(
        "<!doctype html><meta http-equiv=\"Content-Security-Policy\" content=\"default-src 'none'; script-src 'none'; style-src 'unsafe-inline'; img-src data:; font-src data:; base-uri 'none'; form-action 'none'; frame-src 'none'\"><meta charset=\"utf-8\">{source}"
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
    fit: bool,
    zoom: f32,
    console: bool,
}
impl Default for Options {
    fn default() -> Self {
        Self {
            device: Device::Responsive,
            width: 390,
            height: 844,
            rotate: false,
            fit: true,
            zoom: 1.0,
            console: false,
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
            Device::Custom => (self.width.clamp(240, 3840), self.height.clamp(240, 3840)),
        };
        Some(if self.rotate { (h, w) } else { (w, h) })
    }
    fn mobile(&self) -> bool {
        matches!(self.device, Device::Mobile | Device::Tablet)
    }
}

#[derive(Clone, Default)]
struct PageState {
    location: String,
    title: String,
    loading: bool,
    back: bool,
    forward: bool,
    error: Option<String>,
    console: std::collections::VecDeque<ConsoleLine>,
}
#[derive(Clone)]
struct ConsoleLine {
    error: bool,
    text: String,
}
#[derive(Clone, Copy)]
enum Action {
    Back,
    Forward,
    Reload,
    Stop,
    DevTools,
    Inspect,
}

#[derive(Default)]
pub(super) struct BrowserView {
    #[cfg(target_os = "macos")]
    engine: chromium::Engine,
    options: BTreeMap<String, Options>,
    active: String,
    last_location: String,
}
impl BrowserView {
    pub(super) fn hide(&mut self) {
        #[cfg(target_os = "macos")]
        self.engine.hide();
    }
    pub(super) fn clear(&mut self) {
        #[cfg(target_os = "macos")]
        self.engine.close_all();
        self.options.clear();
        self.active.clear();
        self.last_location.clear();
    }
    pub(super) fn close(&mut self, key: &str) {
        #[cfg(target_os = "macos")]
        self.engine.close(key);
        self.options.remove(key);
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
        let device = match device {
            "responsive" => Device::Responsive,
            "desktop" => Device::Desktop,
            "tablet" => Device::Tablet,
            "mobile" => Device::Mobile,
            _ => {
                return Err(
                    "Browser device must be responsive, desktop, tablet, or mobile.".into(),
                );
            }
        };
        let options = self.options.entry(key.into()).or_default();
        options.device = device;
        options.console = console;
        Ok(())
    }
    fn state(&self, key: &str) -> PageState {
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
        #[cfg(target_os = "macos")]
        self.engine.action(&self.active, action);
        #[cfg(not(target_os = "macos"))]
        let _ = action;
    }
    pub(super) fn tab_title(&self, key: &str) -> String {
        let state = self.state(key);
        if !state.title.is_empty() {
            return state.title;
        }
        url::Url::parse(key)
            .ok()
            .and_then(|url| url.host_str().map(str::to_owned))
            .unwrap_or_else(|| key.into())
    }
    pub(super) fn location(&self, key: &str) -> Option<String> {
        let location = self.state(key).location;
        (!location.is_empty()).then_some(location)
    }
    pub(super) fn navigation(&mut self, ui: &mut egui::Ui, key: &str) {
        if self.active != key {
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
        if chrome::icon_button(
            ui,
            if state.loading {
                Icon::Stop
            } else {
                Icon::Refresh
            },
            if state.loading {
                "Stop loading"
            } else {
                "Reload page"
            },
            false,
        )
        .clicked()
        {
            self.action(if state.loading {
                Action::Stop
            } else {
                Action::Reload
            });
        }
    }
    pub(super) fn tools(&mut self, ui: &mut egui::Ui, key: &str) {
        self.active = key.into();
        if chrome::icon_button(ui, Icon::Inspect, "Inspect an element", false).clicked() {
            self.action(Action::Inspect);
        }
        if chrome::icon_button(ui, Icon::Code, "Open Chrome DevTools", false).clicked() {
            self.action(Action::DevTools);
        }
    }
    pub(super) fn toolbar(&mut self, ui: &mut egui::Ui, key: &str) {
        let colors = theme::palette(ui.ctx());
        let narrow = ui.available_width() < 440.0;
        let options = self.options.entry(key.into()).or_default();
        ui.horizontal_centered(|ui| {
            ui.spacing_mut().item_spacing.x = 3.0;
            egui::Frame::new().fill(colors.hover.gamma_multiply(0.45)).corner_radius(7).inner_margin(0).show(ui, |ui| {
                ui.horizontal_centered(|ui| {
                    ui.spacing_mut().item_spacing.x = 1.0;
                    for (device, icon, label) in [(Device::Responsive, Icon::Expand, "Responsive: fill the pane"), (Device::Desktop, Icon::Desktop, "Desktop: 1440 × 900"), (Device::Tablet, Icon::Tablet, "Tablet: 768 × 1024"), (Device::Mobile, Icon::Phone, "Mobile: 390 × 844")] {
                        if chrome::icon_button(ui, icon, label, options.device == device).clicked() { options.device = device; }
                    }
                });
            });
            chrome::divider(ui);
            let dimensions = options.dimensions().map(|(w, h)| format!("{w} × {h}")).unwrap_or_else(|| "Auto".into());
            let size = chrome::label_button(ui, &dimensions, false).on_hover_text("Viewport size and device settings");
            egui::Popup::menu(&size).show(|ui| {
                theme::menu_controls(ui);
                ui.set_min_width(210.0);
                ui.label(RichText::new("Viewport").size(12.0).strong());
                ui.add_space(6.0);
                for (device, label) in [(Device::Responsive, "Responsive · fill pane"), (Device::Desktop, "Desktop · 1440 × 900"), (Device::Tablet, "Tablet · 768 × 1024"), (Device::Mobile, "Mobile · 390 × 844"), (Device::Custom, "Custom dimensions")] {
                    if ui.selectable_value(&mut options.device, device, label).clicked() && device != Device::Custom { ui.close(); }
                }
                if options.device == Device::Custom {
                    ui.horizontal(|ui| {
                        ui.add(egui::DragValue::new(&mut options.width).range(240..=3840).suffix(" px"));
                        ui.label("×");
                        ui.add(egui::DragValue::new(&mut options.height).range(240..=3840).suffix(" px"));
                    });
                }
                ui.separator();
                ui.checkbox(&mut options.fit, "Fit viewport to panel");
                ui.checkbox(&mut options.rotate, "Landscape orientation");
                if options.mobile() { ui.add(egui::Label::new(theme::small(ui, "Touch + Android user agent. Not a real device. Reload for server-side detection.")).wrap()); }
            });
            if !narrow {
                if ui.add_enabled_ui(options.dimensions().is_some(), |ui| chrome::icon_button(ui, Icon::Rotate, "Rotate viewport", false)).inner.clicked() { options.rotate = !options.rotate; }
                if chrome::label_button(ui, "Fit", options.fit).on_hover_text("Fit viewport to panel").clicked() { options.fit = !options.fit; }
            }
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                let console = options.console;
                let response = chrome::icon_button(ui, Icon::Terminal, if console { "Hide console" } else { "Show console" }, console);
                if response.clicked() { options.console = !console; }
                let zoom = chrome::label_button(ui, &format!("{}%", (options.zoom * 100.0) as u32), false).on_hover_text("Page zoom");
                egui::Popup::menu(&zoom).show(|ui| {
                    theme::menu_controls(ui);
                    for value in [0.5, 0.75, 1.0, 1.25, 1.5, 2.0] {
                        if ui.selectable_value(&mut options.zoom, value, format!("{}%", (value * 100.0) as u32)).clicked() { ui.close(); }
                    }
                });
            });
        });
    }
    pub(super) fn viewport(&mut self, ui: &mut egui::Ui, key: &str) -> egui::Rect {
        let state = self.state(key);
        let options = self.options.entry(key.into()).or_default();
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
                            if chrome::label_button(ui, "Clear", false).clicked() {
                                #[cfg(target_os = "macos")]
                                self.engine.clear_console(key);
                            }
                        });
                    });
                    ui.separator();
                    egui::ScrollArea::vertical()
                        .stick_to_bottom(true)
                        .show(ui, |ui| {
                            if state.console.is_empty() {
                                ui.label(theme::small(
                                    ui,
                                    "No messages yet. Open DevTools for the interactive console.",
                                ));
                            }
                            for line in state.console {
                                ui.add(
                                    egui::Label::new(
                                        RichText::new(line.text).monospace().size(11.0).color(
                                            if line.error { colors.red } else { colors.muted },
                                        ),
                                    )
                                    .wrap()
                                    .selectable(true),
                                );
                            }
                        });
                });
        }
        let available = ui.available_rect_before_wrap();
        if let Some(error) = state.error {
            ui.colored_label(theme::palette(ui.ctx()).red, error);
        }
        let Some((width, height)) = options.dimensions() else {
            return available;
        };
        let colors = theme::palette(ui.ctx());
        ui.painter().rect_filled(available, 0, colors.sidebar);
        let available = available.shrink(14.0);
        let scale = if options.fit {
            (available.width() / width as f32)
                .min(available.height() / height as f32)
                .clamp(0.05, 1.0)
        } else {
            1.0
        };
        let size = egui::vec2(width as f32 * scale, height as f32 * scale);
        let mut rect = available;
        egui::ScrollArea::both()
            .id_salt(("device-viewport", key))
            .auto_shrink([false, false])
            .show(ui, |ui| {
                let left = ((ui.available_width() - size.x) / 2.0).max(0.0);
                ui.horizontal(|ui| {
                    ui.add_space(left);
                    rect = ui.allocate_exact_size(size, Sense::hover()).0;
                });
            });
        ui.painter().rect_stroke(
            rect.expand(1.0),
            2,
            Stroke::new(1.0, colors.border),
            egui::StrokeKind::Outside,
        );
        rect.intersect(available)
    }
    pub(super) fn show(
        &mut self,
        frame: &eframe::Frame,
        rect: egui::Rect,
        key: &str,
        url: Option<&str>,
        html: Option<&str>,
        ctx: &egui::Context,
    ) -> Result<(), String> {
        if self.active != key {
            self.last_location.clear();
        }
        self.active = key.into();
        #[cfg(target_os = "macos")]
        {
            self.engine.show(
                frame,
                rect,
                key,
                url,
                html,
                self.options.entry(key.into()).or_default(),
                ctx,
            )
        }
        #[cfg(not(target_os = "macos"))]
        {
            let _ = (frame, rect, url, html, ctx);
            Err("Bundled Chromium is currently packaged for macOS only. File/Markdown/image viewing and HTML Source remain available.".into())
        }
    }
    pub(super) fn take_location(&mut self) -> Option<String> {
        let location = self.state(&self.active).location;
        if location.is_empty() || self.last_location == location {
            return None;
        }
        self.last_location = location.clone();
        Some(location)
    }
}

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
    fn local_preview_policy_precedes_untrusted_markup() {
        let html = safe_html("<script>alert(1)</script><img src='file:///etc/passwd'>");
        assert!(html.find("Content-Security-Policy").unwrap() < html.find("<script>").unwrap());
        assert!(html.contains("script-src 'none'"));
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
                            .show(ctx, |ui| browser.toolbar(ui, key))
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
