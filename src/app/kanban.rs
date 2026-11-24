//! Read-only, live overview. Opening the board never switches or stops a chat.
use super::*;
use std::{collections::BTreeMap, time::Instant};

#[derive(Default)]
pub(super) struct Board {
    pub open: bool,
    project: Option<PathBuf>,
    roots_only: bool,
    observed: BTreeMap<String, (Status, Instant)>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Status {
    Working,
    Waiting,
    Idle,
    Error,
    Queued,
    Stopped,
}
impl Status {
    fn label(self) -> &'static str {
        match self {
            Self::Working => "Working",
            Self::Waiting => "Waiting for input",
            Self::Idle => "Idle",
            Self::Error => "Error",
            Self::Queued => "Queued",
            Self::Stopped => "Stopped",
        }
    }
    fn color(self, colors: &theme::Palette) -> Color32 {
        match self {
            Self::Working => colors.accent,
            Self::Waiting => colors.amber,
            Self::Idle => colors.green,
            Self::Error => colors.red,
            Self::Queued => colors.blue,
            Self::Stopped => colors.dim,
        }
    }
    pub(super) fn from_label(label: &str) -> Self {
        match label {
            "Idle" => Self::Idle,
            "Waiting for input" => Self::Waiting,
            "Queued" => Self::Queued,
            "Stopped" | "Saved" | "Saved · not connected" => Self::Stopped,
            "Owner unavailable" => Self::Error,
            _ if label.starts_with("Failed") => Self::Error,
            _ => Self::Working,
        }
    }
}

#[derive(Clone)]
pub(super) enum Target {
    Current,
    Agent(String),
    Session(Session),
    Task { task: String, run: Option<String> },
}
impl Target {
    fn key(&self) -> String {
        match self {
            Self::Current => "current".into(),
            Self::Agent(id) => format!("agent:{id}"),
            Self::Session(s) => format!("session:{}", s.path.display()),
            Self::Task { task, run } => format!("task:{task}:{}", run.as_deref().unwrap_or("")),
        }
    }
}

pub(super) struct AgentCard {
    pub target: Target,
    pub title: String,
    pub project: PathBuf,
    pub status: Status,
    pub activity: String,
    pub model: String,
    pub parent: Option<String>,
    pub action: &'static str,
    pub path: Option<PathBuf>,
}
impl AgentCard {
    pub(super) fn from_state(
        target: Target,
        title: String,
        project: PathBuf,
        label: &str,
        state: &State,
        parent: Option<String>,
    ) -> Self {
        let status = if !state.dialogs.is_empty() {
            Status::Waiting
        } else if state.error.is_some() && (state.ready() || label == "Owner unavailable") {
            Status::Error
        } else {
            Status::from_label(label)
        };
        let mut action = "Open session";
        let activity = if let Some(dialog) = state.dialogs.front() {
            if let Some(permission) = prompts::approval(dialog) {
                action = "Review approval";
                permission.title
            } else {
                action = "Answer question";
                questions::Request::parse(dialog)
                    .map(|r| r.summary().to_owned())
                    .unwrap_or_else(|| protocol::string(dialog, "title").to_owned())
            }
        } else if status == Status::Error {
            state.error.clone().unwrap_or_else(|| label.into())
        } else if status == Status::Working {
            state
                .messages
                .last()
                .and_then(|m| m.tools.iter().rev().find(|t| !t.finished))
                .map(|t| {
                    if !t.command.is_empty() {
                        format!("{} · {}", t.name, t.command)
                    } else if !t.path.is_empty() {
                        format!("{} · {}", t.name, t.path)
                    } else {
                        t.name.clone()
                    }
                })
                .unwrap_or_else(|| label.into())
        } else if status == Status::Idle {
            "Ready for the next message".into()
        } else {
            label.into()
        };
        Self {
            target,
            title: if title.trim().is_empty() {
                "New conversation".into()
            } else {
                title
            },
            project,
            status,
            activity: short(&activity, 220),
            model: if state.model_name.is_empty() {
                state.model.clone()
            } else {
                state.model_name.clone()
            },
            parent,
            action,
            path: state.session_file.as_ref().map(PathBuf::from),
        }
    }
}

