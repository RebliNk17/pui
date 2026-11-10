//! Tab strip and a single navigation/address row for the docked viewer.
use super::super::webview::chrome;
use super::*;

impl Browser {
    pub(super) fn chrome(&mut self, ui: &mut egui::Ui) {
        let colors = theme::palette(ui.ctx());
        let ctx = ui.ctx().clone();
        let mut select = None;
        let mut close_file = None;
        let mut select_url = None;
        let mut close_url = None;
        let mut new_tab = false;
        egui::TopBottomPanel::top("viewer-tab-strip")
            .exact_height(44.0)
            .frame(
                egui::Frame::new()
                    .fill(colors.sidebar)
                    .inner_margin(egui::Margin::symmetric(10, 6)),
            )
            .show_inside(ui, |ui| {
                ui.horizontal_centered(|ui| {
                    ui.spacing_mut().item_spacing.x = 6.0;
                    if self.tabs.is_empty() && self.urls.is_empty() {
                        ui.label(RichText::new("Files").size(12.0).color(colors.muted));
                    } else {
                        let tabs_width = (ui.available_width() - 96.0).max(90.0);
                        egui::ScrollArea::horizontal()
                            .id_salt("viewer-tabs-scroll")
                            .max_width(tabs_width)
                            .show(ui, |ui| {
                                ui.horizontal(|ui| {
                                    ui.spacing_mut().item_spacing.x = 4.0;
                                    let count = self.tabs.len() + self.urls.len();
                                    let width = ((tabs_width - 36.0) / count.max(1) as f32)
                                        .clamp(120.0, 200.0);
                                    for (index, file) in self.tabs.iter().enumerate() {
                                        let title = file
                                            .path
                                            .file_name()
                                            .unwrap_or_default()
                                            .to_string_lossy();
                                        let (selected, closed) = chrome::tab(
                                            ui,
                                            ("file", &file.path),
                                            &title,
                                            &file.path.display().to_string(),
                                            Icon::File,
                                            self.selected.as_ref() == Some(&file.path),
                                            width,
                                        );
                                        if selected {
                                            select = Some(file.path.clone());
                                        }
                                        if closed {
                                            close_file = Some(index);
                                        }
                                    }
                                    for (index, url) in self.urls.iter().enumerate() {
                                        let title = self.web.tab_title(url);
                                        let (selected, closed) = chrome::tab(
                                            ui,
                                            ("page", url),
                                            &title,
                                            url,
                                            Icon::Globe,
                                            self.selected_url.as_ref() == Some(url),
                                            width,
                                        );
                                        if selected {
                                            select_url = Some(url.clone());
                                        }
                                        if closed {
                                            close_url = Some(index);
                                        }
                                    }
                                    new_tab = chrome::icon_button(
                                        ui,
                                        Icon::Plus,
                                        "Open another file or page",
                                        false,
                                    )
                                    .clicked();
                                });
                            });
                    }
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        if chrome::icon_button(ui, Icon::Close, "Close viewer", false).clicked() {
                            self.close();
                        }
                        if chrome::icon_button(
                            ui,
                            if self.expanded {
                                Icon::Collapse
                            } else {
                                Icon::Expand
                            },
                            if self.expanded {
                                "Restore viewer panel"
                            } else {
                                "Expand viewer"
                            },
                            false,
                        )
                        .clicked()
                        {
                            self.expanded = !self.expanded;
                        }
                    });
                });
            });
        if let Some(path) = select {
            self.address = path.display().to_string();
            self.selected = Some(path);
            self.selected_url = None;
        }
        if let Some(url) = select_url {
            self.address = self.web.location(&url).unwrap_or_else(|| url.clone());
            self.selected_url = Some(url);
            self.selected = None;
        }
        if let Some(index) = close_url {
            let url = self.urls.remove(index);
            self.web.close(&url);
            if self.selected_url.as_ref() == Some(&url) {
                self.selected_url = self.urls.last().cloned();
                if self.selected_url.is_none() {
                    self.selected = self.tabs.last().map(|file| file.path.clone());
                }
                self.address = self
                    .selected_url
                    .clone()
                    .or_else(|| {
                        self.selected
                            .as_ref()
                            .map(|path| path.display().to_string())
                    })
                    .unwrap_or_default();
            }
        }
        if let Some(index) = close_file {
            self.close_tab(index);
        }
        if new_tab {
            self.address.clear();
        }
        let page = self.selected_url.clone();
        let mut open = false;
        egui::TopBottomPanel::top("viewer-navigation")
            .exact_height(48.0)
            .frame(
                egui::Frame::new()
                    .fill(colors.bg)
                    .inner_margin(egui::Margin::symmetric(10, 7)),
            )
            .show_inside(ui, |ui| {
                ui.horizontal_centered(|ui| {
                    ui.spacing_mut().item_spacing.x = 4.0;
                    if let Some(key) = &page {
                        self.web.navigation(ui, key);
                    } else if chrome::icon_button(
                        ui,
                        Icon::Refresh,
                        "Refresh files and selected preview",
                        false,
                    )
                    .clicked()
                    {
                        self.refresh(&ctx);
                    }
                    let trailing = if page.is_some() { 66.0 } else { 34.0 };
                    ui.allocate_ui_with_layout(
                        Vec2::new((ui.available_width() - trailing).max(50.0), 32.0),
                        Layout::left_to_right(Align::Center),
                        |ui| {
                            open = chrome::address(ui, &mut self.address, page.is_some(), new_tab);
                        },
                    );
                    if let Some(key) = &page {
                        self.web.tools(ui, key);
                    } else {
                        open |=
                            chrome::icon_button(ui, Icon::ArrowRight, "Open file or page", false)
                                .clicked();
                    }
                });
            });
        if open {
            let target = self.address.trim().to_owned();
            let root = self.root.clone();
            if let Err(error) = self.open_target(&ctx, &root, &target) {
                self.address_error = Some(error);
            }
        }
        if let Some(error) = &self.address_error {
            egui::TopBottomPanel::top("viewer-address-error")
                .frame(egui::Frame::new().fill(colors.error_bg).inner_margin(10))
                .show_inside(ui, |ui| {
                    ui.add(
                        egui::Label::new(RichText::new(error).size(12.0).color(colors.red)).wrap(),
                    );
                });
        }
        if let Some(key) = self.selected_url.clone() {
            egui::TopBottomPanel::top("browser-device-toolbar")
                .exact_height(42.0)
                .frame(
                    egui::Frame::new()
                        .fill(colors.sidebar)
                        .inner_margin(egui::Margin::symmetric(10, 6)),
                )
                .show_inside(ui, |ui| self.web.toolbar(ui, &key));
        }
    }
}
