//! Full-window settings, independent of the conversation and its agent lifecycle.
use super::*;

#[cfg(test)]
#[path = "design_tests.rs"]
mod design_tests;

#[derive(Clone, Copy, Default, PartialEq, Eq, Debug, Hash)]
enum Page {
    #[default]
    General,
    Appearance,
    Notifications,
    Shortcuts,
    Permissions,
    Models,
    Providers,
    Resources,
    Remote,
}
impl Page {
    const ALL: [Self; 9] = [
        Self::General,
        Self::Appearance,
        Self::Notifications,
        Self::Shortcuts,
        Self::Remote,
        Self::Permissions,
        Self::Providers,
        Self::Models,
        Self::Resources,
    ];
    fn title(self) -> &'static str {
        match self {
            Self::General => "General",
            Self::Appearance => "Appearance",
            Self::Notifications => "Notifications",
            Self::Shortcuts => "Keyboard shortcuts",
            Self::Permissions => "Permissions",
            Self::Models => "Models",
            Self::Providers => "Providers",
            Self::Resources => "Resources",
            Self::Remote => "Remote & Mobile",
        }
    }
    fn subtitle(self) -> &'static str {
        match self {
            Self::General => "Messages, context and agent runtime defaults.",
            Self::Appearance => "Choose a theme, workspace layout and interface scale.",
            Self::Notifications => "Choose how agents let you know their work is finished.",
            Self::Shortcuts => "Shortcuts available across Pui.",
            Self::Permissions => "Choose what your agent can do without asking.",
            Self::Models => "Defaults for new chats and the models Pi provides.",
            Self::Providers => "Connected model providers, managed by Pi.",
            Self::Resources => "Manage skills, MCP servers, extensions and Pi packages.",
            Self::Remote => "Connect Pui Mobile on Android (and iOS) to this host over Tailscale.",
        }
    }
    fn keywords(self) -> &'static str {
        match self {
            Self::General => {
                "steer queue messages enter context compact tokens worker concurrency updates auto check download install"
            }
            Self::Appearance => {
                "layout sidebar inspector scale zoom reset dark light theme color styles midnight ivory forest sage plum rose classic neutral"
            }
            Self::Notifications => {
                "sound completion alerts desktop macos permission notification test"
            }
            Self::Shortcuts => "keyboard shortcuts keys command control enter escape",
            Self::Permissions => "manual auto accept edits plan dont ask classifier safety tools",
            Self::Models => "model reasoning effort thinking pricing context provider",
            Self::Providers => {
                "provider authentication login credentials catalog claude code subscription anthropic openai compatible endpoint local ollama lm studio vllm discover models"
            }
            Self::Resources => {
                "skills mcp servers extensions packages install remove prompts templates themes plugins"
            }
            Self::Remote => {
                "remote mobile android ios phone tailscale qr pairing token websocket server cloud"
            }
        }
    }
    fn icon(self) -> Icon {
        match self {
            Self::General => Icon::Sliders,
            Self::Appearance => Icon::Spark,
            Self::Notifications => Icon::Bell,
            Self::Shortcuts => Icon::Command,
            Self::Permissions => Icon::Shield,
            Self::Models => Icon::Code,
            Self::Providers => Icon::Branch,
            Self::Resources => Icon::Code,
            Self::Remote => Icon::Phone,
        }
    }
    fn matches(self, query: &str) -> bool {
        format!("{} {}", self.title(), self.keywords())
            .to_lowercase()
            .contains(query)
    }
}
pub(in crate::app) struct SearchEntry {
    pub title: &'static str,
    pub page: &'static str,
    pub keywords: &'static str,
}

/// Curated setting labels and aliases only; never search credentials/config values.
pub(in crate::app) fn search_entries() -> Vec<SearchEntry> {
    let mut entries: Vec<_> = Page::ALL
        .into_iter()
        .map(|page| SearchEntry {
            title: page.title(),
            page: page.title(),
            keywords: page.keywords(),
        })
        .collect();
    for (title, page, keywords) in [
        (
            "Messages while working",
            "General",
            "steer queue delivery enter",
        ),
        (
            "Model context window",
            "Models",
            "context window max tokens limit override vision reasoning fallback pricing edit model",
        ),
        (
            "Default model for new chats",
            "Models",
            "default model thinking effort new chat pi settings",
        ),
        (
            "Auto compact at",
            "General",
            "context summarizing tokens threshold percentage compaction",
        ),
        (
            "Keep recent context",
            "General",
            "tokens summarize retention",
        ),
        (
            "Worker concurrency ceiling",
            "General",
            "agents subagents parallel limit",
        ),
        (
            "Automatically check for updates",
            "General",
            "updates github release download install version",
        ),
        (
            "Pi version",
            "General",
            "pi runtime harness agent version bundled",
        ),
        (
            "Theme mode and colors",
            "Appearance",
            "dark light styles midnight ivory forest sage plum rose classic",
        ),
        ("Session sidebar", "Appearance", "layout show hide panel"),
        (
            "Inspector layout",
            "Appearance",
            "detail cards reset workspace",
        ),
        ("Interface scale", "Appearance", "zoom font text size"),
        (
            "Completion sound",
            "Notifications",
            "audio alerts finished deleted chat",
        ),
        (
            "OS notification",
            "Notifications",
            "desktop banner macos access permission",
        ),
        (
            "Permission mode",
            "Permissions",
            "manual auto accept edits plan dont ask bypass classifier safety approval tools",
        ),
        (
            "Bypass confirmation",
            "Permissions",
            "dont ask again reset remember bypass warning confirmation",
        ),
        (
            "Models and thinking levels",
            "Models",
            "reasoning effort pricing context",
        ),
        (
            "Provider authentication",
            "Providers",
            "login credentials connected model catalog claude code subscription anthropic",
        ),
        (
            "OpenAI-compatible endpoint & auto model discovery",
            "Providers",
            "local openai ollama lm studio vllm llamacpp models.json discover",
        ),
        (
            "Skills and extensions",
            "Resources",
            "install packages prompts templates plugins themes",
        ),
        (
            "MCP servers",
            "Resources",
            "tools http stdio connections configuration",
        ),
    ] {
        entries.push(SearchEntry {
            title,
            page,
            keywords,
        });
    }
    entries
}

#[derive(Default)]
pub(in crate::app) struct View {
    page: Page,
    search: String,
    models_search: String,
    models_select_mode: bool,
    models_selected: std::collections::BTreeSet<(String, String)>,
    pub(in crate::app) model_editor: Option<super::model_editor::ModelEditDraft>,
    scale: super::scale::Scale,
    pub(in crate::app) resources: resources::Resources,
    pub(in crate::app) providers: super::providers::ProvidersManager,
    /// The Auto classifier uses the composer's model list, with its own filters.
    classifier_open: bool,
    classifier_list: crate::app::model_picker::ListState,
    default_open: bool,
    default_list: crate::app::model_picker::ListState,
    defaults_error: Option<String>,
}

impl View {
    pub(in crate::app) fn location(&self) -> String {
        if self.page == Page::Resources {
            format!("Resources/{}", self.resources.navigation_view())
        } else {
            self.page.title().into()
        }
    }
    pub(in crate::app) fn navigation_blocked(&self) -> bool {
        self.resources.navigation_blocked()
            || self.providers.any_modal_open()
            || self.model_editor.is_some()
    }
    pub(in crate::app) fn select(&mut self, name: &str) {
        if let Some(view) = name.strip_prefix("Resources/") {
            self.page = Page::Resources;
            self.resources.select_view(view);
            self.search.clear();
            return;
        }
        if let Some(page) = Page::ALL
            .into_iter()
            .find(|p| p.title().eq_ignore_ascii_case(name))
        {
            self.page = page;
            self.search.clear();
        }
    }
}

pub(in crate::app) fn section(ui: &mut egui::Ui, title: &str, description: &str) {
    let colors = crate::theme::palette(ui.ctx());
    ui.add_space(24.0);
    ui.label(RichText::new(title).size(13.0).strong().color(colors.text));
    if !description.is_empty() {
        ui.add(egui::Label::new(RichText::new(description).size(12.0).color(colors.muted)).wrap());
    }
    ui.add_space(6.0);
}
pub(in crate::app) fn card(ui: &mut egui::Ui, body: impl FnOnce(&mut egui::Ui)) {
    let colors = crate::theme::palette(ui.ctx());
    egui::Frame::new()
        .fill(colors.settings_card)
        .stroke(Stroke::new(1.0, colors.border))
        .corner_radius(10)
        .inner_margin(egui::Margin::ZERO)
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.spacing_mut().item_spacing.y = 0.0;
            body(ui);
        });
}
pub(super) fn note(ui: &mut egui::Ui, title: &str, description: &str) {
    let colors = theme::palette(ui.ctx());
    card(ui, |ui| {
        egui::Frame::new().inner_margin(18).show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.spacing_mut().item_spacing.y = 8.0;
            ui.label(RichText::new(title).size(13.0).strong().color(colors.text));
            ui.add(
                egui::Label::new(RichText::new(description).size(12.0).color(colors.muted)).wrap(),
            );
        });
    });
}

pub(in crate::app) fn separator(ui: &mut egui::Ui) {
    let colors = crate::theme::palette(ui.ctx());
    let (rect, _) = ui.allocate_exact_size(Vec2::new(ui.available_width(), 1.0), Sense::hover());
    ui.painter().hline(
        rect.x_range(),
        rect.center().y,
        Stroke::new(1.0, colors.border.gamma_multiply(0.65)),
    );
}
pub(super) fn provider_row(
    ui: &mut egui::Ui,
    provider: &str,
    title: &str,
    description: &str,
    body: impl FnOnce(&mut egui::Ui),
) {
    let colors = crate::theme::palette(ui.ctx());
    egui::Frame::new()
        .inner_margin(egui::Margin::symmetric(18, 14))
        .show(ui, |ui| {
            let width = ui.available_width();
            let controls = (width * 0.46).clamp(190.0, 340.0);
            let origin = ui.cursor().min;
            let icon_w = 24.0;
            let label_width = (width - controls - icon_w - 18.0).max(80.0);
            let mut labels = ui.new_child(
                egui::UiBuilder::new()
                    .id_salt((provider, title, "label"))
                    .max_rect(egui::Rect::from_min_size(
                        origin + Vec2::new(icon_w, 0.0),
                        Vec2::new(label_width, 30.0),
                    ))
                    .layout(Layout::top_down(Align::LEFT)),
            );
            labels.set_width(label_width);
            labels.spacing_mut().item_spacing.y = 3.0;
            labels.add(egui::Label::new(RichText::new(title).size(13.0).color(colors.text)).wrap());
            if !description.is_empty() {
                labels.add(
                    egui::Label::new(RichText::new(description).size(11.5).color(colors.muted))
                        .wrap(),
                );
            }
            let height = labels.min_rect().height().max(theme::CONTROL_HEIGHT);
            crate::icons::draw_provider(
                ui,
                egui::Rect::from_center_size(
                    egui::pos2(origin.x + 8.0, origin.y + height * 0.5),
                    Vec2::splat(16.0),
                ),
                provider,
            );
            let mut right = ui.new_child(
                egui::UiBuilder::new()
                    .id_salt((provider, title, "control"))
                    .max_rect(egui::Rect::from_min_size(
                        egui::pos2(origin.x + width - controls, origin.y),
                        Vec2::new(controls, height),
                    ))
                    .layout(Layout::right_to_left(Align::Center)),
            );
            right.set_width(controls);
            right.set_height(height);
            right.spacing_mut().item_spacing.x = 8.0;
            body(&mut right);
            ui.allocate_exact_size(Vec2::new(width, height), Sense::hover());
        });
}

