//! Full-window settings, independent of the conversation and its agent lifecycle.
use super::*;

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
}
impl Page {
    const ALL: [Self; 8] = [
        Self::General,
        Self::Appearance,
        Self::Notifications,
        Self::Shortcuts,
        Self::Permissions,
        Self::Models,
        Self::Providers,
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
        }
    }
    fn subtitle(self) -> &'static str {
        match self {
            Self::General => "Messages, context and agent runtime defaults.",
            Self::Appearance => "Choose a theme, workspace layout and interface scale.",
            Self::Notifications => "Choose how agents let you know their work is finished.",
            Self::Shortcuts => "Shortcuts available across Pui.",
            Self::Permissions => "Choose what your agent can do without asking.",
            Self::Models => "Models and thinking levels supplied by Pi.",
            Self::Providers => "Connected model providers, managed by Pi.",
            Self::Resources => "Manage skills, MCP servers, extensions and Pi packages.",
        }
    }
    fn keywords(self) -> &'static str {
        match self {
            Self::General => "steer queue messages enter context compact tokens worker concurrency",
            Self::Appearance => "layout sidebar inspector scale zoom reset dark light theme",
            Self::Notifications => {
                "sound completion alerts desktop macos permission notification test"
            }
            Self::Shortcuts => "keyboard shortcuts keys command control enter escape",
            Self::Permissions => "manual auto accept edits plan dont ask classifier safety tools",
            Self::Models => "model reasoning effort thinking pricing context provider",
            Self::Providers => "provider authentication login credentials catalog",
            Self::Resources => {
                "skills mcp servers extensions packages install remove prompts templates themes plugins"
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
        }
    }
    fn matches(self, query: &str) -> bool {
        format!("{} {}", self.title(), self.keywords())
            .to_lowercase()
            .contains(query)
    }
}
#[derive(Default)]
pub(in crate::app) struct View {
    page: Page,
    search: String,
    pub(in crate::app) resources: resources::Resources,
}

impl View {
    pub(in crate::app) fn select(&mut self, name: &str) {
        if let Some(page) = Page::ALL
            .into_iter()
            .find(|p| p.title().eq_ignore_ascii_case(name))
        {
            self.page = page;
            self.search.clear();
        }
    }
}