impl Pui {
    pub(super) fn open_agents_board(&mut self) {
        if self.deletion.in_progress() {
            return;
        }
        self.automation.open = false;
        self.kanban.open = true;
        self.close_model_popovers();
        self.files.close();
        self.inspector_panel = false;
        if self.terminals.is_open() {
            // Hide the dock, never terminate its shell or running command.
            self.terminals.toggle(&self.ctx, &self.prefs.cwd, self.demo);
        }
        self.focus_composer = false;
        self.focus_search = false;
    }

    pub(super) fn board_cards(&self) -> Vec<AgentCard> {
        let mut cards = Vec::new();
        let current = !self.agents.active.is_empty();
        for node in self.agents.nodes.values() {
            let active = node.id == self.agents.active;
            let state = if active {
                &self.state
            } else if let Some(chat) = self.agents.parked.get(&node.id) {
                // Restored history is not a running agent. Keep runtime stops/failures visible.
                if node.stopped
                    && chat.client.is_none()
                    && chat.state.messages.is_empty()
                    && chat.state.error.is_none()
                    && !node.status.starts_with("Failed")
                {
                    continue;
                }
                &chat.state
            } else {
                continue;
            };
            let label = if active
                && !node.stopped
                && !node.status.starts_with("Failed")
                && !matches!(node.status.as_str(), "Queued" | "Preparing worktree")
            {
                self.run_label().0
            } else {
                &node.status
            };
            let mut card = AgentCard::from_state(
                Target::Agent(node.id.clone()),
                state.chat_title().unwrap_or_else(|| node.name.clone()),
                node.cwd.clone(),
                label,
                state,
                node.parent.as_ref().map(|id| {
                    self.agents
                        .nodes
                        .get(id)
                        .map(|n| n.name.clone())
                        .unwrap_or_else(|| id.clone())
                }),
            );
            card.path = node.path.clone().or(card.path);
            cards.push(card);
        }
        if !current
            && (self.demo
                || self.client.is_some()
                || self.connected
                || self.loading_chat
                || self.state.error.is_some())
        {
            cards.push(AgentCard::from_state(
                Target::Current,
                self.state.chat_title().unwrap_or_default(),
                self.prefs.cwd.clone(),
                self.run_label().0,
                &self.state,
                None,
            ));
        }
        self.task_board_cards(&mut cards);
        for live in self.sync.live_sessions() {
            if let Some(card) = cards.iter_mut().find(|c| {
                c.path
                    .as_deref()
                    .is_some_and(|p| sidebar_tree::same_path(p, &live.path))
            }) {
                // Parked mirrors do not receive full snapshots; registry status
                // is authoritative until their session is opened again.
                card.status = Status::from_label(&live.label);
                card.activity = format!("{} · another Pui window", live.label);
                continue;
            }
            let saved = self
                .sessions
                .iter()
                .find(|s| sidebar_tree::same_path(&s.path, &live.path));
            let project = if live.cwd.as_os_str().is_empty() {
                saved.map(|s| s.cwd.clone()).unwrap_or_default()
            } else {
                live.cwd
            };
            let title = if live.title.is_empty() {
                saved
                    .map(|s| s.title.clone())
                    .unwrap_or_else(|| "Live session".into())
            } else {
                live.title
            };
            let session = Session {
                path: live.path.clone(),
                cwd: project.clone(),
                project: project.clone(),
                title: title.clone(),
                created: saved.and_then(|s| s.created),
                modified: std::time::SystemTime::now(),
            };
            cards.push(AgentCard {
                target: Target::Session(session),
                title,
                project,
                status: Status::from_label(&live.label),
                activity: format!("{} · another Pui window", live.label),
                model: live.model,
                parent: None,
                action: "Open session",
                path: Some(live.path),
            });
        }
        cards.sort_by(|a, b| a.project.cmp(&b.project).then(a.title.cmp(&b.title)));
        cards
    }