pub(in crate::app) fn row(
    ui: &mut egui::Ui,
    title: &str,
    description: &str,
    body: impl FnOnce(&mut egui::Ui),
) {
    let colors = crate::theme::palette(ui.ctx());
    egui::Frame::new()
        .inner_margin(egui::Margin::symmetric(18, 16))
        .show(ui, |ui| {
            let width = ui.available_width();
            let controls = (width * 0.40).clamp(180.0, 260.0);
            // Pin both columns to the card's edges. A horizontal auto-sized
            // child shrinks to its label and makes the control drift per row.
            let origin = ui.cursor().min;
            let label_width = (width - controls - 18.0).max(80.0);
            let mut labels = ui.new_child(
                egui::UiBuilder::new()
                    .id_salt((title, "label"))
                    .max_rect(egui::Rect::from_min_size(
                        origin,
                        Vec2::new(label_width, 30.0),
                    ))
                    .layout(Layout::top_down(Align::LEFT)),
            );
            labels.set_width(label_width);
            labels.spacing_mut().item_spacing.y = 4.0;
            labels.add(egui::Label::new(RichText::new(title).size(13.0).color(colors.text)).wrap());
            if !description.is_empty() {
                labels.add(
                    egui::Label::new(RichText::new(description).size(11.5).color(colors.muted))
                        .wrap(),
                );
            }
            let height = labels.min_rect().height().max(theme::CONTROL_HEIGHT);
            let mut right = ui.new_child(
                egui::UiBuilder::new()
                    .id_salt((title, "control"))
                    .max_rect(egui::Rect::from_min_size(
                        origin + egui::vec2(width - controls, 0.0),
                        Vec2::new(controls, height),
                    ))
                    .layout(Layout::right_to_left(Align::Center)),
            );
            body(&mut right);
            ui.allocate_rect(
                egui::Rect::from_min_size(
                    origin,
                    Vec2::new(width, height.max(right.min_rect().height())),
                ),
                Sense::hover(),
            );
        });
}
pub(in crate::app) fn toggle(ui: &mut egui::Ui, value: &mut bool, title: &str) {
    let colors = theme::palette(ui.ctx());
    let (target, response) =
        ui.allocate_exact_size(Vec2::new(38.0, theme::CONTROL_HEIGHT), Sense::click());
    let rect = egui::Rect::from_center_size(target.center(), Vec2::new(34.0, 20.0));
    let response = response.on_hover_cursor(egui::CursorIcon::PointingHand);
    if response.clicked() || (response.has_focus() && ui.input(|i| i.key_pressed(egui::Key::Space)))
    {
        *value = !*value;
    }
    response.widget_info(|| {
        egui::WidgetInfo::selected(egui::WidgetType::Checkbox, ui.is_enabled(), *value, title)
    });
    let fill = if *value {
        colors.switch_on
    } else {
        colors.switch_off
    };
    ui.painter().rect_filled(rect, 10, fill);
    let x = if *value {
        rect.right() - 10.0
    } else {
        rect.left() + 10.0
    };
    ui.painter()
        .circle_filled(egui::pos2(x, rect.center().y), 8.0, Color32::from_gray(245));
    if response.has_focus() {
        ui.painter().rect_stroke(
            target.expand(2.0),
            7,
            Stroke::new(1.0, colors.accent),
            egui::StrokeKind::Outside,
        );
    }
}
fn keycaps(ui: &mut egui::Ui, keys: &[&str]) {
    let colors = crate::theme::palette(ui.ctx());
    for key in keys.iter().rev() {
        egui::Frame::new()
            .fill(colors.hover)
            .stroke(Stroke::new(1.0, colors.border))
            .corner_radius(5)
            .inner_margin(egui::Margin::symmetric(6, 3))
            .show(ui, |ui| {
                ui.label(RichText::new(*key).size(11.0).color(colors.muted));
            });
    }
}
fn navigation(ui: &mut egui::Ui, page: Page, selected: bool) -> egui::Response {
    let colors = crate::theme::palette(ui.ctx());
    let (rect, response) = ui.allocate_exact_size(
        Vec2::new(ui.available_width(), theme::CONTROL_HEIGHT),
        Sense::click(),
    );
    if selected || response.hovered() {
        ui.painter().rect_filled(
            rect,
            8,
            if selected {
                colors.accent_bg
            } else {
                colors.surface
            },
        );
    }
    icons::draw(
        ui,
        egui::Rect::from_center_size(
            rect.left_center() + egui::vec2(15.0, 0.0),
            Vec2::splat(16.0),
        ),
        page.icon(),
        if selected {
            colors.accent
        } else {
            colors.muted
        },
    );
    ui.painter().text(
        rect.left_center() + egui::vec2(34.0, 0.0),
        egui::Align2::LEFT_CENTER,
        page.title(),
        FontId::proportional(12.0),
        if selected { colors.accent } else { colors.text },
    );
    response.widget_info(|| {
        egui::WidgetInfo::selected(
            egui::WidgetType::SelectableLabel,
            ui.is_enabled(),
            selected,
            page.title(),
        )
    });
    response.on_hover_cursor(egui::CursorIcon::PointingHand)
}
use crate::app::model_picker::{ListState, ModelList, Pick, model_list};

pub(super) struct Dropdown<'a> {
    /// Scopes widget IDs and the list's filters.
    pub salt: &'a str,
    pub title: &'a str,
    pub models: &'a [protocol::Model],
    /// `provider/id` of the selection; `None` selects Automatic (if offered).
    pub selected: Option<&'a str>,
    pub automatic: Option<(&'a str, &'a str)>,
    pub empty: &'a str,
}

fn model_label(model: &protocol::Model) -> String {
    format!(
        "{} · {}",
        if model.name.is_empty() {
            &model.id
        } else {
            &model.name
        },
        crate::accounts::display_provider(&model.provider)
    )
}

/// A Settings button that opens the composer's searchable, priced model list.
pub(super) fn model_dropdown(
    ui: &mut egui::Ui,
    open: &mut bool,
    list_state: &mut ListState,
    dropdown: Dropdown<'_>,
) -> Option<Pick> {
    let colors = theme::palette(ui.ctx());
    let label = match (dropdown.selected, dropdown.automatic) {
        (None, Some((automatic, _))) => automatic.to_owned(),
        (None, None) => "Choose a model".to_owned(),
        (Some(key), _) => dropdown
            .models
            .iter()
            .find(|m| format!("{}/{}", m.provider, m.id) == key)
            .map(model_label)
            .unwrap_or_else(|| key.to_owned()),
    };
    let width = 230.0_f32.min(ui.available_width());
    let (rect, response) =
        ui.allocate_exact_size(Vec2::new(width, theme::CONTROL_HEIGHT), Sense::click());
    ui.painter().rect(
        rect,
        7,
        if *open || response.hovered() {
            colors.hover
        } else {
            colors.surface
        },
        Stroke::new(
            1.0,
            if response.has_focus() {
                colors.accent
            } else {
                colors.border
            },
        ),
        egui::StrokeKind::Inside,
    );
    let mut job = egui::text::LayoutJob::simple_singleline(
        label.clone(),
        FontId::proportional(12.0),
        colors.text,
    );
    job.wrap.max_width = (rect.width() - 34.0).max(0.0);
    job.wrap.max_rows = 1;
    let galley = ui.painter().layout_job(job);
    ui.painter().galley(
        egui::pos2(rect.left() + 10.0, rect.center().y - galley.size().y / 2.0),
        galley,
        colors.text,
    );
    icons::chevron(
        ui,
        egui::Rect::from_center_size(
            rect.right_center() - egui::vec2(14.0, 0.0),
            Vec2::splat(10.0),
        ),
        false,
        colors.dim,
    );
    response.widget_info(|| {
        egui::WidgetInfo::labeled(
            egui::WidgetType::Button,
            true,
            format!("{}: {label}", dropdown.title),
        )
    });
    let response = response
        .on_hover_text(label.as_str())
        .on_hover_cursor(egui::CursorIcon::PointingHand);
    #[cfg(test)]
    ui.ctx().data_mut(|d| {
        d.insert_temp(
            egui::Id::new(format!("{}-rect", dropdown.salt)),
            response.rect,
        )
    });
    if response.clicked() {
        *open = !*open;
        if *open {
            list_state.query.clear();
            list_state.focus = true;
        }
    }
    if !*open {
        return None;
    }
    if !list_state.provider.is_empty()
        && !dropdown
            .models
            .iter()
            .any(|m| m.provider == list_state.provider)
    {
        list_state.provider.clear();
    }
    let ctx = ui.ctx().clone();
    let popup_width = 340.0_f32.min((ctx.screen_rect().width() - 16.0).max(120.0));
    // The provider filter is a child popup; its click must not close this one.
    let close_behavior = if egui::Popup::is_any_open(&ctx) {
        egui::PopupCloseBehavior::IgnoreClicks
    } else {
        egui::PopupCloseBehavior::CloseOnClickOutside
    };
    let selected = dropdown.selected.and_then(|key| key.split_once('/'));
    let mut still_open = true;
    let mut pick = None;
    egui::Popup::new(
        egui::Id::new((dropdown.salt, "settings-model-dropdown")),
        ctx.clone(),
        response.rect,
        ui.layer_id(),
    )
    .open_bool(&mut still_open)
    .align(egui::emath::RectAlign::BOTTOM_END)
    .align_alternatives(&[egui::emath::RectAlign::TOP_END])
    .close_behavior(close_behavior)
    .width(popup_width)
    .frame(prompt_ui::popup(&ctx).inner_margin(6))
    .gap(6.0)
    .show(|ui| {
        theme::menu_controls(ui);
        ui.set_width(popup_width - 12.0);
        let refs: Vec<&protocol::Model> = dropdown.models.iter().collect();
        pick = model_list(
            ui,
            list_state,
            ModelList {
                salt: dropdown.salt,
                models: &refs,
                selected,
                enabled: true,
                automatic: dropdown
                    .automatic
                    .map(|(label, description)| (label, description, dropdown.selected.is_none())),
            },
            |_| {},
            |_| 260.0,
            dropdown.empty,
        );
    });
    *open = pick.is_none() && still_open;
    pick
}