pub(super) fn section(ui: &mut egui::Ui, title: &str, description: &str) {
    let colors = crate::theme::palette(ui.ctx());
    ui.add_space(30.0);
    ui.label(RichText::new(title).size(13.0).color(colors.text));
    if !description.is_empty() {
        ui.label(RichText::new(description).size(12.0).color(colors.muted));
    }
    ui.add_space(4.0);
}
pub(super) fn card(ui: &mut egui::Ui, body: impl FnOnce(&mut egui::Ui)) {
    let colors = crate::theme::palette(ui.ctx());
    egui::Frame::new()
        .fill(colors.settings_card)
        .stroke(Stroke::new(1.0, colors.border))
        .corner_radius(14)
        .inner_margin(egui::Margin::ZERO)
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.spacing_mut().item_spacing.y = 0.0;
            body(ui);
        });
}
fn separator(ui: &mut egui::Ui) {
    let colors = crate::theme::palette(ui.ctx());
    let (rect, _) = ui.allocate_exact_size(Vec2::new(ui.available_width(), 1.0), Sense::hover());
    ui.painter().hline(
        rect.x_range(),
        rect.center().y,
        Stroke::new(1.0, colors.border.gamma_multiply(0.65)),
    );
}
pub(super) fn row(
    ui: &mut egui::Ui,
    title: &str,
    description: &str,
    body: impl FnOnce(&mut egui::Ui),
) {
    let colors = crate::theme::palette(ui.ctx());
    egui::Frame::new()
        .inner_margin(egui::Margin::symmetric(18, 15))
        .show(ui, |ui| {
            let width = ui.available_width();
            let controls = (width * 0.35).clamp(125.0, 240.0);
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
            let height = labels.min_rect().height().max(30.0);
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
pub(super) fn toggle(ui: &mut egui::Ui, value: &mut bool, title: &str) {
    let colors = theme::palette(ui.ctx());
    let (rect, response) = ui.allocate_exact_size(Vec2::new(34.0, 20.0), Sense::click());
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
    let (rect, response) =
        ui.allocate_exact_size(Vec2::new(ui.available_width(), 31.0), Sense::click());
    if selected || response.hovered() {
        ui.painter().rect_filled(
            rect,
            8,
            if selected {
                colors.hover
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
        colors.muted,
    );
    ui.painter().text(
        rect.left_center() + egui::vec2(34.0, 0.0),
        egui::Align2::LEFT_CENTER,
        page.title(),
        FontId::proportional(12.0),
        colors.text,
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
impl Pui {
    pub(in crate::app) fn session_settings(&mut self, ctx: &egui::Context) {
        let colors = crate::theme::palette(ctx);
        if !self.settings_open {
            return;
        }
        let before = self.prefs.settings.clone();
        let alerts_before = self.prefs.notifications;
        let theme_before = self.prefs.theme;

        if ctx.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::Escape)) {
            self.settings_open = false;
            self.focus_composer = true;
        }
        let query = self.settings_view.search.trim().to_lowercase();
        if !query.is_empty() && !self.settings_view.page.matches(&query) {
            if let Some(page) = Page::ALL.into_iter().find(|p| p.matches(&query)) {
                self.settings_view.page = page;
            }
        }
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
                ui.add_space(43.0);
                ui.horizontal(|ui| {
                    icons::inline(ui, Icon::ArrowLeft, colors.muted);
                    if ui
                        .add(
                            egui::Button::new(
                                RichText::new("Back to app").size(12.0).color(colors.text),
                            )
                            .frame(false),
                        )
                        .clicked()
                    {
                        self.settings_open = false;
                        self.focus_composer = true;
                    }
                });
                ui.add_space(15.0);
                egui::Frame::new()
                    .fill(colors.surface)
                    .corner_radius(10)
                    .inner_margin(egui::Margin::symmetric(10, 8))
                    .show(ui, |ui| {
                        ui.horizontal(|ui| {
                            icons::inline(ui, Icon::Search, colors.dim);
                            ui.add(
                                egui::TextEdit::singleline(&mut self.settings_view.search)
                                    .id_salt("settings-search")
                                    .hint_text("Search settings")
                                    .font(FontId::proportional(12.0))
                                    .frame(false)
                                    .desired_width((ui.available_width() - 4.0).max(50.0)),
                            );
                        });
                    });
                ui.add_space(13.0);
                for (group, pages) in [("App", &Page::ALL[..4]), ("Agent", &Page::ALL[4..])] {
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
                egui::ScrollArea::vertical()
                    .id_salt(("settings-content", self.settings_view.page))
                    .auto_shrink([false, false])
                    .show(ui, |ui| {
                        let width = (ui.available_width() - 64.0).clamp(320.0, 744.0);
                        let padding = ((ui.available_width() - width) / 2.0).max(24.0);
                        ui.horizontal(|ui| {
                            ui.add_space(padding);
                            ui.vertical(|ui| {
                                ui.set_width(width);
                                ui.add_space(84.0);
                                let page = self.settings_view.page;
                                ui.label(RichText::new(page.title()).size(23.0).color(colors.text));
                                ui.add_space(1.0);
                                ui.label(
                                    RichText::new(page.subtitle())
                                        .size(12.5)
                                        .color(colors.muted),
                                );
                                match page {
                                    Page::General => self.settings_general(ui),
                                    Page::Appearance => self.settings_appearance(ui, ctx),
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
                                }
                                ui.add_space(45.0);
                            });
                        });
                    });
            });
        self.propagate_notifications(ctx, alerts_before);
        if theme_before != self.prefs.theme {
            self.prefs.theme.apply(ctx);
            ctx.request_repaint();
        }
        self.propagate_settings(&before);
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
                    ui.selectable_value(
                        &mut self.prefs.settings.delivery,
                        DeliveryMode::Queue,
                        "Queue",
                    );
                    ui.selectable_value(
                        &mut self.prefs.settings.delivery,
                        DeliveryMode::Steer,
                        "Steer",
                    );
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
                        ui.visuals_mut().widgets.inactive.bg_fill = colors.border;
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
        section(ui, "Ultracode", "");
        card(ui, |ui| {
            row(
                ui,
                "Worker concurrency ceiling",
                "A limit, not a target team size. The parent chooses the fewest useful workers.",
                |ui| {
                    ui.add(egui::DragValue::new(&mut self.prefs.agent_concurrency).range(1..=16));
                },
            )
        });
    }
    fn settings_appearance(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        section(ui, "Theme", "");
        card(ui, |ui| {
            row(
                ui,
                "Color theme",
                "Choose a light or dark palette for this window. Your choice is saved automatically.",
                |ui| {
                    ui.selectable_value(&mut self.prefs.theme, theme::Theme::Light, "Light");
                    ui.selectable_value(&mut self.prefs.theme, theme::Theme::Dark, "Dark");
                },
            )
        });
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
                "Adjust text and controls throughout this window.",
                |ui| {
                    let mut zoom = ctx.zoom_factor();
                    ui.spacing_mut().slider_width = 110.0;
                    if ui
                        .add(
                            egui::Slider::new(&mut zoom, 0.8..=1.4)
                                .step_by(0.05)
                                .custom_formatter(|v, _| format!("{:.0}%", v * 100.0)),
                        )
                        .changed()
                    {
                        ctx.set_zoom_factor(zoom);
                    }
                },
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
                    "Play a short sound when an agent finishes successfully.",
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
                ("Search sessions", vec![cmd, "K"]),
                ("Toggle sidebar", vec![cmd, "B"]),
                ("Toggle inspector", vec![cmd, "I"]),
                ("New conversation", vec![cmd, "N"]),
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
                ("Stop agent".into(), vec!["Esc"]),
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
                "Back from settings",
                "Return to your conversation without interrupting work.",
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
        let colors = crate::theme::palette(ui.ctx());
        section(
            ui,
            "Tool access",
            "Permission checks include nested codemode calls. They are not an OS sandbox.",
        );
        ui.label(theme::small(ui, "Changes apply to subsequent tool calls. Commands already executing are not interrupted."));
        ui.add_space(6.0);
        card(ui, |ui| {
            ui.scope(|ui|row(ui,"Permission mode","Applied to this Pui owner and its workers; read-only worker restrictions always win.",|ui|{
            egui::ComboBox::from_id_salt("settings-permission-mode").selected_text(self.prefs.settings.permission.label()).width(140.0).show_ui(ui,|ui|{
                for mode in PermissionMode::ALL {if ui.selectable_label(self.prefs.settings.permission == mode,mode.label()).clicked(){self.choose_permission(mode);}}
            });
        }));
        });
        self.permission_confirmation(ui);
        ui.add_space(12.0);
        ui.label(
            RichText::new(self.prefs.settings.permission.description())
                .size(12.0)
                .color(colors.muted),
        );
        if self.prefs.settings.permission == PermissionMode::Auto {
            section(ui, "Auto classifier", "");
            card(ui, |ui| {
                row(
                    ui,
                    "Classifier model",
                    "The lowest-priced suitable model from the current provider, using configured prices.",
                    |ui| {
                        ui.add(
                            egui::Label::new(theme::small(
                                ui,
                                self.state
                                    .permission_classifier
                                    .as_deref()
                                    .unwrap_or("Unavailable — asks for approval"),
                            ))
                            .truncate(),
                        );
                    },
                );
                separator(ui);
                row(
                    ui,
                    "Classifier usage",
                    "Separate from the conversation model cost; current connection only.",
                    |ui| {
                        ui.label(theme::small(
                            ui,
                            format!("${:.5}", self.state.permission_cost),
                        ));
                    },
                );
            });
        }
        section(ui, "Safety", "");
        card(ui, |ui| {
            row(
                ui,
                "Trusted extensions",
                "Extensions can run their own code. Bypass skips Pui approvals, not OS permissions or read-only worker restrictions.",
                |ui| {
                    ui.label(theme::small(ui, "Tool-level controls"));
                },
            )
        });
    }
    fn settings_models(&mut self, ui: &mut egui::Ui) {
        section(
            ui,
            "Current conversation",
            "Model choices and supported thinking levels come from Pi.",
        );
        let models = self.state.models.clone();
        card(ui, |ui| {
            ui.scope(|ui|{
                row(ui,"Model","Changing the conversation model does not change Pi authentication.",|ui|{
                    let name=if self.state.model_name.is_empty(){self.state.model.as_str()}else{self.state.model_name.as_str()};
                    egui::ComboBox::from_id_salt("settings-model").selected_text(if name.is_empty(){"Loading models…"}else{name}).width(190.0).show_ui(ui,|ui|{
                        for model in &models{if ui.selectable_label(model.id==self.state.model&&model.provider==self.state.provider,format!("{} · {}",model.name,model.provider)).clicked(){self.state.levels.clear();self.send(json!({"type":"set_model","provider":model.provider,"modelId":model.id}));}}
                    });
                });separator(ui);
                row(ui,"Thinking effort","Only levels supported by the selected model are offered.",|ui|{
                    let levels=self.state.levels.clone();
                    egui::ComboBox::from_id_salt("settings-effort").selected_text(thinking_label(&self.state.thinking)).width(120.0).show_ui(ui,|ui|{for level in levels{if ui.selectable_label(level==self.state.thinking,thinking_label(&level)).clicked(){self.send(json!({"type":"set_thinking_level","level":level}));}}});
                });
            });
        });
        ui.add_space(8.0);
        ui.label(theme::small(ui, "An in-flight provider response keeps its original model; Pi applies your selection to subsequent requests."));
        section(ui, "Available models", "");
        if models.is_empty() {
            ui.label(theme::small(ui, "Connect to Pi to load the model catalog."));
            return;
        }
        card(ui, |ui| {
            for (index, model) in models.iter().enumerate() {
                if index > 0 {
                    separator(ui);
                }
                row(
                    ui,
                    &model.name,
                    &format!("{} · {}", model.provider, model.id),
                    |ui| {
                        ui.label(theme::small(
                            ui,
                            format!(
                                "{}k context · ${} / ${}",
                                model.context_window / 1000,
                                model.input_cost,
                                model.output_cost
                            ),
                        ))
                        .on_hover_text("Configured input / output price per million tokens");
                    },
                );
            }
        });
    }
    fn settings_providers(&self, ui: &mut egui::Ui) {
        section(
            ui,
            "Configured providers",
            "Pui uses Pi's existing authentication. Credentials never appear here.",
        );
        let mut providers = std::collections::BTreeMap::<&str, usize>::new();
        for model in &self.state.models {
            *providers.entry(&model.provider).or_default() += 1;
        }
        if providers.is_empty() {
            card(ui, |ui| {
                row(
                    ui,
                    "No catalog loaded",
                    "Connect to Pi in a workspace to discover providers.",
                    |_| {},
                )
            });
        } else {
            card(ui, |ui| {
                for (index, (provider, count)) in providers.into_iter().enumerate() {
                    if index > 0 {
                        separator(ui);
                    }
                    row(
                        ui,
                        provider,
                        &format!("{count} models available from Pi"),
                        |ui| {
                            ui.label(theme::small(
                                ui,
                                if provider == self.state.provider {
                                    "Current provider"
                                } else {
                                    "Available"
                                },
                            ));
                        },
                    );
                }
            });
        }
        section(ui, "Authentication", "");
        card(ui, |ui| {
            row(
                ui,
                "Manage sign-in with Pi",
                "Use Pi's /login in your terminal. Pui does not store API keys or change provider configuration.",
                |ui| {
                    ui.monospace("pi /login");
                },
            )
        });
    }
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
        for mode in [theme::Theme::Light, theme::Theme::Dark] {
            prefs.theme = mode;
            let saved = serde_json::to_string(&prefs).unwrap();
            let restored: Preferences = serde_json::from_str(&saved).unwrap();
            assert_eq!(restored.theme, mode);
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
        assert!(text.iter().any(|(t, _)| t == "Back to app"));
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
}