    fn open_board_card(&mut self, target: Target) {
        if self.deletion.in_progress() {
            return;
        }
        self.kanban.open = false;
        self.files.close();
        self.inspector_panel = false;
        match target {
            Target::Current => self.tasks.open = false,
            Target::Agent(id) => self.view_agent(&id),
            Target::Session(session) => self.open_chat(session),
            Target::Task { task, run } => self.open_task_from_board(task, run),
        }
        self.focus_composer = !self.tasks.open;
    }

    pub(super) fn agents_board(&mut self, ctx: &egui::Context) {
        let colors = theme::palette(ctx);
        let cards = self.board_cards();
        let now = Instant::now();
        self.kanban
            .observed
            .retain(|key, _| cards.iter().any(|c| c.target.key() == *key));
        for card in &cards {
            let entry = self
                .kanban
                .observed
                .entry(card.target.key())
                .or_insert((card.status, now));
            if entry.0 != card.status {
                *entry = (card.status, now);
            }
        }
        let projects: std::collections::BTreeSet<_> =
            cards.iter().map(|c| c.project.clone()).collect();
        if self
            .kanban
            .project
            .as_ref()
            .is_some_and(|p| !projects.contains(p))
        {
            self.kanban.project = None;
        }
        let mut target = None;
        egui::CentralPanel::default()
            .frame(egui::Frame::new().fill(colors.bg).inner_margin(24))
            .show(ctx, |ui| {
                // Keep overflow discoverable and draggable, including without a trackpad.
                ui.spacing_mut().scroll = egui::style::ScrollStyle::solid();
                // Nested scrollers consume only their own axis, not a wheel gesture
                // intended for a column that has reached its top/bottom.
                ui.style_mut().always_scroll_the_only_direction = false;
                ui.horizontal(|ui| {
                    ui.label(RichText::new("Agents board").size(25.0).strong());
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        ui.label(RichText::new(if self.demo { "DEMO · example agents" } else { "Live status" })
                            .size(11.0).color(if self.demo { colors.amber } else { colors.green }));
                    });
                });
                ui.label(RichText::new("One place to see what every agent is doing.").size(12.0).color(colors.muted));
                ui.add_space(20.0);
                ui.horizontal_wrapped(|ui| {
                    egui::ComboBox::from_id_salt("board-project")
                        .selected_text(self.kanban.project.as_ref().map(|p| sessions::project_name(p)).unwrap_or_else(|| "All projects".into()))
                        .show_ui(ui, |ui| {
                            ui.selectable_value(&mut self.kanban.project, None, "All projects");
                            for project in &projects {
                                ui.selectable_value(&mut self.kanban.project, Some(project.clone()), sessions::project_name(project))
                                    .on_hover_text(project.display().to_string());
                            }
                        });
                    egui::ComboBox::from_id_salt("board-agent-kind")
                        .selected_text(if self.kanban.roots_only { "Main agents only" } else { "Agents & subagents" })
                        .show_ui(ui, |ui| {
                            ui.selectable_value(&mut self.kanban.roots_only, false, "Agents & subagents");
                            ui.selectable_value(&mut self.kanban.roots_only, true, "Main agents only");
                        });
                });
                let visible: Vec<_> = cards.iter().filter(|c| {
                    self.kanban.project.as_ref().is_none_or(|p| *p == c.project)
                        && (!self.kanban.roots_only || c.parent.is_none())
                }).collect();
                let waiting = visible.iter().filter(|c| c.status == Status::Waiting).count();
                ui.add_space(8.0);
                ui.label(RichText::new(format!("{} agents · {waiting} waiting for you", visible.len()))
                    .size(11.0).color(if waiting > 0 { colors.amber } else { colors.dim }));
                ui.add_space(18.0);
                let mut statuses = vec![Status::Working, Status::Waiting, Status::Idle, Status::Error];
                for status in [Status::Queued, Status::Stopped] {
                    if visible.iter().any(|c| c.status == status) { statuses.push(status); }
                }
                let height = (ui.available_height() - 58.0).max(180.0);
                let gap = 14.0;
                let width = ((ui.available_width() - gap * (statuses.len() - 1) as f32) / statuses.len() as f32).max(230.0);
                let board_scroll = egui::ScrollArea::horizontal().id_salt("agents-board-horizontal")
                    .max_height(height + 22.0).auto_shrink([false, false]).show(ui, |ui| {
                    ui.spacing_mut().item_spacing.x = gap;
                    ui.horizontal_top(|ui| {
                        for status in statuses {
                            let items: Vec<_> = visible.iter().copied().filter(|c| c.status == status).collect();
                            egui::Frame::new().fill(colors.surface.gamma_multiply(0.9))
                                .stroke(Stroke::new(1.0, colors.border)).corner_radius(10).inner_margin(10)
                                .show(ui, |ui| {
                                ui.with_layout(Layout::top_down(Align::Min), |ui| {
                                ui.set_width(width - 22.0);
                                ui.set_min_height(height);
                                let bottom = ui.cursor().top() + height;
                                ui.horizontal(|ui| {
                                    dot(ui, status.color(colors));
                                    ui.label(RichText::new(status.label()).size(12.0));
                                    ui.label(RichText::new(items.len().to_string()).size(10.0).color(colors.dim));
                                });
                                ui.add_space(12.0);
                                let scroll = egui::ScrollArea::vertical().id_salt(("agents-board-column", status.label()))
                                    .max_height((bottom - ui.cursor().top()).max(1.0)).auto_shrink([false, false])
                                    .show(ui, |ui| {
                                    if items.is_empty() {
                                        ui.label(RichText::new(if visible.is_empty() && cards.is_empty() {
                                            "No live agents yet. Open a chat to get started."
                                        } else { "No agents" }).size(11.0).color(colors.dim));
                                    }
                                    for card in items {
                                        let since = self.kanban.observed[&card.target.key()].1;
                                        let checkpoint=if let Target::Task{task,..}=&card.target{self.tasks.records.iter().find(|t|&t.id==task).and_then(|t|t.checkpoint_summary())}else{None};
                                        if agent_card(ui, card, since.elapsed(),checkpoint.as_deref()) {
                                            target = Some(card.target.clone());
                                        }
                                        ui.add_space(5.0);
                                    }
                                });
                                #[cfg(test)]
                                ctx.data_mut(|data| data.insert_temp(egui::Id::new(("kanban-scroll", status.label())),
                                    (scroll.id, scroll.inner_rect, scroll.content_size)));
                                #[cfg(not(test))]
                                let _ = scroll;
                                });
                            });
                        }
                    });
                });
                #[cfg(test)]
                ctx.data_mut(|data| data.insert_temp(egui::Id::new("kanban-horizontal-scroll"), board_scroll.id));
                #[cfg(not(test))]
                let _ = board_scroll;
                ui.add_space(10.0);
                ui.label(RichText::new("Click an agent to open its session · Statuses update automatically, no manual dragging")
                    .size(10.0).color(colors.dim));
            });
        if let Some(target) = target {
            self.open_board_card(target);
        }
        // Only the visible elapsed labels need a timer; agent state is event-driven.
        if !cards.is_empty() {
            ctx.request_repaint_after(Duration::from_secs(1));
        }
    }
}