impl Pui {
    pub(in crate::app) fn session_settings(&mut self, ctx: &egui::Context) {
        let colors = crate::theme::palette(ctx);
        if !self.settings_open {
            return;
        }
        let before = self.prefs.settings.clone();
        let alerts_before = self.prefs.notifications;
        let theme_before = (self.prefs.theme, self.prefs.color_styles);

        if ctx.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::Escape))
            && (self.settings_view.page != Page::Resources
                || !self.settings_view.resources.escape())
            && !self.settings_view.providers.escape()
            && !self.escape_model_editor()
            && !std::mem::take(&mut self.settings_view.classifier_open)
            && !std::mem::take(&mut self.settings_view.default_open)
        {
            self.settings_open = false;
            self.focus_composer = true;
        }
        let query = self.settings_view.search.trim().to_lowercase();
        if !query.is_empty() && !self.settings_view.page.matches(&query) {
            if let Some(page) = Page::ALL.into_iter().find(|p| p.matches(&query)) {
                self.settings_view.page = page;
            }
        }
        self.settings_navigation_header(ctx);
        egui::SidePanel::left("settings-navigation")
            .exact_width(244.0)
            .resizable(false)
            .show_separator_line(true)
            .frame(
                egui::Frame::new()
                    .fill(colors.settings_nav)
                    .inner_margin(12),
            )
            .show(ctx, |ui| {
                ui.add_space(8.0);
                ui.add(
                    theme::text_field(&mut self.settings_view.search, "Search settings")
                        .id_salt("settings-search")
                        .desired_width(ui.available_width()),
                );
                ui.add_space(13.0);
                for (group, pages) in [("App", &Page::ALL[..5]), ("Agent", &Page::ALL[5..])] {
                    let visible: Vec<_> = pages
                        .iter()
                        .copied()
                        .filter(|p| p.matches(&query))
                        .collect();
                    if visible.is_empty() {
                        continue;
                    }
                    ui.label(RichText::new(group).size(11.0).color(colors.dim));
                    ui.add_space(3.0);
                    for page in visible {
                        if navigation(ui, page, self.settings_view.page == page).clicked() {
                            self.settings_view.page = page;
                        }
                    }
                    ui.add_space(18.0);
                }
                if !query.is_empty() && !Page::ALL.iter().any(|p| p.matches(&query)) {
                    ui.label(theme::small(ui, "No matching settings"));
                }
            });
        egui::CentralPanel::default()
            .frame(egui::Frame::new().fill(colors.settings_bg))
            .show(ctx, |ui| {
                // Keep native titlebar dragging available in the blank top margin.
                let drag = egui::Rect::from_min_size(
                    ui.max_rect().min,
                    Vec2::new(ui.available_width(), 42.0),
                );
                if ui
                    .interact(drag, ui.id().with("settings-titlebar"), Sense::drag())
                    .drag_started()
                {
                    ctx.send_viewport_cmd(egui::ViewportCommand::StartDrag);
                }
                if self.settings_view.page == Page::Resources {
                    egui::Frame::new()
                        .inner_margin(egui::Margin::symmetric(24, 12))
                        .show(ui, |ui| {
                            self.settings_view
                                .resources
                                .show(ui, &self.prefs.cwd, self.demo);
                        });
                    return;
                }
                egui::ScrollArea::vertical()
                    .id_salt(("settings-content", self.settings_view.page))
                    .auto_shrink([false, false])
                    .show(ui, |ui| {
                        let width = (ui.available_width() - 64.0).clamp(320.0, 820.0);
                        let padding = ((ui.available_width() - width) / 2.0).max(24.0);
                        ui.horizontal(|ui| {
                            ui.add_space(padding);
                            ui.vertical(|ui| {
                                ui.set_width(width);
                                ui.add_space(72.0);
                                let page = self.settings_view.page;
                                ui.label(
                                    RichText::new(page.title())
                                        .size(26.0)
                                        .strong()
                                        .color(colors.text),
                                );
                                ui.add_space(1.0);
                                ui.add(
                                    egui::Label::new(
                                        RichText::new(page.subtitle())
                                            .size(12.5)
                                            .color(colors.muted),
                                    )
                                    .wrap(),
                                );
                                ui.add_space(8.0);
                                match page {
                                    Page::General => self.settings_general(ui),
                                    Page::Appearance => self.settings_appearance(ui),
                                    Page::Notifications => self.settings_notifications(ui, ctx),
                                    Page::Shortcuts => self.settings_shortcuts(ui),
                                    Page::Permissions => self.settings_permissions(ui),
                                    Page::Models => self.settings_models(ui),
                                    Page::Providers => self.settings_providers(ui),
                                    Page::Resources => self.settings_view.resources.show(
                                        ui,
                                        &self.prefs.cwd,
                                        self.demo,
                                    ),
                                    Page::Remote => self.settings_remote(ui),
                                }
                                ui.add_space(45.0);
                            });
                        });
                    });
            });
        self.propagate_notifications(ctx, alerts_before);
        if theme_before != (self.prefs.theme, self.prefs.color_styles) {
            self.prefs.color_styles.apply(ctx, self.prefs.theme);
            ctx.request_repaint();
        }
        self.propagate_settings(&before);
        self.settings_view.providers.show_modals(ctx);
        self.show_model_editor_dialog(ctx);
    }

    fn escape_model_editor(&mut self) -> bool {
        let Some(draft) = self.settings_view.model_editor.as_mut() else {
            return false;
        };
        if !draft.escape() {
            self.settings_view.model_editor = None;
        }
        true
    }

    fn show_model_editor_dialog(&mut self, ctx: &egui::Context) {
        if self.settings_view.model_editor.is_none() {
            return;
        }
        let all_models = self.configured_models();
        let existing_fallbacks = self.settings_view.providers.model_fallbacks.clone();
        let outcome = {
            let draft = self.settings_view.model_editor.as_mut().unwrap();
            super::model_editor::show_model_editor(ctx, draft, &all_models, &existing_fallbacks)
        };
        match outcome {
            super::model_editor::EditorOutcome::Open => {}
            super::model_editor::EditorOutcome::Cancel => {
                self.settings_view.model_editor = None;
            }
            super::model_editor::EditorOutcome::Save(patch) => {
                match self.settings_view.providers.apply_model_edits(&patch) {
                    Ok(()) => {
                        self.apply_model_patch_in_memory(&patch);
                        self.settings_view.model_editor = None;
                    }
                    Err(err) => {
                        if let Some(draft) = self.settings_view.model_editor.as_mut() {
                            draft.error = Some(err);
                        }
                    }
                }
            }
            super::model_editor::EditorOutcome::Reset(targets) => {
                match self.settings_view.providers.reset_model_edits(&targets) {
                    Ok(()) => {
                        self.settings_view.model_editor = None;
                    }
                    Err(err) => {
                        if let Some(draft) = self.settings_view.model_editor.as_mut() {
                            draft.error = Some(err);
                        }
                    }
                }
            }
        }
    }

    fn apply_model_patch_in_memory(&mut self, patch: &super::model_editor::ModelEditPatch) {
        use super::model_editor::FieldEdit;
        for (provider, model_id) in &patch.targets {
            for m in &mut self.state.models {
                if m.provider == *provider && m.id == *model_id {
                    if let FieldEdit::Set(name) = &patch.name {
                        m.name = name.clone();
                    }
                    if let FieldEdit::Set(tokens) = patch.context_window {
                        m.context_window = tokens;
                    }
                    if let FieldEdit::Set(v) = patch.input_cost {
                        m.input_cost = v;
                    }
                    if let FieldEdit::Set(v) = patch.output_cost {
                        m.output_cost = v;
                    }
                }
            }
            for entry in &mut self.catalog.models {
                if entry.model.provider == *provider && entry.model.id == *model_id {
                    if let FieldEdit::Set(name) = &patch.name {
                        entry.model.name = name.clone();
                    }
                    if let FieldEdit::Set(v) = patch.vision {
                        entry.vision = v;
                    }
                    if let FieldEdit::Set(v) = patch.reasoning {
                        entry.reasoning = v;
                    }
                    if let FieldEdit::Set(tokens) = patch.context_window {
                        entry.model.context_window = tokens;
                    }
                    if let FieldEdit::Set(tokens) = patch.max_tokens {
                        entry.max_tokens = tokens;
                    }
                    if let FieldEdit::Set(v) = patch.input_cost {
                        entry.model.input_cost = v;
                    }
                    if let FieldEdit::Set(v) = patch.output_cost {
                        entry.model.output_cost = v;
                    }
                    if let FieldEdit::Set(v) = patch.cache_read_cost {
                        entry.cache_read_cost = v;
                    }
                    if let FieldEdit::Set(v) = patch.cache_write_cost {
                        entry.cache_write_cost = v;
                    }
                    match &patch.thinking_format {
                        FieldEdit::Set(v) => entry.thinking_format = Some(v.clone()),
                        FieldEdit::Clear => entry.thinking_format = None,
                        FieldEdit::Keep => {}
                    }
                }
            }
            if let FieldEdit::Set(tokens) = patch.context_window
                && self.state.model == *model_id
                && (self.state.provider.is_empty() || self.state.provider == *provider)
            {
                self.state.context_window = tokens as f64;
            }
        }
    }
    fn settings_general(&mut self, ui: &mut egui::Ui) {
        let colors = crate::theme::palette(ui.ctx());
        section(ui, "Messages", "");
        card(ui, |ui| {
            row(
                ui,
                "Messages while working",
                "Steer at the next opening, or queue until the current response finishes.",
                |ui| {
                    for (mode, label) in [
                        (DeliveryMode::Queue, "Queue"),
                        (DeliveryMode::Steer, "Steer"),
                    ] {
                        if theme::choice(ui, self.prefs.settings.delivery == mode, label).clicked()
                        {
                            self.prefs.settings.delivery = mode;
                        }
                    }
                },
            );
            separator(ui);
            row(
                ui,
                "Send shortcuts",
                "Shift+Enter inserts a new line.",
                |ui| {
                    ui.label(theme::small(
                        ui,
                        format!(
                            "Enter: {} · {}Enter: {}",
                            self.prefs.settings.delivery.label(),
                            command(),
                            self.prefs.settings.delivery.opposite().label()
                        ),
                    ));
                },
            );
        });
        section(
            ui,
            "Context",
            "Older context is summarized, never deleted from the saved session.",
        );
        ui.label(theme::small(
            ui,
            "Changes apply at the next context check. A summary already running is left alone.",
        ));
        card(ui, |ui| {
            ui.scope(|ui| {
                row(
                    ui,
                    "Auto compact at",
                    "Percentage of the model context window used before summarizing.",
                    |ui| {
                        ui.spacing_mut().slider_width = 115.0;
                        theme::slider_style(ui);
                        ui.add(
                            egui::Slider::new(&mut self.prefs.settings.compact_percent, 50..=95)
                                .suffix("%")
                                .trailing_fill(true),
                        );
                    },
                );
                separator(ui);
                row(
                    ui,
                    "Keep recent context",
                    "Tokens retained without summarizing; capped for smaller context windows.",
                    |ui| {
                        ui.add(
                            egui::DragValue::new(&mut self.prefs.settings.keep_recent_tokens)
                                .range(1000..=200000)
                                .speed(1000)
                                .suffix(" tokens"),
                        );
                    },
                );
            });
        });
        if let Some(error) = &self.state.compaction_settings_error {
            ui.add_space(8.0);
            ui.label(RichText::new(error).size(11.0).color(colors.red));
        }
        section(ui, "Ultracode & Workflows", "");
        card(ui, |ui| {
            let mut dynamic_enabled = !self.prefs.dynamic_workflows_disabled;
            row(
                ui,
                "Dynamic workflows",
                "Let agents run phased multi-agent workflows in parallel inside Background tasks. Workflows can use your usage limit quickly.",
                |ui| {
                    toggle(ui, &mut dynamic_enabled, "Dynamic workflows");
                    self.prefs.dynamic_workflows_disabled = !dynamic_enabled;
                },
            );
            separator(ui);
            row(
                ui,
                "Worker concurrency ceiling",
                "A limit, not a target team size. The parent chooses the fewest useful workers.",
                |ui| {
                    ui.add(egui::DragValue::new(&mut self.prefs.agent_concurrency).range(1..=16));
                },
            )
        });
        self.settings_pi_folder(ui);
        section(ui, "Updates", "");
        card(ui, |ui| {
            row(
                ui,
                "Automatically check for updates",
                "Periodically check GitHub Releases in the background. Updates never install or restart automatically without your confirmation.",
                |ui| {
                    toggle(
                        ui,
                        &mut self.prefs.settings.auto_check_updates,
                        "Automatically check for updates",
                    );
                },
            );
            separator(ui);
            let status_desc = if self.updates.ready_to_install() {
                "An update is downloaded, verified and ready to install.".to_owned()
            } else if let Some(version) = self.updates.available_version() {
                format!("Pui {version} is available on GitHub.")
            } else {
                format!("Installed version {}", env!("CARGO_PKG_VERSION"))
            };
            row(ui, "Software updates", &status_desc, |ui| {
                if ui.button("Check for Updates…").clicked() {
                    self.updates.start(ui.ctx());
                }
            });
            separator(ui);
            let (pi_version, pi_source) = match crate::runtime::pi_version() {
                Some(version) if crate::runtime::active_bundle().is_some() => {
                    (version, "Bundled inside Pui; updated with Pui releases.")
                }
                Some(version) => (version, "External Pi installation."),
                None => (
                    "Unknown".to_owned(),
                    "Pi package version could not be read.",
                ),
            };
            row(ui, "Pi version", pi_source, |ui| {
                ui.label(theme::small(ui, format!("Pi {pi_version}")));
            });
        });
    }
    fn settings_appearance(&mut self, ui: &mut egui::Ui) {
        section(ui, "Theme", "");
        card(ui, |ui| {
            row(
                ui,
                "Mode",
                "Choose light or dark, then pick a theme below. Your choices are saved automatically.",
                |ui| {
                    for (mode, label) in
                        [(theme::Theme::Light, "Light"), (theme::Theme::Dark, "Dark")]
                    {
                        if theme::choice(ui, self.prefs.theme == mode, label).clicked() {
                            self.prefs.theme = mode;
                        }
                    }
                },
            )
        });
        super::appearance::styles(ui, self.prefs.theme, &mut self.prefs.color_styles);
        section(ui, "Workspace", "");
        card(ui, |ui| {
            row(
                ui,
                "Session sidebar",
                "Show your projects, conversations and agent trees.",
                |ui| {
                    let mut shown = !self.prefs.hide_sidebar;
                    toggle(ui, &mut shown, "Session sidebar");
                    self.prefs.hide_sidebar = !shown;
                },
            );
            separator(ui);
            row(
                ui,
                "Inspector",
                "Show context, outputs and workspace detail cards when space allows.",
                |ui| {
                    let mut shown = !self.prefs.hide_inspector;
                    toggle(ui, &mut shown, "Inspector");
                    self.prefs.hide_inspector = !shown;
                },
            );
            separator(ui);
            row(
                ui,
                "Inspector layout",
                "Restore the default card order, sizes and visibility.",
                |ui| {
                    if ui.button("Reset layout").clicked() {
                        self.prefs.cards = CardLayout::default();
                    }
                },
            );
        });
        section(ui, "Scale", "");
        card(ui, |ui| {
            row(
                ui,
                "Interface scale",
                "Changes apply when you release the slider. Type a percentage and press Enter for an exact value.",
                |ui| self.settings_view.scale.show(ui),
            )
        });
    }
    fn settings_notifications(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        section(
            ui,
            "System",
            "macOS decides whether Pui can show desktop notifications.",
        );
        let status = self.notifications.status.clone().unwrap_or_else(|| {
            "Notification permission has not been checked in this window.".into()
        });
        card(ui, |ui| {
            row(ui, "macOS notification access", &status, |ui| {
                if ui
                    .add_enabled(
                        !self.demo && notifications::Notifications::supported(),
                        egui::Button::new("Ask macOS"),
                    )
                    .clicked()
                {
                    self.notifications.configure(ctx);
                }
            });
        });
        section(
            ui,
            "Agent completion",
            "Includes background chats and workers. Shared chats notify once, from the owning window.",
        );
        card(ui, |ui| {
            ui.add_enabled_ui(notifications::Notifications::supported(), |ui| {
                row(
                    ui,
                    "Completion sound",
                    "Play a short sound when an agent finishes successfully or a chat is deleted.",
                    |ui| toggle(ui, &mut self.prefs.notifications.sound, "Completion sound"),
                );
                separator(ui);
                row(
                    ui,
                    "OS notification",
                    "Show a desktop banner when an agent finishes successfully.",
                    |ui| toggle(ui, &mut self.prefs.notifications.desktop, "OS notification"),
                );
                separator(ui);
                row(
                    ui,
                    "Test alerts",
                    "Preview your selected sound and notification combination.",
                    |ui| {
                        if ui
                            .add_enabled(
                                !self.demo
                                    && (self.prefs.notifications.sound
                                        || self.prefs.notifications.desktop),
                                egui::Button::new("Test alerts"),
                            )
                            .clicked()
                        {
                            self.notifications.complete(
                                ctx,
                                self.prefs.notifications,
                                "Pui completion alert test",
                                None,
                            );
                        }
                    },
                );
            });
        });
        ui.add_space(10.0);
        ui.label(theme::small(ui, "No success alerts for stopped, aborted or failed runs. Banners follow macOS Focus settings."));
        if !notifications::Notifications::supported() {
            ui.label(theme::small(
                ui,
                "Completion alerts currently support macOS only.",
            ));
        }
    }
    fn settings_shortcuts(&self, ui: &mut egui::Ui) {
        let cmd = command();
        section(ui, "App", "");
        card(ui, |ui| {
            for (index, (title, keys)) in [
                ("Open settings", vec![cmd, ","]),
                ("Search chats, messages and settings", vec![cmd, "K"]),
                ("Quick Open workspace file (:line)", vec![cmd, "P"]),
                ("Toggle sidebar", vec![cmd, "B"]),
                ("Toggle inspector", vec![cmd, "I"]),
                ("New conversation", vec![cmd, "N"]),
                ("Close browser tab / window", vec![cmd, "W"]),
            ]
            .into_iter()
            .enumerate()
            {
                if index > 0 {
                    separator(ui);
                }
                row(ui, title, "", |ui| keycaps(ui, &keys));
            }
        });
        section(ui, "Composer", "");
        card(ui, |ui| {
            for (index, (title, keys)) in [
                (
                    format!(
                        "Send / {} while working",
                        self.prefs.settings.delivery.label()
                    ),
                    vec!["Enter"],
                ),
                (
                    format!(
                        "{} while working",
                        self.prefs.settings.delivery.opposite().label()
                    ),
                    vec![cmd, "Enter"],
                ),
                ("Recall sent messages in this chat".into(), vec!["↑", "↓"]),
                ("New line".into(), vec!["Shift", "Enter"]),
                ("Stop agent (chat focus)".into(), vec!["Esc"]),
            ]
            .into_iter()
            .enumerate()
            {
                if index > 0 {
                    separator(ui);
                }
                row(ui, &title, "", |ui| keycaps(ui, &keys));
            }
        });
        section(
            ui,
            "Approval requests",
            "These shortcuts take priority over sending while an approval is visible.",
        );
        card(ui, |ui| {
            row(ui, "Allow once", "Approve only the current action.", |ui| {
                keycaps(ui, &[cmd, "Enter"])
            });
            separator(ui);
            row(
                ui,
                "Deny",
                "Reject the current action without stopping the run.",
                |ui| keycaps(ui, &[cmd, "Backspace"]),
            );
        });
        section(ui, "Navigation", "");
        card(ui, |ui| {
            row(
                ui,
                "Previous view",
                "Also available with the mouse back button.",
                |ui| {
                    keycaps(
                        ui,
                        &[
                            if cfg!(target_os = "macos") {
                                "⌘"
                            } else {
                                "Alt"
                            },
                            "[",
                        ],
                    )
                },
            );
            separator(ui);
            row(
                ui,
                "Next view",
                "Also available with the mouse forward button.",
                |ui| {
                    keycaps(
                        ui,
                        &[
                            if cfg!(target_os = "macos") {
                                "⌘"
                            } else {
                                "Alt"
                            },
                            "]",
                        ],
                    )
                },
            );
            separator(ui);
            row(
                ui,
                "Close settings",
                "Return to the workspace without interrupting work.",
                |ui| keycaps(ui, &["Esc"]),
            );
            separator(ui);
            row(
                ui,
                "Message rail",
                "Focus the rail, then jump between sent messages.",
                |ui| keycaps(ui, &["Alt", "↑ / ↓"]),
            );
        });
    }
    fn settings_permissions(&mut self, ui: &mut egui::Ui) {
        section(
            ui,
            "Tool access",
            "Permission checks include nested codemode calls. They are not an OS sandbox.",
        );
        ui.label(theme::small(ui, "Choose each chat's agent mode from the composer. New chats start in the mode you selected last (Auto on first launch). Changes apply to subsequent tool calls; commands already executing are not interrupted."));
        // Independent of the selected chat: the app-level catalog and Pi's files.
        section(
            ui,
            "Auto classifier",
            "Used by chats in Auto mode. Blocked calls return to the agent, not an approval prompt. The agent continues safe work and reports blocked attempts.",
        );
        card(ui, |ui| {
            row(
                ui,
                "Classifier model",
                "Automatic uses the lowest input + output price across the models shown in the model picker. Sends the proposed call and recent requests to that provider. Applies to all chats, without changing their conversation models.",
                |ui| self.classifier_picker(ui),
            );
            separator(ui);
            let resolved = self.resolved_classifier();
            row(
                ui,
                "Resolved model",
                "Uses configured prices, including zero-price entries. An unavailable override never silently falls back to another model.",
                |ui| {
                    ui.add(egui::Label::new(theme::small(ui, resolved)).wrap());
                },
            );
        });
        section(ui, "Confirmations", "");
        card(ui, |ui| {
            row(
                ui,
                "Bypass confirmation",
                if self.prefs.skip_bypass_confirmation {
                    "Skipped: you chose \"Don't ask again\". Reset to confirm before every switch to Bypass."
                } else {
                    "Pui asks before switching a chat to Bypass all permissions."
                },
                |ui| {
                    if ui
                        .add_enabled(
                            self.prefs.skip_bypass_confirmation,
                            egui::Button::new("Reset"),
                        )
                        .clicked()
                    {
                        self.prefs.skip_bypass_confirmation = false;
                    }
                },
            );
        });
        section(ui, "Safety", "");
        note(
            ui,
            "Trusted extensions",
            "Extensions can run their own code. Bypass skips Pui approvals, not OS permissions or read-only worker restrictions. These are tool-level controls, not a security sandbox.",
        );
    }
    /// Classifier choices from the app catalog (picker scope, Auto's
    /// eligibility rules); before the first catalog load, the chat's report.
    fn classifier_choices(&self) -> Vec<protocol::Model> {
        if !self.catalog.loaded {
            return self.state.permission_classifier_models.clone();
        }
        let providers = &self.settings_view.providers;
        self.catalog
            .classifier_models(
                |p| providers.is_provider_configured(p),
                &self.prefs.hidden_models,
            )
            .into_iter()
            .map(|m| m.model.clone())
            .collect()
    }
    fn resolved_classifier(&self) -> String {
        const UNAVAILABLE: &str = "Unavailable · calls needing classification are blocked";
        if !self.catalog.loaded {
            return self
                .state
                .permission_classifier
                .clone()
                .unwrap_or_else(|| UNAVAILABLE.into());
        }
        let providers = &self.settings_view.providers;
        let candidates = self.catalog.classifier_models(
            |p| providers.is_provider_configured(p),
            &self.prefs.hidden_models,
        );
        let found = match &self.prefs.settings.permission_classifier {
            Some(key) => candidates.iter().find(|m| m.key() == *key),
            None => candidates.iter().find(|m| m.priced),
        };
        found.map(|m| m.key()).unwrap_or_else(|| UNAVAILABLE.into())
    }
    fn classifier_picker(&mut self, ui: &mut egui::Ui) {
        const AUTOMATIC: &str = "Automatic · cheapest available";
        let models = self.classifier_choices();
        let selected = self.prefs.settings.permission_classifier.clone();
        let pick = model_dropdown(
            ui,
            &mut self.settings_view.classifier_open,
            &mut self.settings_view.classifier_list,
            Dropdown {
                salt: "classifier",
                title: "Classifier model",
                models: &models,
                selected: selected.as_deref(),
                automatic: Some((
                    AUTOMATIC,
                    "Lowest-priced model in the picker; skips providers that fail.",
                )),
                empty: "No classifier models available. Connect a provider in Settings → Providers.",
            },
        );
        match pick {
            Some(Pick::Automatic) => self.prefs.settings.permission_classifier = None,
            Some(Pick::Model { provider, id }) => {
                self.prefs.settings.permission_classifier = Some(format!("{provider}/{id}"))
            }
            None => {}
        }
    }
    /// Models from configured providers (hidden ones included, so they can be
    /// shown again). App-level: never the selected chat's process.
    fn configured_models(&self) -> Vec<protocol::Model> {
        let providers = &self.settings_view.providers;
        if self.catalog.loaded {
            self.catalog
                .models
                .iter()
                .filter(|m| providers.is_provider_configured(&m.model.provider))
                .map(|m| m.model.clone())
                .collect()
        } else {
            self.state
                .models
                .iter()
                .filter(|m| providers.is_provider_configured(&m.provider))
                .cloned()
                .collect()
        }
    }
    fn settings_models(&mut self, ui: &mut egui::Ui) {
        self.settings_view.providers.ensure_loaded(self.demo);
        let models = self.configured_models();
        section(
            ui,
            "Defaults for new chats",
            "Saved in Pi's settings (~/.pi/agent/settings.json), shared with the Pi CLI. Each chat changes its own model from the composer; Pi remembers the last one you chose.",
        );
        let visible: Vec<protocol::Model> = models
            .iter()
            .filter(|m| {
                !self
                    .prefs
                    .hidden_models
                    .contains(&format!("{}/{}", m.provider, m.id))
            })
            .cloned()
            .collect();
        let defaults = self.catalog.defaults.clone();
        let default_key = defaults
            .provider
            .as_ref()
            .zip(defaults.model.as_ref())
            .map(|(p, m)| format!("{p}/{m}"));
        let mut write = None;
        card(ui, |ui| {
            row(
                ui,
                "Default model",
                "The model a new chat starts with.",
                |ui| {
                    if let Some(Pick::Model { provider, id }) = model_dropdown(
                        ui,
                        &mut self.settings_view.default_open,
                        &mut self.settings_view.default_list,
                        Dropdown {
                            salt: "default-model",
                            title: "Default model",
                            models: &visible,
                            selected: default_key.as_deref(),
                            automatic: None,
                            empty: "No models available. Connect a provider in Settings → Providers.",
                        },
                    ) {
                        write = Some((Some((provider, id)), None));
                    }
                },
            );
            separator(ui);
            row(
                ui,
                "Default thinking effort",
                "Applied when the model supports it; Pi uses the nearest supported level otherwise.",
                |ui| {
                    let current = defaults.thinking.clone().unwrap_or_default();
                    egui::ComboBox::from_id_salt("settings-default-effort")
                        .selected_text(if current.is_empty() {
                            "Pi default".to_owned()
                        } else {
                            thinking_label(&current)
                        })
                        .width(140.0)
                        .show_ui(ui, |ui| {
                            for level in ["off", "minimal", "low", "medium", "high", "xhigh"] {
                                if ui
                                    .selectable_label(current == level, thinking_label(level))
                                    .clicked()
                                {
                                    write = Some((None, Some(level.to_owned())));
                                }
                            }
                        });
                },
            );
        });
        if let Some((model, thinking)) = write {
            let result = catalog::write_defaults(
                model.as_ref().map(|(p, m)| (p.as_str(), m.as_str())),
                thinking.as_deref(),
            );
            match result {
                Ok(()) => self.catalog.defaults = catalog::read_defaults(),
                Err(error) => self.settings_view.defaults_error = Some(error),
            }
        }
        if let Some(error) = &self.settings_view.defaults_error {
            ui.add_space(6.0);
            ui.label(
                RichText::new(error)
                    .size(11.0)
                    .color(theme::palette(ui.ctx()).red),
            );
        }
        section(ui, "Available models", "");
        if let Some(error) = &self.catalog.error {
            ui.label(theme::small(
                ui,
                format!("{error} Showing the last loaded list."),
            ));
        } else if self.catalog.loading() && !self.catalog.loaded {
            ui.label(theme::small(ui, "Loading models from Pi…"));
        }
        if models.is_empty() {
            let message = if self.catalog.loading() {
                "Loading models from Pi…"
            } else {
                "No configured providers. Add a provider in Settings → Providers."
            };
            ui.label(theme::small(ui, message));
            return;
        }
        let colors = theme::palette(ui.ctx());
        let select_btn_w = 88.0;
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 8.0;
            let search_w = (ui.available_width() - select_btn_w - 8.0).max(120.0);
            egui::Frame::new()
                .fill(colors.surface)
                .stroke(Stroke::new(1.0_f32, colors.border))
                .corner_radius(8)
                .inner_margin(egui::Margin::symmetric(10, 6))
                .show(ui, |ui| {
                    ui.set_width((search_w - 22.0).max(80.0));
                    ui.horizontal(|ui| {
                        ui.spacing_mut().item_spacing.x = 6.0;
                        crate::icons::inline(ui, Icon::Search, colors.dim);
                        let clear_w = if self.settings_view.models_search.is_empty() {
                            0.0
                        } else {
                            22.0
                        };
                        let edit_w = (ui.available_width() - clear_w).max(60.0);
                        ui.add(
                            egui::TextEdit::singleline(&mut self.settings_view.models_search)
                                .id_salt("settings-models-search")
                                .hint_text("Search models by name, ID, or provider…")
                                .frame(false)
                                .desired_width(edit_w),
                        );
                        if !self.settings_view.models_search.is_empty()
                            && ui
                                .add(
                                    egui::Button::new(
                                        RichText::new("×").size(13.0).color(colors.muted),
                                    )
                                    .frame(false),
                                )
                                .on_hover_text("Clear search")
                                .clicked()
                        {
                            self.settings_view.models_search.clear();
                        }
                    });
                });
            let select_label = if self.settings_view.models_select_mode {
                "Done"
            } else {
                "Select"
            };
            if theme::choice(ui, self.settings_view.models_select_mode, select_label)
                .on_hover_text("Select multiple models to edit or show/hide together")
                .clicked()
            {
                self.settings_view.models_select_mode = !self.settings_view.models_select_mode;
                if !self.settings_view.models_select_mode {
                    self.settings_view.models_selected.clear();
                }
            }
        });

        let available_keys: std::collections::BTreeSet<(String, String)> = models
            .iter()
            .map(|m| (m.provider.clone(), m.id.clone()))
            .collect();
        self.settings_view
            .models_selected
            .retain(|k| available_keys.contains(k));

        let mut open_single: Option<protocol::Model> = None;
        let mut open_multi = false;

        if self.settings_view.models_select_mode && !self.settings_view.models_selected.is_empty() {
            let count = self.settings_view.models_selected.len();
            ui.add_space(10.0);
            egui::Frame::new()
                .fill(colors.accent_bg)
                .stroke(Stroke::new(1.0_f32, colors.accent.gamma_multiply(0.5)))
                .corner_radius(10)
                .inner_margin(egui::Margin::symmetric(12, 8))
                .show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    ui.horizontal(|ui| {
                        ui.label(
                            RichText::new(format!(
                                "{count} model{} selected",
                                if count == 1 { "" } else { "s" }
                            ))
                            .size(12.5)
                            .strong()
                            .color(colors.text),
                        );
                        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                            ui.spacing_mut().item_spacing.x = 6.0;
                            if prompt_ui::button(ui, "Clear", prompt_ui::Button::Secondary, true)
                                .clicked()
                            {
                                self.settings_view.models_selected.clear();
                            }
                            if prompt_ui::button(ui, "Hide", prompt_ui::Button::Secondary, true)
                                .clicked()
                            {
                                for (prov, id) in &self.settings_view.models_selected {
                                    self.prefs.hidden_models.insert(format!("{prov}/{id}"));
                                }
                            }
                            if prompt_ui::button(ui, "Show", prompt_ui::Button::Secondary, true)
                                .clicked()
                            {
                                for (prov, id) in &self.settings_view.models_selected {
                                    self.prefs.hidden_models.remove(&format!("{prov}/{id}"));
                                }
                            }
                            let edit_label = if count == 1 {
                                "Edit model".to_owned()
                            } else {
                                format!("Edit {count} models")
                            };
                            if prompt_ui::button(ui, &edit_label, prompt_ui::Button::Primary, true)
                                .clicked()
                            {
                                open_multi = true;
                            }
                        });
                    });
                });
        }

        self.settings_view.providers.ensure_loaded(self.demo);
        let query = self.settings_view.models_search.trim().to_ascii_lowercase();
        let mut by_provider: std::collections::BTreeMap<String, Vec<protocol::Model>> =
            std::collections::BTreeMap::new();
        for model in &models {
            if !query.is_empty() {
                let display = crate::accounts::display_provider(&model.provider);
                let hay = format!("{} {} {} {}", model.name, model.id, model.provider, display)
                    .to_ascii_lowercase();
                if !hay.contains(&query) {
                    continue;
                }
            }
            by_provider
                .entry(model.provider.clone())
                .or_default()
                .push(model.clone());
        }
        if by_provider.is_empty() {
            ui.add_space(12.0);
            ui.label(theme::small(
                ui,
                format!(
                    "No models match \"{}\".",
                    self.settings_view.models_search.trim()
                ),
            ));
            return;
        }
        for (provider, provider_models) in by_provider {
            let display = crate::accounts::display_provider(&provider);
            let heading = if display.eq_ignore_ascii_case(&provider) {
                display.to_owned()
            } else {
                format!("{display} ({provider})")
            };
            ui.add_space(20.0);
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 8.0;
                crate::icons::provider_inline(ui, &provider);
                ui.label(
                    RichText::new(&heading)
                        .size(13.5)
                        .strong()
                        .color(theme::palette(ui.ctx()).text),
                );
                ui.label(theme::small(
                    ui,
                    format!("· {} model(s)", provider_models.len()),
                ));
                if self.settings_view.models_select_mode {
                    let all_selected = provider_models.iter().all(|m| {
                        self.settings_view
                            .models_selected
                            .contains(&(m.provider.clone(), m.id.clone()))
                    });
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        let text = if all_selected {
                            "Deselect all"
                        } else {
                            "Select all"
                        };
                        if ui
                            .add(
                                egui::Label::new(
                                    RichText::new(text).size(11.5).color(colors.accent),
                                )
                                .sense(Sense::click()),
                            )
                            .on_hover_cursor(egui::CursorIcon::PointingHand)
                            .clicked()
                        {
                            for m in &provider_models {
                                let key = (m.provider.clone(), m.id.clone());
                                if all_selected {
                                    self.settings_view.models_selected.remove(&key);
                                } else {
                                    self.settings_view.models_selected.insert(key);
                                }
                            }
                        }
                    });
                }
            });
            ui.add_space(6.0);
            card(ui, |ui| {
                for (index, model) in provider_models.iter().enumerate() {
                    if index > 0 {
                        separator(ui);
                    }
                    let target = self.build_model_edit_target(model);
                    let hidden_key = format!("{}/{}", model.provider, model.id);
                    let was_visible = !self.prefs.hidden_models.contains(&hidden_key);
                    let mut visible = was_visible;
                    let sel_key = (model.provider.clone(), model.id.clone());
                    let is_selected = self.settings_view.models_selected.contains(&sel_key);
                    let mut selected = is_selected;
                    let select_mode = self.settings_view.models_select_mode;
                    let edit_clicked = model_settings_row(
                        ui,
                        &target,
                        &display,
                        select_mode,
                        &mut selected,
                        &mut visible,
                    );
                    if selected != is_selected {
                        if selected {
                            self.settings_view.models_selected.insert(sel_key);
                        } else {
                            self.settings_view.models_selected.remove(&sel_key);
                        }
                    }
                    if visible != was_visible {
                        if visible {
                            self.prefs.hidden_models.remove(&hidden_key);
                        } else {
                            self.prefs.hidden_models.insert(hidden_key);
                        }
                    }
                    if edit_clicked {
                        open_single = Some(model.clone());
                    }
                }
            });
        }

        if let Some(model) = open_single {
            let target = self.build_model_edit_target(&model);
            self.settings_view.model_editor =
                Some(super::model_editor::ModelEditDraft::single(target));
        } else if open_multi {
            let targets: Vec<_> = models
                .iter()
                .filter(|m| {
                    self.settings_view
                        .models_selected
                        .contains(&(m.provider.clone(), m.id.clone()))
                })
                .map(|m| self.build_model_edit_target(m))
                .collect();
            if !targets.is_empty() {
                self.settings_view.model_editor =
                    Some(super::model_editor::ModelEditDraft::multi(targets));
            }
        }
    }

    pub(super) fn build_model_edit_target(
        &self,
        model: &protocol::Model,
    ) -> super::model_editor::ModelEditTarget {
        let key = (model.provider.clone(), model.id.clone());
        let catalog_entry = self
            .catalog
            .models
            .iter()
            .find(|c| c.model.provider == model.provider && c.model.id == model.id);
        let custom_entry = self
            .settings_view
            .providers
            .custom_providers
            .iter()
            .find(|p| p.id == model.provider)
            .and_then(|p| p.models.iter().find(|m| m.id == model.id));
        let ov = self.settings_view.providers.model_overrides.get(&key);

        let name = ov.and_then(|o| o.name.clone()).unwrap_or_else(|| {
            if model.name.is_empty() {
                model.id.clone()
            } else {
                model.name.clone()
            }
        });
        let vision = ov
            .and_then(|o| o.vision)
            .or_else(|| catalog_entry.map(|c| c.vision))
            .or_else(|| custom_entry.map(|c| c.vision))
            .unwrap_or(false);
        let reasoning = ov
            .and_then(|o| o.reasoning)
            .or_else(|| catalog_entry.map(|c| c.reasoning))
            .or_else(|| custom_entry.map(|c| c.reasoning))
            .unwrap_or(false);
        let context_window = ov
            .and_then(|o| o.context_window)
            .or_else(|| (model.context_window > 0).then_some(model.context_window))
            .or_else(|| custom_entry.map(|c| c.context_window as u64))
            .unwrap_or(128_000);
        let max_tokens = ov
            .and_then(|o| o.max_tokens)
            .or_else(|| catalog_entry.map(|c| c.max_tokens).filter(|&v| v > 0))
            .or_else(|| custom_entry.map(|c| c.max_tokens as u64).filter(|&v| v > 0))
            .unwrap_or(16_384);
        let input_cost = ov.and_then(|o| o.input_cost).unwrap_or(model.input_cost);
        let output_cost = ov.and_then(|o| o.output_cost).unwrap_or(model.output_cost);
        let cache_read_cost = ov
            .and_then(|o| o.cache_read_cost)
            .or_else(|| catalog_entry.map(|c| c.cache_read_cost))
            .unwrap_or(0.0);
        let cache_write_cost = ov
            .and_then(|o| o.cache_write_cost)
            .or_else(|| catalog_entry.map(|c| c.cache_write_cost))
            .unwrap_or(0.0);
        let thinking_format = ov
            .and_then(|o| o.thinking_format.clone())
            .or_else(|| catalog_entry.and_then(|c| c.thinking_format.clone()));
        let thinking_level = self
            .settings_view
            .providers
            .model_thinking_levels
            .get(&key)
            .cloned();
        let fallback = self
            .settings_view
            .providers
            .model_fallbacks
            .get(&key)
            .cloned();
        let has_customizations = self
            .settings_view
            .providers
            .has_model_customizations(&model.provider, &model.id);

        super::model_editor::ModelEditTarget {
            provider: model.provider.clone(),
            id: model.id.clone(),
            name,
            vision,
            reasoning,
            context_window,
            max_tokens,
            input_cost,
            output_cost,
            cache_read_cost,
            cache_write_cost,
            thinking_format,
            thinking_level,
            fallback,
            has_customizations,
        }
    }
}

fn model_settings_row(
    ui: &mut egui::Ui,
    target: &super::model_editor::ModelEditTarget,
    provider_display: &str,
    select_mode: bool,
    selected: &mut bool,
    visible: &mut bool,
) -> bool {
    let colors = theme::palette(ui.ctx());
    let mut edit_clicked = false;
    let fill = if select_mode && *selected {
        colors.accent_bg.gamma_multiply(0.35)
    } else {
        Color32::TRANSPARENT
    };
    egui::Frame::new()
        .fill(fill)
        .inner_margin(egui::Margin::symmetric(18, 12))
        .show(ui, |ui| {
            let width = ui.available_width();
            let controls = (width * 0.44).clamp(200.0, 290.0);
            let origin = ui.cursor().min;
            let check_w = if select_mode { 26.0 } else { 0.0 };
            let icon_w = 24.0;
            let left_offset = check_w + icon_w;
            let label_width = (width - controls - left_offset - 12.0).max(80.0);

            let mut labels = ui.new_child(
                egui::UiBuilder::new()
                    .id_salt((&target.provider, &target.id, "model-row-label"))
                    .max_rect(egui::Rect::from_min_size(
                        origin + Vec2::new(left_offset, 0.0),
                        Vec2::new(label_width, 30.0),
                    ))
                    .layout(Layout::top_down(Align::LEFT)),
            );
            labels.set_width(label_width);
            labels.spacing_mut().item_spacing.y = 3.0;
            labels.horizontal_wrapped(|ui| {
                ui.spacing_mut().item_spacing = egui::vec2(6.0, 3.0);
                ui.label(RichText::new(&target.name).size(13.0).color(colors.text));
                if target.has_customizations {
                    row_pill(ui, "Customized", colors.accent_bg, colors.accent);
                }
            });
            labels.add(
                egui::Label::new(
                    RichText::new(format!("{provider_display} · {}", target.id))
                        .size(11.5)
                        .color(colors.muted),
                )
                .wrap(),
            );
            if target.vision || target.reasoning || target.fallback.is_some() {
                labels.horizontal_wrapped(|ui| {
                    ui.spacing_mut().item_spacing = egui::vec2(5.0, 4.0);
                    if target.vision {
                        row_pill(ui, "Vision", colors.surface, colors.muted);
                    }
                    if target.reasoning {
                        row_pill(ui, "Reasoning", colors.surface, colors.muted);
                    }
                    if let Some(fb) = &target.fallback {
                        let short = fb.split('/').next_back().unwrap_or(fb);
                        row_pill(
                            ui,
                            &format!("Fallback → {short}"),
                            colors.amber.gamma_multiply(0.14),
                            colors.amber,
                        );
                    }
                });
            }

            let height = labels.min_rect().height().max(theme::CONTROL_HEIGHT);
            if select_mode {
                let mut check_ui = ui.new_child(
                    egui::UiBuilder::new()
                        .id_salt((&target.provider, &target.id, "model-row-select"))
                        .max_rect(egui::Rect::from_min_size(
                            egui::pos2(origin.x, origin.y),
                            Vec2::new(check_w, height),
                        ))
                        .layout(Layout::left_to_right(Align::Center)),
                );
                check_ui.checkbox(selected, "");
            }
            crate::icons::draw_provider(
                ui,
                egui::Rect::from_center_size(
                    egui::pos2(origin.x + check_w + 8.0, origin.y + height * 0.5),
                    Vec2::splat(16.0),
                ),
                &target.provider,
            );

            let mut right = ui.new_child(
                egui::UiBuilder::new()
                    .id_salt((&target.provider, &target.id, "model-row-controls"))
                    .max_rect(egui::Rect::from_min_size(
                        egui::pos2(origin.x + width - controls, origin.y),
                        Vec2::new(controls, height),
                    ))
                    .layout(Layout::right_to_left(Align::Center)),
            );
            right.set_width(controls);
            right.set_height(height);
            right.spacing_mut().item_spacing.x = 8.0;

            toggle(
                &mut right,
                visible,
                &format!("Show {} in chat model picker", target.name),
            );
            if right
                .add(
                    egui::Button::new(RichText::new("Edit").size(12.0).color(colors.text))
                        .fill(colors.surface)
                        .stroke(Stroke::new(1.0_f32, colors.border))
                        .corner_radius(7)
                        .min_size(Vec2::new(48.0, 26.0)),
                )
                .on_hover_cursor(egui::CursorIcon::PointingHand)
                .on_hover_text("Edit context window, capabilities, fallback, and pricing")
                .clicked()
            {
                edit_clicked = true;
            }
            let ctx_label = super::model_editor::format_tokens(target.context_window.max(4_096));
            egui::Frame::new()
                .fill(colors.bg)
                .stroke(Stroke::new(1.0_f32, colors.border))
                .corner_radius(6)
                .inner_margin(egui::Margin::symmetric(7, 3))
                .show(&mut right, |ui| {
                    ui.label(RichText::new(&ctx_label).size(11.0).color(colors.muted))
                        .on_hover_text(format!(
                            "Context window ({} tokens). Click Edit to change.",
                            target.context_window.max(4_096)
                        ));
                });
            let price_in = super::model_editor::format_price(target.input_cost);
            let price_out = super::model_editor::format_price(target.output_cost);
            right
                .label(theme::small(&right, format!("${price_in} / ${price_out}")))
                .on_hover_text("Configured input / output price per million tokens");

            ui.allocate_exact_size(Vec2::new(width, height), Sense::hover());
        });
    edit_clicked
}