fn short(text: &str, max: usize) -> String {
    let mut chars = text.chars();
    let prefix: String = chars.by_ref().take(max).collect();
    if chars.next().is_some() {
        format!("{prefix}…")
    } else {
        prefix
    }
}
fn elapsed(duration: Duration) -> String {
    let seconds = duration.as_secs();
    if seconds < 60 {
        format!("{seconds}s")
    } else if seconds < 3600 {
        format!("{}m", seconds / 60)
    } else {
        format!("{}h {}m", seconds / 3600, seconds / 60 % 60)
    }
}
/// Bound previews by rendered lines, not character count (commands may contain newlines).
fn card_preview(
    ui: &mut egui::Ui,
    text: &str,
    size: f32,
    color: Color32,
    rows: usize,
    strong: bool,
) {
    let font = if strong {
        FontId::new(size, egui::FontFamily::Name("body-bold".into()))
    } else {
        FontId::proportional(size)
    };
    let mut job = egui::text::LayoutJob::simple(text.into(), font, color, ui.available_width());
    job.wrap.max_rows = rows;
    job.wrap.break_anywhere = true;
    ui.add(egui::Label::new(job).wrap());
}

fn agent_card(
    ui: &mut egui::Ui,
    card: &AgentCard,
    duration: Duration,
    checkpoint: Option<&str>,
) -> bool {
    let colors = theme::palette(ui.ctx());
    let response = ui
        .push_id(card.target.key(), |ui| {
            let frame = egui::Frame::new()
                .fill(colors.surface)
                .stroke(Stroke::new(1.0, colors.border))
                .corner_radius(7)
                .inner_margin(egui::Margin::symmetric(10, 6))
                .show(ui, |ui| {
                    ui.spacing_mut().item_spacing =
                        egui::vec2(6.0, if checkpoint.is_some() { 0.0 } else { 2.0 });
                    ui.spacing_mut().interact_size.y = 18.0;
                    ui.set_width((ui.available_width() - 2.0).max(60.0));
                    ui.horizontal(|ui| {
                        icons::inline(ui, Icon::Folder, colors.dim);
                        ui.add(
                            egui::Label::new(
                                RichText::new(sessions::project_name(&card.project))
                                    .size(10.0)
                                    .color(colors.dim),
                            )
                            .truncate()
                            .show_tooltip_when_elided(false),
                        )
                        .on_hover_text(card.project.display().to_string());
                        if card.parent.is_some() {
                            ui.label(RichText::new("Subagent").size(9.0).color(colors.muted));
                        }
                    });
                    card_preview(ui, &card.title, 12.0, colors.text, 2, true);
                    let attention = matches!(card.status, Status::Waiting | Status::Error);
                    if attention {
                        egui::Frame::new()
                            .fill(if card.status == Status::Waiting {
                                colors.amber.gamma_multiply(0.12)
                            } else {
                                colors.error_bg
                            })
                            .corner_radius(5)
                            .inner_margin(egui::Margin::symmetric(6, 4))
                            .show(ui, |ui| {
                                ui.set_width(ui.available_width());
                                card_preview(
                                    ui,
                                    &card.activity,
                                    11.0,
                                    card.status.color(colors),
                                    2,
                                    false,
                                );
                            });
                    } else {
                        card_preview(ui, &card.activity, 11.0, colors.muted, 1, false);
                    }
                    if let Some(checkpoint) = checkpoint {
                        ui.scope(|ui| {
                            ui.spacing_mut().interact_size.y = 12.0;
                            card_preview(ui, checkpoint, 10.0, colors.dim, 1, false);
                        });
                    }
                    if let Some(parent) = &card.parent {
                        ui.add(
                            egui::Label::new(
                                RichText::new(format!("Parent · {parent}"))
                                    .size(10.0)
                                    .color(colors.dim),
                            )
                            .truncate(),
                        );
                    }
                    ui.horizontal(|ui| {
                        ui.add(
                            egui::Label::new(
                                RichText::new(if card.model.is_empty() {
                                    "Model not reported"
                                } else {
                                    &card.model
                                })
                                .size(10.0)
                                .color(colors.dim),
                            )
                            .truncate(),
                        );
                        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                            ui.label(
                                RichText::new(elapsed(duration))
                                    .size(10.0)
                                    .color(colors.dim),
                            )
                            .on_hover_text(
                                "Time in this status since first observed on this board",
                            );
                        });
                    });
                    if attention {
                        ui.label(
                            RichText::new(format!("{} →", card.action))
                                .size(11.0)
                                .color(card.status.color(colors)),
                        );
                    }
                });
            let response = ui.interact(
                frame.response.rect,
                ui.id().with("open-agent"),
                Sense::click(),
            );
            response.widget_info(|| {
                egui::WidgetInfo::labeled(egui::WidgetType::Button, true, &card.title)
            });
            if response.hovered() || response.has_focus() {
                ui.painter().rect_stroke(
                    response.rect,
                    7,
                    Stroke::new(1.0, colors.accent),
                    egui::StrokeKind::Inside,
                );
            }
            response.on_hover_cursor(egui::CursorIcon::PointingHand)
        })
        .inner;
    response.clicked()
}

#[cfg(test)]
mod compact_tests;
#[cfg(test)]
mod tests;