fn row_pill(ui: &mut egui::Ui, text: &str, fill: Color32, text_color: Color32) {
    let colors = theme::palette(ui.ctx());
    egui::Frame::new()
        .fill(fill)
        .stroke(Stroke::new(1.0_f32, colors.border.gamma_multiply(0.6)))
        .corner_radius(5)
        .inner_margin(egui::Margin::symmetric(6, 1))
        .show(ui, |ui| {
            ui.label(RichText::new(text).size(10.5).color(text_color));
        });
}

fn command() -> &'static str {
    if cfg!(target_os = "macos") {
        "⌘"
    } else {
        "Ctrl"
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn texts(shape: &egui::Shape, out: &mut Vec<(String, egui::Rect)>) {
        match shape {
            egui::Shape::Text(t) => out.push((
                t.galley.job.text.clone(),
                egui::Rect::from_min_size(t.pos, t.galley.size()),
            )),
            egui::Shape::Vec(v) => {
                for s in v {
                    texts(s, out);
                }
            }
            _ => {}
        }
    }
    #[test]
    fn auto_classifier_picker_changes_and_cancels_without_interrupting_work() {
        for mode in [theme::Theme::Light, theme::Theme::Dark] {
            for (width, zoom) in [(520.0, 1.25), (900.0, 1.0)] {
                let ctx = egui::Context::default();
                let mut app = Pui::new(
                    &eframe::CreationContext::_new_kittest(ctx.clone()),
                    Options {
                        demo: true,
                        theme: Some(mode),
                        ..Default::default()
                    },
                );
                ctx.set_zoom_factor(zoom);
                app.state.busy = true;
                app.draft = "keep my draft".into();
                app.state.permission_classifier = Some("google/flash".into());
                app.state.permission_classifier_models = vec![protocol::Model {
                    id: "flash".into(),
                    name: "Flash".into(),
                    provider: "google".into(),
                    context_window: 32000,
                    input_cost: 0.1,
                    output_cost: 0.4,
                }];
                let frame = |app: &mut Pui, events| {
                    ctx.run(
                        egui::RawInput {
                            screen_rect: Some(egui::Rect::from_min_size(
                                egui::Pos2::ZERO,
                                Vec2::new(width, 1100.0),
                            )),
                            events,
                            ..Default::default()
                        },
                        |ctx| {
                            egui::CentralPanel::default()
                                .show(ctx, |ui| app.settings_permissions(ui));
                        },
                    )
                };
                fn visible_text(shape: &egui::Shape, out: &mut Vec<(String, egui::Rect)>) {
                    match shape {
                        egui::Shape::Text(t) => out.push((
                            t.galley.job.text.clone(),
                            t.galley.rect.translate(t.pos.to_vec2()),
                        )),
                        egui::Shape::Vec(shapes) => {
                            shapes.iter().for_each(|s| visible_text(s, out))
                        }
                        _ => {}
                    }
                }
                let labels = |output: egui::FullOutput| {
                    let mut labels = Vec::new();
                    for shape in output.shapes {
                        visible_text(&shape.shape, &mut labels);
                    }
                    labels
                };
                let click = |app: &mut Pui, pos| {
                    for pressed in [true, false] {
                        frame(
                            app,
                            vec![
                                egui::Event::PointerMoved(pos),
                                egui::Event::PointerButton {
                                    pos,
                                    button: egui::PointerButton::Primary,
                                    pressed,
                                    modifiers: egui::Modifiers::NONE,
                                },
                            ],
                        );
                    }
                };
                frame(&mut app, vec![]);
                let text = labels(frame(&mut app, vec![]));
                let automatic = text
                    .iter()
                    .find(|(t, _)| t == "Automatic · cheapest available")
                    .expect("Automatic classifier choice")
                    .1;
                assert!(automatic.right() <= width + 0.1);
                let actual = ctx
                    .data(|d| d.get_temp::<egui::Rect>(egui::Id::new("classifier-rect")))
                    .unwrap();
                click(&mut app, actual.center());
                assert!(
                    app.settings_view.classifier_open,
                    "did not open: {actual:?} label {automatic:?}"
                );
                frame(&mut app, vec![]);
                let text = labels(frame(&mut app, vec![]));
                let flash = text
                    .iter()
                    .find(|(t, _)| t == "Flash")
                    .unwrap_or_else(|| {
                        panic!("Live eligible classifier at {width}/{zoom}: {text:?}")
                    })
                    .1;
                assert!(flash.right() <= width + 0.1);
                click(&mut app, flash.center());
                assert_eq!(
                    app.prefs.settings.permission_classifier.as_deref(),
                    Some("google/flash")
                );
                let text = labels(frame(&mut app, vec![]));
                let selected = text.iter().find(|(t, _)| t == "Flash · google").unwrap().1;
                let actual = ctx
                    .data(|d| d.get_temp::<egui::Rect>(egui::Id::new("classifier-rect")))
                    .unwrap();
                assert!(selected.intersects(actual));
                click(&mut app, actual.center());
                frame(
                    &mut app,
                    vec![egui::Event::Key {
                        key: egui::Key::Escape,
                        physical_key: None,
                        pressed: true,
                        repeat: false,
                        modifiers: egui::Modifiers::NONE,
                    }],
                );
                assert_eq!(
                    app.prefs.settings.permission_classifier.as_deref(),
                    Some("google/flash")
                );
                assert!(!app.settings_view.classifier_open);
                // Disconnected overrides stay visible, and can return to Automatic.
                app.state.permission_classifier_models.clear();
                app.state.permission_classifier = None;
                let text = labels(frame(&mut app, vec![]));
                assert!(text.iter().any(|(t, _)| t.starts_with("Unavailable")));
                let selected = text.iter().find(|(t, _)| t == "google/flash").unwrap().1;
                let actual = ctx
                    .data(|d| d.get_temp::<egui::Rect>(egui::Id::new("classifier-rect")))
                    .unwrap();
                assert!(selected.intersects(actual));
                click(&mut app, actual.center());
                frame(&mut app, vec![]);
                let text = labels(frame(&mut app, vec![]));
                let automatic = text
                    .iter()
                    .find(|(t, _)| t == "Automatic · cheapest available")
                    .unwrap()
                    .1;
                let actual = ctx
                    .data(|d| {
                        d.get_temp::<egui::Rect>(egui::Id::new(("classifier", "automatic-rect")))
                    })
                    .unwrap();
                assert!(
                    automatic.intersects(actual),
                    "Automatic label vs hit rect at {width}/{zoom}: {automatic:?} vs {actual:?}. Labels: {text:?}"
                );
                click(&mut app, actual.center());
                assert!(
                    app.prefs.settings.permission_classifier.is_none(),
                    "Automatic not selected at {width}/{zoom}: {actual:?}, {automatic:?}"
                );
                assert!(app.state.busy);
                assert_eq!(app.draft, "keep my draft");
                assert!(app.state.dialogs.is_empty());
            }
        }
    }
    #[test]
    fn settings_use_the_app_catalog_without_any_chat_connection() {
        let ctx = egui::Context::default();
        let mut app = Pui::new(
            &eframe::CreationContext::_new_kittest(ctx.clone()),
            Options {
                demo: true,
                ..Default::default()
            },
        );
        // A released/idle chat: no client, no chat catalog.
        app.client = None;
        app.connected = false;
        app.state.models.clear();
        app.state.permission_classifier_models.clear();
        app.state.permission_classifier = None;
        app.catalog.models = crate::app::catalog::fixture(&[
            ("anthropic", "claude-opus-5-5", 5.0, 25.0),
            ("Nvidia", "glm5", 0.5, 1.0),
        ]);
        app.catalog.loaded = true;
        let render = |app: &mut Pui, page: fn(&mut Pui, &mut egui::Ui)| {
            let output = ctx.run(Default::default(), |ctx| {
                egui::CentralPanel::default().show(ctx, |ui| page(app, ui));
            });
            let mut text = Vec::new();
            fn walk(shape: &egui::Shape, out: &mut Vec<String>) {
                match shape {
                    egui::Shape::Text(t) => out.push(t.galley.job.text.clone()),
                    egui::Shape::Vec(v) => v.iter().for_each(|s| walk(s, out)),
                    _ => {}
                }
            }
            for shape in output.shapes {
                walk(&shape.shape, &mut text);
            }
            text
        };
        let models = render(&mut app, |app, ui| app.settings_models(ui));
        assert!(models.iter().any(|t| t == "glm5"), "{models:?}");
        assert!(models.iter().any(|t| t == "Default model"));
        assert!(!models.iter().any(|t| t.contains("Connect to Pi")));
        let permissions = render(&mut app, |app, ui| app.settings_permissions(ui));
        assert!(
            permissions.iter().any(|t| t == "Nvidia/glm5"),
            "cheapest priced visible model resolves without a chat: {permissions:?}"
        );
    }
    #[test]
    fn resources_page_is_searchable_and_keeps_the_running_chat_intact() {
        let ctx = egui::Context::default();
        let mut app = Pui::new(
            &eframe::CreationContext::_new_kittest(ctx.clone()),
            Options {
                demo: true,
                demo_settings: true,
                ..Default::default()
            },
        );
        app.settings_view.search = "mcp".into();
        app.state.busy = true;
        app.draft = "unsent draft".into();
        let mut frame = eframe::Frame::_new_kittest();
        for _ in 0..2 {
            let output = ctx.run(
                egui::RawInput {
                    screen_rect: Some(egui::Rect::from_min_size(
                        egui::Pos2::ZERO,
                        Vec2::new(1480.0, 960.0),
                    )),
                    ..Default::default()
                },
                |ctx| eframe::App::update(&mut app, ctx, &mut frame),
            );
            let mut text = vec![];
            for shape in output.shapes {
                texts(&shape.shape, &mut text);
            }
            assert_eq!(app.settings_view.page, Page::Resources);
            assert!(text.iter().any(|(text, _)| text == "code-review"));
            assert!(text.iter().any(|(text, _)| text.contains("MCP servers")));
            assert!(text.iter().any(|(text, _)| text.contains("Demo fixtures")));
        }
        assert!(app.state.busy);
        assert_eq!(app.draft, "unsent draft");
        for query in ["skills", "install", "extensions", "templates"] {
            assert!(Page::Resources.matches(query));
        }
    }
    #[test]
    fn theme_preferences_default_to_dark_and_roundtrip() {
        let mut prefs: Preferences = serde_json::from_str("{}").unwrap();
        assert_eq!(prefs.theme, theme::Theme::Dark);
        assert_eq!(prefs.color_styles, theme::Styles::default());
        assert_eq!(prefs.color_styles.dark, theme::DarkStyle::Neutral);
        assert_eq!(prefs.color_styles.light, theme::LightStyle::Neutral);
        prefs.color_styles = theme::Styles {
            dark: theme::DarkStyle::Forest,
            light: theme::LightStyle::Rose,
        };
        for mode in [theme::Theme::Light, theme::Theme::Dark] {
            prefs.theme = mode;
            let saved = serde_json::to_string(&prefs).unwrap();
            let restored: Preferences = serde_json::from_str(&saved).unwrap();
            assert_eq!(restored.theme, mode);
            assert_eq!(restored.color_styles, prefs.color_styles);
        }
    }

    #[test]
    fn appearance_clicks_switch_the_theme_without_interrupting_work() {
        let ctx = egui::Context::default();
        let mut app = Pui::new(
            &eframe::CreationContext::_new_kittest(ctx.clone()),
            Options {
                demo: true,
                demo_settings: true,
                theme: Some(theme::Theme::Dark),
                ..Default::default()
            },
        );
        app.settings_view.search = "light".into();
        app.state.busy = true;
        app.draft = "keep this draft".into();
        ctx.set_zoom_factor(1.1);
        let mut window = eframe::Frame::_new_kittest();
        let mut frame = |app: &mut Pui, events| {
            ctx.run(
                egui::RawInput {
                    screen_rect: Some(egui::Rect::from_min_size(
                        egui::Pos2::ZERO,
                        Vec2::new(1480.0, 960.0),
                    )),
                    events,
                    ..Default::default()
                },
                |ctx| eframe::App::update(app, ctx, &mut window),
            )
        };
        frame(&mut app, vec![]);
        for (label, mode) in [("Light", theme::Theme::Light), ("Dark", theme::Theme::Dark)] {
            let output = frame(&mut app, vec![]);
            let mut text = vec![];
            for shape in output.shapes {
                texts(&shape.shape, &mut text);
            }
            let pos = text
                .iter()
                .find(|(text, _)| text == label)
                .unwrap()
                .1
                .center();
            for pressed in [true, false] {
                frame(
                    &mut app,
                    vec![
                        egui::Event::PointerMoved(pos),
                        egui::Event::PointerButton {
                            pos,
                            button: egui::PointerButton::Primary,
                            pressed,
                            modifiers: egui::Modifiers::NONE,
                        },
                    ],
                );
            }
            frame(&mut app, vec![]);
            assert_eq!(app.settings_view.page, Page::Appearance);
            assert_eq!(app.prefs.theme, mode);
            assert_eq!(ctx.style().visuals.dark_mode, mode == theme::Theme::Dark);
            assert_eq!(theme::palette(&ctx).settings_bg, mode.palette().settings_bg);
            assert!(app.state.busy);
            assert_eq!(app.draft, "keep this draft");
            assert_eq!(ctx.zoom_factor(), 1.1);
        }
    }

    #[test]
    fn color_style_cards_preview_and_remember_independent_mode_choices() {
        let ctx = egui::Context::default();
        let mut app = Pui::new(
            &eframe::CreationContext::_new_kittest(ctx.clone()),
            Options {
                demo: true,
                demo_settings: true,
                ..Default::default()
            },
        );
        app.settings_view.search = "forest".into();
        app.state.busy = true;
        app.draft = "unsent draft".into();
        let mut window = eframe::Frame::_new_kittest();
        let mut frame = |app: &mut Pui, events| {
            ctx.run(
                egui::RawInput {
                    screen_rect: Some(egui::Rect::from_min_size(
                        egui::Pos2::ZERO,
                        Vec2::new(1480.0, 1100.0),
                    )),
                    events,
                    ..Default::default()
                },
                |ctx| eframe::App::update(app, ctx, &mut window),
            )
        };
        frame(&mut app, vec![]);
        for (label, mode, expected_dark, expected_light) in [
            (
                "Forest",
                theme::Theme::Dark,
                theme::DarkStyle::Forest,
                theme::LightStyle::Neutral,
            ),
            (
                "Light",
                theme::Theme::Light,
                theme::DarkStyle::Forest,
                theme::LightStyle::Neutral,
            ),
            (
                "Rose",
                theme::Theme::Light,
                theme::DarkStyle::Forest,
                theme::LightStyle::Rose,
            ),
            (
                "Dark",
                theme::Theme::Dark,
                theme::DarkStyle::Forest,
                theme::LightStyle::Rose,
            ),
            (
                "Light",
                theme::Theme::Light,
                theme::DarkStyle::Forest,
                theme::LightStyle::Rose,
            ),
            (
                "Classic",
                theme::Theme::Light,
                theme::DarkStyle::Forest,
                theme::LightStyle::Neutral,
            ),
            (
                "Dark",
                theme::Theme::Dark,
                theme::DarkStyle::Forest,
                theme::LightStyle::Neutral,
            ),
            (
                "Classic",
                theme::Theme::Dark,
                theme::DarkStyle::Neutral,
                theme::LightStyle::Neutral,
            ),
        ] {
            let output = frame(&mut app, vec![]);
            let mut text = Vec::new();
            for shape in output.shapes {
                texts(&shape.shape, &mut text);
            }
            let (visible, hidden) = match app.prefs.theme {
                theme::Theme::Dark => (["Midnight", "Forest", "Plum"], ["Ivory", "Sage", "Rose"]),
                theme::Theme::Light => (["Ivory", "Sage", "Rose"], ["Midnight", "Forest", "Plum"]),
            };
            for name in visible {
                assert!(
                    text.iter().any(|(label, _)| label == name),
                    "Missing theme {name}"
                );
            }
            for name in hidden {
                assert!(
                    !text.iter().any(|(label, _)| label == name),
                    "Other mode's theme {name} should be hidden"
                );
            }
            assert_eq!(
                text.iter().filter(|(label, _)| label == "Classic").count(),
                1
            );
            let pos = text
                .iter()
                .find(|(text, _)| text == label)
                .unwrap()
                .1
                .center();
            for pressed in [true, false] {
                frame(
                    &mut app,
                    vec![
                        egui::Event::PointerMoved(pos),
                        egui::Event::PointerButton {
                            pos,
                            button: egui::PointerButton::Primary,
                            pressed,
                            modifiers: egui::Modifiers::NONE,
                        },
                    ],
                );
            }
            frame(&mut app, vec![]);
            assert_eq!(app.settings_view.page, Page::Appearance);
            assert_eq!(app.prefs.theme, mode);
            assert_eq!(
                app.prefs.color_styles,
                theme::Styles {
                    dark: expected_dark,
                    light: expected_light
                }
            );
            assert_eq!(
                theme::palette(&ctx).accent,
                app.prefs.color_styles.palette(mode).accent
            );
            assert_eq!(
                ctx.style().visuals.hyperlink_color,
                app.prefs.color_styles.palette(mode).accent
            );
            assert!(app.state.busy);
            assert_eq!(app.draft, "unsent draft");
        }
    }

    #[test]
    fn controls_share_a_right_edge_despite_different_label_lengths() {
        for width in [520.0, 860.0] {
            let ctx = egui::Context::default();
            let mut controls = Vec::new();
            let _ = ctx.run(egui::RawInput { screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(width, 800.0))), ..Default::default() }, |ctx| {
                egui::CentralPanel::default().show(ctx, |ui| {
                    card(ui, |ui| {
                        for (title, description) in [("Short", ""), ("A long setting title that wraps on narrow windows", "An intentionally long description that should wrap onto more than one line without shifting any switches."), ("Another", "A short description.")] {
                            row(ui, title, description, |ui| { controls.push(ui.allocate_exact_size(egui::vec2(34.0, 20.0), Sense::click()).0); });
                        }
                    });
                });
            });
            assert_eq!(controls.len(), 3);
            for control in &controls[1..] {
                assert!(
                    (control.right() - controls[0].right()).abs() < 0.1,
                    "Controls drifted at width {width}: {controls:?}"
                );
            }
            assert!(controls.windows(2).all(|r| r[0].bottom() < r[1].top()));
        }
    }
    #[test]
    fn settings_is_a_full_page_and_navigation_does_not_abort_work() {
        let ctx = egui::Context::default();
        let mut app = Pui::new(
            &eframe::CreationContext::_new_kittest(ctx.clone()),
            Options {
                demo: true,
                demo_settings: true,
                ..Default::default()
            },
        );
        app.state.busy = true;
        app.draft = "keep this draft".into();
        let mut window = eframe::Frame::_new_kittest();
        let mut frame = |app: &mut Pui, events| {
            ctx.run(
                egui::RawInput {
                    screen_rect: Some(egui::Rect::from_min_size(
                        egui::Pos2::ZERO,
                        Vec2::new(1480.0, 960.0),
                    )),
                    events,
                    ..Default::default()
                },
                |ctx| eframe::App::update(app, ctx, &mut window),
            )
        };
        frame(&mut app, vec![]);
        let output = frame(&mut app, vec![]);
        let mut text = vec![];
        for shape in output.shapes {
            texts(&shape.shape, &mut text);
        }
        assert!(text.iter().any(|(t, _)| t == "Chat"));
        assert!(!text.iter().any(|(t, _)| t == "Back to app"));
        assert!(text.iter().any(|(t, _)| t == "Auto compact at"));
        assert!(!text.iter().any(|(t, _)| t.contains("Stop active agents")));
        assert!(!text.iter().any(|(t, _)| t.contains("Message Pi")));
        let general = text
            .iter()
            .find(|(t, r)| t == "General" && r.left() > 244.0)
            .unwrap()
            .1;
        assert!(general.top() > 60.0);
        let notification = text
            .iter()
            .find(|(t, _)| t == "Notifications")
            .unwrap()
            .1
            .center();
        for pressed in [true, false] {
            frame(
                &mut app,
                vec![
                    egui::Event::PointerMoved(notification),
                    egui::Event::PointerButton {
                        pos: notification,
                        button: egui::PointerButton::Primary,
                        pressed,
                        modifiers: egui::Modifiers::NONE,
                    },
                ],
            );
        }
        assert_eq!(app.settings_view.page, Page::Notifications);
        app.settings_view.search = "compact".into();
        frame(&mut app, vec![]);
        assert_eq!(app.settings_view.page, Page::General);
        frame(
            &mut app,
            vec![egui::Event::Key {
                key: egui::Key::Escape,
                physical_key: None,
                pressed: true,
                repeat: false,
                modifiers: egui::Modifiers::NONE,
            }],
        );
        assert!(!app.settings_open);
        assert!(app.state.busy);
        assert_eq!(app.draft, "keep this draft");
    }

    #[test]
    fn models_page_opens_single_and_multi_edit_dialogs_without_inline_context_input() {
        let ctx = egui::Context::default();
        let mut app = Pui::new(
            &eframe::CreationContext::_new_kittest(ctx.clone()),
            Options {
                demo: true,
                demo_settings: true,
                ..Default::default()
            },
        );
        app.settings_view.page = Page::Models;
        app.settings_view
            .providers
            .set_configured_providers_for_test(&["anthropic"], &["Nvidia"]);
        app.catalog.models = catalog::fixture(&[
            ("Nvidia", "z-ai/glm-5.3-flash", 0.0, 0.0),
            ("anthropic", "claude-sonnet-4-6", 3.0, 15.0),
        ]);
        app.catalog.models[0].model.context_window = 1_000_000;
        app.catalog.models[0].vision = true;
        app.catalog.models[0].reasoning = true;
        app.catalog.loaded = true;

        let mut window = eframe::Frame::_new_kittest();
        let mut frame = |app: &mut Pui, events| {
            ctx.run(
                egui::RawInput {
                    screen_rect: Some(egui::Rect::from_min_size(
                        egui::Pos2::ZERO,
                        Vec2::new(1280.0, 900.0),
                    )),
                    events,
                    ..Default::default()
                },
                |ctx| eframe::App::update(app, ctx, &mut window),
            )
        };
        let collect_labels = |output: &egui::FullOutput| {
            let mut out = vec![];
            for shape in &output.shapes {
                texts(&shape.shape, &mut out);
            }
            out
        };

        let output = frame(&mut app, vec![]);
        let labels = collect_labels(&output);
        assert!(
            labels.iter().any(|(t, _)| t == "1M"),
            "context chip shown on row"
        );
        assert!(
            labels.iter().any(|(t, _)| t == "Vision"),
            "vision pill shown on row"
        );
        assert!(
            labels.iter().any(|(t, _)| t == "Reasoning"),
            "reasoning pill shown on row"
        );
        assert!(
            labels.iter().any(|(t, _)| t == "Edit"),
            "Edit button shown on row"
        );
        assert!(
            labels.iter().any(|(t, _)| t == "Select"),
            "Select toggle shown"
        );

        // Clicking Edit opens single-edit modal.
        let edit_pos = labels.iter().find(|(t, _)| t == "Edit").unwrap().1.center();
        for pressed in [true, false] {
            frame(
                &mut app,
                vec![
                    egui::Event::PointerMoved(edit_pos),
                    egui::Event::PointerButton {
                        pos: edit_pos,
                        button: egui::PointerButton::Primary,
                        pressed,
                        modifiers: egui::Modifiers::NONE,
                    },
                ],
            );
        }
        assert!(app.settings_view.model_editor.is_some());
        assert!(!app.settings_view.model_editor.as_ref().unwrap().is_multi());
        assert!(app.settings_view.navigation_blocked());

        // Escape closes the Edit Model dialog without closing Settings.
        frame(
            &mut app,
            vec![egui::Event::Key {
                key: egui::Key::Escape,
                physical_key: None,
                pressed: true,
                repeat: false,
                modifiers: egui::Modifiers::NONE,
            }],
        );
        assert!(app.settings_view.model_editor.is_none());
        assert!(app.settings_open);

        // Multi-select mode -> Edit 2 models opens multi-edit dialog.
        app.settings_view.models_select_mode = true;
        app.settings_view
            .models_selected
            .insert(("Nvidia".into(), "z-ai/glm-5.3-flash".into()));
        app.settings_view
            .models_selected
            .insert(("anthropic".into(), "claude-sonnet-4-6".into()));
        let output = frame(&mut app, vec![]);
        let labels = collect_labels(&output);
        let bulk_pos = labels
            .iter()
            .find(|(t, _)| t == "Edit 2 models")
            .expect("multi-edit action button")
            .1
            .center();
        for pressed in [true, false] {
            frame(
                &mut app,
                vec![
                    egui::Event::PointerMoved(bulk_pos),
                    egui::Event::PointerButton {
                        pos: bulk_pos,
                        button: egui::PointerButton::Primary,
                        pressed,
                        modifiers: egui::Modifiers::NONE,
                    },
                ],
            );
        }
        let editor = app
            .settings_view
            .model_editor
            .as_ref()
            .expect("multi-edit dialog opened");
        assert!(editor.is_multi());
        assert_eq!(editor.targets.len(), 2);
    }
}
