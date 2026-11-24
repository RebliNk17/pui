//! Cached, viewport-culled conversation and the sent-message navigation rail.
use crate::{
    activity::Summary,
    icons::{self, Icon},
    markdown,
    protocol::{Block, Message, PromptUsage},
};
use eframe::egui::{self, Align, FontId, Layout, Rect, RichText, Sense, Stroke, Vec2};
use std::{
    collections::HashMap,
    hash::{Hash, Hasher},
    sync::{Arc, Mutex, atomic::AtomicUsize},
};

mod error;
#[cfg(test)]
mod git_activity_tests;
mod inline_cards;
mod new_messages;
mod progress;
#[cfg(test)]
mod rtl_tests;
pub use progress::Progress;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TranscriptAction {
    Open(String),
    ViewSource(String),
    Continue(usize),
    OpenVisual {
        id: String,
        title: String,
        html: String,
    },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FindTarget {
    pub query: String,
    pub message_index: usize,
    pub occurrence_in_message: usize,
    pub scroll_to_active: bool,
}

fn scope_find(
    ctx: &egui::Context,
    target: Option<&FindTarget>,
    message_index: usize,
    active_rect: &Arc<Mutex<Option<Rect>>>,
) {
    let highlight = target
        .filter(|f| !f.query.is_empty())
        .map(|f| markdown::FindHighlight {
            query: f.query.clone(),
            active_in_scope: (f.message_index == message_index).then_some(f.occurrence_in_message),
            seen: Arc::new(AtomicUsize::new(0)),
            active_rect: active_rect.clone(),
        });
    markdown::set_find_highlight(ctx, highlight);
}

#[derive(Default)]
pub struct Transcript {
    heights: HashMap<usize, (u64, f32, f32)>,
    pub jump: Option<f32>,
    pub scroll: f32,
    pub markers: Vec<f32>,
    pub total: f32,
    pub viewport: f32,
    pub active: usize,
    pub rendered: usize,
    pub view_revision: u64,
    pub expand_activity: bool,
    pub view: View,
    pub gallery: crate::media::Gallery,
    pub visuals: crate::inline_visual::InlineVisualHost,
    pub cwd: Option<std::path::PathBuf>,
    pub action: Option<TranscriptAction>,
    positions: Vec<(usize, f32)>,
    restore: Option<(usize, f32)>,
    pub progress: Option<Progress>,
    pub message_action: Option<(usize, MessageAction)>,
    pub message_actions_enabled: bool,
    pub continue_enabled: bool,
    pub task_output: bool,
    /// Same chat rendering with smaller gutters inside a companion pane.
    pub embedded: bool,
    pub pending_messages: Vec<crate::protocol::PendingMessage>,
    pub find: Option<FindTarget>,
    progress_phase: Option<(Progress, f64)>,
    new_messages: new_messages::NewMessages,
}

#[derive(
    Clone, Copy, Debug, Default, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize,
)]
pub enum View {
    #[default]
    Normal,
    Verbose,
}

impl View {
    pub fn menu(&mut self, ui: &mut egui::Ui) {
        ui.menu_button("Transcript view", |ui| {
            crate::theme::menu_controls(ui);
            for (label, view) in [("Normal", Self::Normal), ("Verbose", Self::Verbose)] {
                if ui.selectable_value(self, view, label).on_hover_text(match view {
                    Self::Normal => "Action summaries, progress updates and replies; raw thinking stays hidden.",
                    Self::Verbose => "Include expandable thinking and detailed tool calls/results.",
                }).clicked() {
                    ui.close();
                }
            }
        });
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MessageAction {
    Edit,
    Fork,
    Rewind,
}

fn agent_message_title(text: &str) -> String {
    let header = text.lines().next().unwrap_or_default();
    if let Some(sender) = header
        .strip_prefix("Agent message from ")
        .and_then(|header| header.strip_suffix(':'))
    {
        format!("Message from {sender}")
    } else if let Some((sender, _)) = header
        .strip_prefix("Worker ")
        .and_then(|header| header.split_once(" settled."))
    {
        format!("Report from {sender}")
    } else {
        "Agent message received".into()
    }
}

fn agent_message(ui: &mut egui::Ui, text: &str) -> egui::Response {
    crate::section::Section::new(agent_message_title(text))
        .id_salt("agent-message")
        .icon(Icon::Branch)
        .grouped()
        .show(ui, |ui| markdown::show(ui, text))
}

fn compaction_separator(ui: &mut egui::Ui) {
    let colors = crate::theme::palette(ui.ctx());
    let (rect, response) =
        ui.allocate_exact_size(egui::vec2(ui.available_width(), 22.0), Sense::hover());
    let label = "Context compacted";
    let galley =
        ui.painter()
            .layout_no_wrap(label.into(), FontId::proportional(11.0), colors.muted);
    let center = rect.center();
    let gap = (galley.size().x / 2.0 + 12.0).min(rect.width() / 2.0);
    for (left, right) in [
        (rect.left(), center.x - gap),
        (center.x + gap, rect.right()),
    ] {
        ui.painter().line_segment(
            [egui::pos2(left, center.y), egui::pos2(right, center.y)],
            egui::Stroke::new(1.0, colors.border),
        );
    }
    ui.painter()
        .with_clip_rect(rect)
        .galley(center - galley.size() / 2.0, galley, colors.muted);
    response.on_hover_text(
        "The model's context was summarized here. Earlier messages remain visible in this chat.",
    );
}

/// Each sent message gets exactly one mark. Clustered marks spread near the pointer.
pub fn zoom_markers(positions: &[f32], focus: Option<f32>, height: f32) -> Vec<f32> {
    let Some(focus) = focus else {
        return positions.to_vec();
    };
    if positions.len() < 2 || height < 32.0 {
        return positions.to_vec();
    }
    let closest = (0..positions.len())
        .min_by(|a, b| {
            (positions[*a] - focus)
                .abs()
                .total_cmp(&(positions[*b] - focus).abs())
        })
        .unwrap_or(0);
    if (closest == 0 || positions[closest] - positions[closest - 1] >= 14.0)
        && (closest + 1 == positions.len() || positions[closest + 1] - positions[closest] >= 14.0)
    {
        return positions.to_vec();
    }
    let count = positions
        .len()
        .min(11)
        .min(((height - 16.0) / 16.0).floor().max(2.0) as usize);
    let first = closest
        .saturating_sub(count / 2)
        .min(positions.len() - count);
    let last = first + count - 1;
    let start = (focus - (closest - first) as f32 * 16.0)
        .clamp(8.0, (height - 8.0 - (count - 1) as f32 * 16.0).max(8.0));
    let end = start + (count - 1) as f32 * 16.0;
    positions
        .iter()
        .enumerate()
        .map(|(i, y)| {
            if i < first {
                4.0 + (start - 8.0) * y / positions[first].max(1.0)
            } else if i > last {
                end + 4.0
                    + (height - end - 8.0) * (y - positions[last])
                        / (height - positions[last]).max(1.0)
            } else {
                start + (i - first) as f32 * 16.0
            }
        })
        .collect()
}

impl Transcript {
    pub fn clear(&mut self) {
        *self = Self::default();
    }
    /// Jump to the containing turn after layout, including viewport-culled messages.
    pub fn jump_to_message(&mut self, index: usize) {
        self.restore = Some((index, 0.0));
    }
    /// Keep the same visible turn when the saved tail is replaced by full RPC history.
    pub fn replace_history(&mut self, old: &[Message], new: &[Message]) {
        // Loading older history is not an incoming message.
        self.new_messages.rebase();
        let at_bottom = self.total - self.viewport - self.scroll < 32.0;
        let anchor = self
            .positions
            .iter()
            .rev()
            .find(|(_, y)| *y <= self.scroll + 1.0)
            .and_then(|(index, y)| {
                let message = old.get(*index)?;
                new.iter()
                    .position(|m| {
                        m.role == message.role
                            && m.timestamp == message.timestamp
                            && m.text == message.text
                    })
                    .map(|index| (index, self.scroll - y))
            });
        self.heights.clear();
        self.positions.clear();
        if !at_bottom {
            self.restore = anchor;
        } else {
            self.jump = None;
        }
    }
    pub fn show(
        &mut self,
        ui: &mut egui::Ui,
        messages: &[Message],
        model: &str,
        thinking: &str,
        busy: bool,
        session: &str,
    ) {
        self.show_with_footer(ui, messages, model, thinking, busy, session, |_| {});
    }

    #[allow(clippy::too_many_arguments)]
    pub fn show_with_footer(
        &mut self,
        ui: &mut egui::Ui,
        messages: &[Message],
        model: &str,
        thinking: &str,
        busy: bool,
        session: &str,
        footer: impl FnOnce(&mut egui::Ui),
    ) {
        self.new_messages.observe(messages, session);
        let colors = crate::theme::palette(ui.ctx());
        let now = ui.input(|i| i.time);
        let elapsed = if let Some(progress) = self.progress {
            self.progress_phase.get_or_insert((progress, now));
            if self
                .progress_phase
                .is_some_and(|(previous, _)| previous != progress)
            {
                self.progress_phase = Some((progress, now));
            }
            (now - self.progress_phase.unwrap().1).max(0.0)
        } else {
            self.progress_phase = None;
            0.0
        };
        let progress = self.progress;
        let pending_messages = &self.pending_messages;
        // Textures replace fixed-size thumbnail slots; loading must not invalidate
        // every offscreen turn and restart image decoding across the whole history.
        self.gallery.poll(ui.ctx());
        self.visuals.poll(ui.ctx());
        let rect = ui.available_rect_before_wrap();
        let rail_width = if self.embedded { 0.0 } else { 24.0 };
        let rail = Rect::from_min_size(rect.min, Vec2::new(rail_width, rect.height()));
        let content = Rect::from_min_max(rect.min + egui::vec2(rail_width, 0.0), rect.max);
        let mut child = ui.new_child(
            egui::UiBuilder::new()
                .max_rect(content)
                .layout(Layout::top_down(Align::LEFT)),
        );
        // Transcript labels and footers are not 32pt form controls.
        child.spacing_mut().item_spacing.y = 4.0;
        child.spacing_mut().interact_size.y = 22.0;
        child.spacing_mut().button_padding = egui::vec2(6.0, 3.0);
        let gutter = if self.embedded { 16.0 } else { 64.0 };
        let column = (content.width() - gutter).clamp(120.0, 736.0);
        let padding =
            ((content.width() - column) / 2.0).max(if self.embedded { 8.0 } else { 12.0 });
        let mut area = egui::ScrollArea::vertical()
            .id_salt(("transcript-v2", session))
            .auto_shrink([false, false])
            .scroll_bar_visibility(egui::scroll_area::ScrollBarVisibility::AlwaysHidden)
            .stick_to_bottom(true);
        let jump_to_latest = std::mem::take(&mut self.new_messages.jump_to_latest);
        if jump_to_latest {
            area = area.animated(false);
        }
        if let Some(jump) = self.jump.take() {
            area = area.vertical_scroll_offset(jump).stick_to_bottom(false);
        }
        self.rendered = 0;
        let mut markers = Vec::new();
        let mut positions = Vec::new();
        let restore = self.restore.take();
        let mut restored = None;
        let revision = self.view_revision;
        let mut layout_changed = false;
        let expand = self.expand_activity;
        let view = self.view;
        let task_output = self.task_output;
        let gallery = &mut self.gallery;
        let visuals = &mut self.visuals;
        let cwd = self.cwd.as_deref();
        let mut action: Option<TranscriptAction> = None;
        let mut message_action = None;
        let actions_enabled = self.message_actions_enabled;
        let find_target = self.find.clone();
        let find_rect: Arc<Mutex<Option<Rect>>> = Arc::new(Mutex::new(None));
        let output = area.show_viewport(&mut child, |ui, viewport| {
            let top = ui.cursor().top();
            ui.add_space(16.0);
            let mut index = 0;
            while index < messages.len() {
                let start = index;
                if messages[index].role == "assistant" {
                    index += 1;
                    while index < messages.len() && messages[index].role == "assistant" {
                        index += 1;
                    }
                } else {
                    index += 1;
                }
                let group = &messages[start..index];
                let y = ui.cursor().top() - top;
                positions.push((start, y));
                if let Some((target, offset)) = restore {
                    if target >= start && target < index {
                        restored = Some(y + offset);
                    }
                }
                if group[0].role == "user" {
                    markers.push(y);
                }
                let mut hasher = std::collections::hash_map::DefaultHasher::new();
                revision.hash(&mut hasher);
                view.hash(&mut hasher);
                expand.hash(&mut hasher);
                task_output.hash(&mut hasher);
                for m in group {
                    m.role.hash(&mut hasher);
                    m.revision.hash(&mut hasher);
                    m.text.len().hash(&mut hasher);
                    m.error.hash(&mut hasher);
                    m.tools.len().hash(&mut hasher);
                    for image in &m.images {
                        image.key.hash(&mut hasher);
                    }
                    for t in &m.tools {
                        t.finished.hash(&mut hasher);
                        t.output.len().hash(&mut hasher);
                        t.duration_ms.hash(&mut hasher);
                        t.failed.hash(&mut hasher);
                        t.images.len().hash(&mut hasher);
                        t.visual.is_some().hash(&mut hasher);
                        t.open_target.hash(&mut hasher);
                        t.open_queued.hash(&mut hasher);
                        for child in &t.children {
                            child.finished.hash(&mut hasher);
                            child.failed.hash(&mut hasher);
                            child.output.len().hash(&mut hasher);
                            child.args.len().hash(&mut hasher);
                            child.images.len().hash(&mut hasher);
                        }
                    }
                }
                self.message_actions_enabled.hash(&mut hasher);
                (self.continue_enabled, index == messages.len()).hash(&mut hasher);
                (busy && index == messages.len()).hash(&mut hasher);
                let signature = hasher.finish();
                let cached = self
                    .heights
                    .get(&start)
                    .filter(|(sig, width, _)| *sig == signature && (*width - column).abs() < 0.5)
                    .map(|(_, _, h)| *h);
                let offscreen = cached.is_some_and(|height| {
                    y + height < viewport.top() - 100.0 || y > viewport.bottom() + 100.0
                });
                if offscreen {
                    ui.allocate_space(Vec2::new(
                        column,
                        (cached.unwrap() - ui.spacing().item_spacing.y).max(0.0),
                    ));
                } else {
                    self.rendered += group.len();
                    let before = ui.cursor().top();
                    ui.scope_builder(
                        egui::UiBuilder::new()
                            .id_salt(("turn", session, start))
                            .max_rect(Rect::from_min_size(
                                egui::pos2(ui.min_rect().left() + padding, before),
                                Vec2::new(column, 1.0),
                            ))
                            .layout(Layout::top_down(Align::LEFT)),
                        |ui| {
                            ui.set_width(column);
                            ui.push_id(start, |ui| {
                                if group[0].role == "user" {
                                    scope_find(ui.ctx(), find_target.as_ref(), start, &find_rect);
                                    if let Some(action) =
                                        user_actions(ui, &group[0], gallery, actions_enabled)
                                    {
                                        message_action = Some((start, action));
                                    }
                                } else if matches!(
                                    group[0].role.as_str(),
                                    "compaction" | "compactionSummary"
                                ) {
                                    compaction_separator(ui);
                                } else if group[0].role == "custom" {
                                    scope_find(ui.ctx(), find_target.as_ref(), start, &find_rect);
                                    layout_changed |= agent_message(ui, &group[0].text).changed();
                                } else {
                                    let (changed, fork) = assistant_with_media(
                                        ui,
                                        group,
                                        start,
                                        model,
                                        thinking,
                                        (
                                            busy && index == messages.len(),
                                            expand,
                                            actions_enabled,
                                            view,
                                        ),
                                        task_output,
                                        (index == messages.len()).then_some(self.continue_enabled),
                                        gallery,
                                        Some(visuals),
                                        cwd,
                                        &mut action,
                                        find_target.as_ref(),
                                        &find_rect,
                                    );
                                    layout_changed |= changed;
                                    if fork {
                                        message_action = Some((index - 1, MessageAction::Fork));
                                    }
                                }
                            });
                        },
                    );
                    ui.add_space(match group[0].role.as_str() {
                        "user" => 16.0,
                        "custom" => 8.0,
                        _ => 20.0,
                    });
                    self.heights
                        .insert(start, (signature, column, ui.cursor().top() - before));
                }
            }
            for pending in pending_messages {
                let before = ui.cursor().top();
                ui.scope_builder(
                    egui::UiBuilder::new()
                        .id_salt(("pending-message", &pending.id))
                        .max_rect(Rect::from_min_size(
                            egui::pos2(ui.min_rect().left() + padding, before),
                            Vec2::new(column, 1.0),
                        ))
                        .layout(Layout::top_down(Align::LEFT)),
                    |ui| {
                        ui.set_width(column);
                        let message = Message {
                            role: "user".into(),
                            text: pending.text.clone(),
                            images: pending.images.clone(),
                            ..Default::default()
                        };
                        let status_width = footer_text_width(ui, pending.status.label());
                        let footer_width = status_width + footer_action_width(ui, "Copy") + 10.0;
                        user_bubble(ui, &message, gallery, footer_width);
                        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                            ui.set_min_height(ui.spacing().interact_size.y);
                            ui.spacing_mut().item_spacing.x = 10.0;
                            ui.add_sized(
                                [status_width, ui.spacing().interact_size.y],
                                egui::Label::new(
                                    RichText::new(pending.status.label()).size(10.0).color(
                                        if pending.status
                                            == crate::protocol::DeliveryStatus::Uncertain
                                        {
                                            colors.amber
                                        } else {
                                            colors.muted
                                        },
                                    ),
                                )
                                .truncate(),
                            );
                            if icons::labeled_button(ui, Icon::Copy, "Copy", true).clicked() {
                                ui.ctx().copy_text(pending.text.clone());
                            }
                        });
                    },
                );
                ui.add_space(16.0);
            }
            if let Some(progress) = progress {
                let before = ui.cursor().top();
                ui.scope_builder(
                    egui::UiBuilder::new()
                        .max_rect(Rect::from_min_size(
                            egui::pos2(ui.min_rect().left() + padding, before),
                            Vec2::new(column, 24.0),
                        ))
                        .layout(Layout::top_down(Align::LEFT)),
                    |ui| {
                        ui.set_width(column);
                        progress::row(ui, progress, elapsed);
                    },
                );
            }
            let before = ui.cursor().top();
            ui.scope_builder(
                egui::UiBuilder::new()
                    .id_salt(("conversation-footer", session))
                    .max_rect(Rect::from_min_size(
                        egui::pos2(ui.min_rect().left() + padding, before),
                        Vec2::new(column, 1.0),
                    ))
                    .layout(Layout::top_down(Align::LEFT)),
                |ui| {
                    ui.set_width(column);
                    footer(ui);
                },
            );
            ui.add_space(12.0);
            if find_target.as_ref().is_some_and(|f| f.scroll_to_active)
                && let Ok(Some(rect)) = find_rect.lock().map(|g| *g)
            {
                ui.scroll_to_rect(rect, Some(Align::Center));
                if let Some(find) = &mut self.find {
                    find.scroll_to_active = false;
                }
            }
            if jump_to_latest {
                // Use this frame's measured tail; egui resumes sticky scrolling at the end.
                ui.scroll_with_delta(egui::vec2(0.0, -ui.min_rect().height()));
            }
        });
        markdown::set_find_highlight(ui.ctx(), None);
        // Disclosures only change their owning, visible turn. Keep all other
        // measured heights; a global revision caused full-history relayout.
        if layout_changed {
            ui.ctx().request_repaint();
        }
        self.message_action = message_action;
        self.action = action;
        self.positions = positions;
        if let Some(offset) = restored {
            self.jump = Some(offset);
            ui.ctx().request_repaint();
        }
        self.scroll = output.state.offset.y;
        self.total = output.content_size.y;
        self.viewport = output.inner_rect.height();
        self.markers = markers;
        self.active = self
            .markers
            .iter()
            .rposition(|y| *y <= self.scroll + 32.0)
            .unwrap_or(0);
        self.rail(ui, rail);
        if self.total - self.viewport - self.scroll < 32.0 {
            self.new_messages.unread = false;
        }
        if self
            .new_messages
            .show(ui, output.inner_rect)
            .is_some_and(|r| r.clicked())
        {
            self.new_messages.unread = false;
            self.new_messages.jump_to_latest = true;
            self.jump = None;
            self.restore = None;
            ui.ctx().request_repaint();
        }
        ui.advance_cursor_after_rect(rect);
    }

    fn rail(&mut self, ui: &mut egui::Ui, rect: Rect) {
        let colors = crate::theme::palette(ui.ctx());
        let response = ui.interact(rect, ui.make_persistent_id("message-rail"), Sense::click());
        let height = rect.height();
        let max = (self.total - self.viewport).max(0.0);
        let positions: Vec<_> = self
            .markers
            .iter()
            .map(|y| (y / self.total.max(1.0) * height).clamp(4.0, (height - 4.0).max(4.0)))
            .collect();
        let focus = response.hover_pos().map(|p| p.y - rect.top());
        let positions = zoom_markers(&positions, focus, height);
        let hovered = focus
            .and_then(|y| {
                (0..positions.len()).min_by(|a, b| {
                    (positions[*a] - y)
                        .abs()
                        .total_cmp(&(positions[*b] - y).abs())
                })
            })
            .filter(|i| (positions[*i] - focus.unwrap()).abs() < 7.0);
        let p = ui.painter();
        if hovered.is_some() {
            ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
        }
        for (index, y) in positions.iter().enumerate() {
            let active = index == self.active || hovered == Some(index);
            let center = egui::pos2(rect.left() + 15.0, rect.top() + y);
            p.line_segment(
                [
                    center - egui::vec2(if active { 6.5 } else { 4.5 }, 0.0),
                    center + egui::vec2(if active { 6.5 } else { 4.5 }, 0.0),
                ],
                Stroke::new(
                    2.0,
                    if active {
                        colors.accent
                    } else {
                        colors.dim.gamma_multiply(0.65)
                    },
                ),
            );
        }
        if response.clicked() {
            if let Some(index) = hovered {
                self.jump = Some(self.markers[index]);
            }
            response.request_focus();
        }
        if response.hovered() {
            let delta = ui.input_mut(|i| {
                let delta = i.smooth_scroll_delta.y;
                i.smooth_scroll_delta.y = 0.0;
                delta
            });
            if delta != 0.0 {
                self.jump = Some((self.scroll - delta).clamp(0.0, max));
            }
        }
        if response.has_focus() {
            ui.memory_mut(|m| {
                m.set_focus_lock_filter(
                    response.id,
                    egui::EventFilter {
                        vertical_arrows: true,
                        ..Default::default()
                    },
                )
            });
            ui.input(|i| {
                if i.modifiers.alt && !self.markers.is_empty() {
                    let index = if i.key_pressed(egui::Key::ArrowUp) {
                        self.active.saturating_sub(1)
                    } else if i.key_pressed(egui::Key::ArrowDown) {
                        (self.active + 1).min(self.markers.len() - 1)
                    } else {
                        self.active
                    };
                    if index != self.active {
                        self.jump = Some(self.markers[index]);
                    }
                } else {
                    for (key, value) in [
                        (egui::Key::Home, 0.0),
                        (egui::Key::End, max),
                        (egui::Key::PageUp, self.scroll - height * 0.9),
                        (egui::Key::PageDown, self.scroll + height * 0.9),
                        (egui::Key::ArrowUp, self.scroll - 40.0),
                        (egui::Key::ArrowDown, self.scroll + 40.0),
                    ] {
                        if i.key_pressed(key) {
                            self.jump = Some(value.clamp(0.0, max));
                        }
                    }
                }
            });
        }
        if self.jump.is_some() {
            ui.ctx().request_repaint();
        }
    }
}

/// Pi appends coordinate-mapping notes for model input after resized images.
/// Hide only exact, standalone trailing notes backed by attachments. Keep raw
/// Message text untouched for history, exports, synchronization and model context.
pub(crate) fn user_display_text(message: &Message) -> &str {
    if message.images.is_empty() {
        return &message.text;
    }
    let original = message.text.as_str();
    let mut text = original.trim_end();
    let mut notes = 0;
    loop {
        let (prefix, line) = text.rsplit_once('\n').unwrap_or(("", text));
        if !image_dimension_note(line) {
            break;
        }
        notes += 1;
        if notes > message.images.iter().filter(|a| a.file.is_none()).count() {
            return original;
        }
        text = prefix.trim_end();
    }
    let text = if notes == 0 { original } else { text };
    crate::media::file_manifest(text)
        .filter(|(_, files)| {
            files.iter().all(|f| {
                message
                    .images
                    .iter()
                    .any(|a| a.file.as_ref().is_some_and(|a| a.path == f.path))
            })
        })
        .map(|(body, _)| body)
        .unwrap_or(text)
}
fn image_dimension_note(line: &str) -> bool {
    fn dimensions(value: &str) -> bool {
        value.split_once('x').is_some_and(|(w, h)| {
            [w, h].into_iter().all(|n| {
                !n.is_empty()
                    && n.bytes().all(|b| b.is_ascii_digit())
                    && n.parse::<u32>().is_ok_and(|n| n > 0)
            })
        })
    }
    let Some(note) = line
        .strip_prefix("[Image: original ")
        .and_then(|s| s.strip_suffix(" to map to original image.]"))
    else {
        return false;
    };
    let Some((original, rest)) = note.split_once(", displayed at ") else {
        return false;
    };
    let Some((displayed, scale)) = rest.split_once(". Multiply coordinates by ") else {
        return false;
    };
    dimensions(original)
        && dimensions(displayed)
        && scale.split_once('.').is_some_and(|(whole, fraction)| {
            !whole.is_empty()
                && whole.bytes().all(|b| b.is_ascii_digit())
                && fraction.len() == 2
                && fraction.bytes().all(|b| b.is_ascii_digit())
        })
}
#[cfg(test)]
fn user(ui: &mut egui::Ui, message: &Message, gallery: &mut crate::media::Gallery) {
    user_actions(ui, message, gallery, false);
}
fn user_actions(
    ui: &mut egui::Ui,
    message: &Message,
    gallery: &mut crate::media::Gallery,
    enabled: bool,
) -> Option<MessageAction> {
    let footer_width = footer_text_width(ui, &time_label(message.timestamp))
        + ["Copy", "Edit", "Rewind", "Fork"]
            .into_iter()
            .map(|label| footer_action_width(ui, label))
            .sum::<f32>()
        + 40.0;
    user_bubble(ui, message, gallery, footer_width);
    user_footer(ui, user_display_text(message), message.timestamp, enabled)
}

fn footer_text_width(ui: &egui::Ui, text: &str) -> f32 {
    ui.painter()
        .layout_no_wrap(
            text.into(),
            FontId::proportional(10.0),
            crate::theme::palette(ui.ctx()).dim,
        )
        .size()
        .x
}

fn footer_action_width(ui: &egui::Ui, label: &str) -> f32 {
    footer_text_width(ui, label) + 16.0 // 12px icon + 4px gap, matching icons::labeled_button.
}

fn user_bubble(
    ui: &mut egui::Ui,
    message: &Message,
    gallery: &mut crate::media::Gallery,
    footer_width: f32,
) {
    let colors = crate::theme::palette(ui.ctx());
    let text = user_display_text(message);
    let available = ui.available_width();
    let max = (available * 0.86)
        .max(footer_width.min(available))
        .max(140.0)
        .min(available);
    let galley = crate::app::composer_text::message_galley(ui.ctx(), text, (max - 34.0).max(1.0));
    let width = (galley.size().x + 34.0)
        .max(footer_width.min(max))
        .max(if message.images.is_empty() {
            0.0
        } else {
            max.min(274.0)
        })
        .min(available);
    let left = ui.max_rect().right() - width;
    let top = ui.cursor().top();
    ui.scope_builder(
        egui::UiBuilder::new()
            .max_rect(Rect::from_min_size(
                egui::pos2(left, top),
                Vec2::new(width, 1.0),
            ))
            .layout(Layout::top_down(Align::RIGHT)),
        |ui| {
            ui.vertical(|ui| {
                ui.set_width(width);
                egui::Frame::new()
                    .fill(colors.user_bg)
                    .corner_radius(egui::CornerRadius {
                        nw: 18,
                        ne: 18,
                        sw: 18,
                        se: 4,
                    })
                    .inner_margin(egui::Margin::symmetric(17, 10))
                    .show(ui, |ui| {
                        ui.set_min_width((width - 34.0).max(0.0));
                        gallery.user_thumbnails(ui, &message.images);
                        if !message.images.is_empty() && !text.is_empty() {
                            ui.add_space(6.0);
                        }
                        if !text.is_empty() {
                            crate::app::composer_text::show_message(ui, text);
                        }
                    });
            });
        },
    );
}

fn user_footer(
    ui: &mut egui::Ui,
    text: &str,
    timestamp: i64,
    enabled: bool,
) -> Option<MessageAction> {
    let mut action = None;
    let colors = crate::theme::palette(ui.ctx());
    let unavailable = "Finish or stop the run, or use the owning window to change history.";
    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
        ui.spacing_mut().item_spacing.x = 10.0;
        // Reverse construction keeps the visible order timestamp, Copy, Edit,
        // Rewind, Fork while anchoring the whole footer to the user's right edge.
        for (label, icon, kind, hint) in [
            ("Fork", Icon::Branch, MessageAction::Fork, "Start a separate conversation branched from this message. Original chat and files stay unchanged."),
            ("Rewind", Icon::Rewind, MessageAction::Rewind, "Return to before this message. Choose file restore only when available; nothing is resent."),
            ("Edit", Icon::Edit, MessageAction::Edit, "Bring this message back to the composer. Your current draft is saved; nothing is sent automatically."),
        ] {
            if icons::labeled_button(ui, icon, label, enabled)
                .on_hover_text(hint)
                .on_disabled_hover_text(unavailable)
                .clicked()
            {
                action = Some(kind);
            }
        }
        if icons::labeled_button(ui, Icon::Copy, "Copy", true)
            .clicked()
        {
            ui.ctx().copy_text(text.to_owned());
        }
        ui.label(RichText::new(time_label(timestamp)).size(10.0).color(colors.dim));
    });
    action
}

#[cfg(test)]
#[allow(clippy::too_many_arguments)]
fn assistant(
    ui: &mut egui::Ui,
    messages: &[Message],
    start: usize,
    model: &str,
    thinking: &str,
    flags: (bool, bool, bool, View),
    task_output: bool,
    gallery: &mut crate::media::Gallery,
    find: Option<&FindTarget>,
    find_rect: &Arc<Mutex<Option<Rect>>>,
) -> (bool, bool) {
    let mut action = None;
    assistant_with_media(
        ui,
        messages,
        start,
        model,
        thinking,
        flags,
        task_output,
        Some(flags.2),
        gallery,
        None,
        None,
        &mut action,
        find,
        find_rect,
    )
}

#[allow(clippy::too_many_arguments)]
fn assistant_with_media(
    ui: &mut egui::Ui,
    messages: &[Message],
    start: usize,
    model: &str,
    thinking: &str,
    flags: (bool, bool, bool, View),
    task_output: bool,
    continue_enabled: Option<bool>,
    gallery: &mut crate::media::Gallery,
    mut visuals: Option<&mut crate::inline_visual::InlineVisualHost>,
    cwd: Option<&std::path::Path>,
    action: &mut Option<TranscriptAction>,
    find: Option<&FindTarget>,
    find_rect: &Arc<Mutex<Option<Rect>>>,
) -> (bool, bool) {
    let colors = crate::theme::palette(ui.ctx());
    let (running, expand, actions_enabled, view) = flags;
    let mut fork = false;
    let first = &messages[0];
    ui.horizontal(|ui| {
        let model_name = if first.model.is_empty() {
            model
        } else {
            &first.model
        };
        let (rect, _) = ui.allocate_exact_size(Vec2::splat(23.0), Sense::hover());
        if crate::icons::has_provider_logo(model_name) {
            ui.painter().rect_filled(rect, 7, colors.surface);
            crate::icons::draw_provider(ui, rect.shrink(4.0), model_name);
        } else {
            ui.painter().rect_filled(rect, 7, colors.accent_bg);
            ui.painter().text(
                rect.center(),
                egui::Align2::CENTER_CENTER,
                "pi",
                FontId::new(15.0, egui::FontFamily::Name("pi-serif".into())),
                colors.accent,
            );
        }
        let effort = if first.effort.is_empty() {
            thinking
        } else {
            &first.effort
        };
        ui.label(
            RichText::new(format!(
                "{}{}",
                model_name,
                if effort.is_empty() || effort == "off" {
                    String::new()
                } else {
                    format!(" · {effort}")
                }
            ))
            .size(11.0)
            .color(colors.muted),
        );
    });
    ui.add_space(6.0);
    let mut changed = false;
    let mut pending: Vec<&crate::protocol::Tool> = Vec::new();
    let mut group_index = 0;
    let flush_pending = |ui: &mut egui::Ui,
                         pending: &mut Vec<&crate::protocol::Tool>,
                         group_index: &mut usize,
                         running_run: bool,
                         gallery: &mut crate::media::Gallery,
                         action: &mut Option<TranscriptAction>|
     -> bool {
        if pending.is_empty() {
            return false;
        }
        let changed = action_run(
            ui,
            pending,
            *group_index,
            running_run,
            expand,
            view,
            gallery,
        );
        inline_cards::html_file_cards(ui, pending, action);
        pending.clear();
        *group_index += 1;
        changed
    };
    for (message_index, message) in messages.iter().enumerate() {
        scope_find(ui.ctx(), find, start + message_index, find_rect);
        if message.blocks.is_empty() {
            // Normal summarizes recorded actions, never internal reasoning. Hidden
            // thinking must not split one action run into many summary rows.
            // Verbose retains legacy reasoning before the reply.
            if view == View::Verbose && !message.thinking.is_empty() {
                changed |=
                    flush_pending(ui, &mut pending, &mut group_index, false, gallery, action);
                changed |= thinking_text(ui, &message.thinking, (message_index, "legacy"));
            }
            if !message.text.is_empty() {
                changed |=
                    flush_pending(ui, &mut pending, &mut group_index, false, gallery, action);
                if !task_output || !crate::task_output::show(ui, &message.text) {
                    markdown::show_reply_with_media(
                        ui,
                        &message.text,
                        (message_index, "text"),
                        Some(markdown::InlineMedia { gallery, base: cwd }),
                    );
                }
            }

            for tool in &message.tools {
                if let Some(answers) = &tool.answers {
                    changed |=
                        flush_pending(ui, &mut pending, &mut group_index, false, gallery, action);
                    crate::tool_view::answers::show(ui, answers);
                } else if let Some(visual) = &tool.visual {
                    changed |=
                        flush_pending(ui, &mut pending, &mut group_index, false, gallery, action);
                    if let Some(visuals) = visuals.as_deref_mut() {
                        let outcome = visuals.show(ui, &tool.id, visual);
                        changed |= outcome.changed;
                        if outcome.expand {
                            *action = Some(TranscriptAction::OpenVisual {
                                id: tool.id.clone(),
                                title: visual.title.clone(),
                                html: visual.html.clone(),
                            });
                        }
                    }
                } else if tool.name == "pui_open" && !tool.failed && tool.open_target.is_some() {
                    changed |=
                        flush_pending(ui, &mut pending, &mut group_index, false, gallery, action);
                    if let Some(visuals) = visuals.as_deref_mut() {
                        inline_cards::open_card(ui, tool, gallery, visuals, cwd, action);
                    }
                } else {
                    pending.push(tool);
                }
            }
        } else {
            for (block_index, block) in message.blocks.iter().enumerate() {
                match block {
                    Block::Text(text) if !text.is_empty() => {
                        changed |= flush_pending(
                            ui,
                            &mut pending,
                            &mut group_index,
                            false,
                            gallery,
                            action,
                        );
                        if !task_output || !crate::task_output::show(ui, text) {
                            markdown::show_reply_with_media(
                                ui,
                                text,
                                (message_index, "block", block_index),
                                Some(markdown::InlineMedia { gallery, base: cwd }),
                            );
                        }
                    }
                    Block::Thinking(text) if view == View::Verbose && !text.is_empty() => {
                        changed |= flush_pending(
                            ui,
                            &mut pending,
                            &mut group_index,
                            false,
                            gallery,
                            action,
                        );
                        changed |= thinking_text(ui, text, (message_index, block_index));
                    }
                    Block::Tool(id) => {
                        if let Some(tool) = message.tools.iter().find(|t| t.id == *id) {
                            if let Some(answers) = &tool.answers {
                                changed |= flush_pending(
                                    ui,
                                    &mut pending,
                                    &mut group_index,
                                    false,
                                    gallery,
                                    action,
                                );
                                crate::tool_view::answers::show(ui, answers);
                            } else if let Some(visual) = &tool.visual {
                                changed |= flush_pending(
                                    ui,
                                    &mut pending,
                                    &mut group_index,
                                    false,
                                    gallery,
                                    action,
                                );
                                if let Some(visuals) = visuals.as_deref_mut() {
                                    let outcome = visuals.show(ui, &tool.id, visual);
                                    changed |= outcome.changed;
                                    if outcome.expand {
                                        *action = Some(TranscriptAction::OpenVisual {
                                            id: tool.id.clone(),
                                            title: visual.title.clone(),
                                            html: visual.html.clone(),
                                        });
                                    }
                                }
                            } else if tool.name == "pui_open"
                                && !tool.failed
                                && tool.open_target.is_some()
                            {
                                changed |= flush_pending(
                                    ui,
                                    &mut pending,
                                    &mut group_index,
                                    false,
                                    gallery,
                                    action,
                                );
                                if let Some(visuals) = visuals.as_deref_mut() {
                                    inline_cards::open_card(
                                        ui, tool, gallery, visuals, cwd, action,
                                    );
                                }
                            } else {
                                pending.push(tool);
                            }
                        }
                    }
                    _ => {}
                }
            }
        }
    }
    changed |= flush_pending(ui, &mut pending, &mut group_index, running, gallery, action);
    // Failed attempts remain available in history, but a later assistant message
    // supersedes them. While Pi is still working it may retry or compact first.
    if !running {
        if let Some(error) = messages.last().and_then(|message| message.error.as_ref()) {
            if error::row(ui, error, continue_enabled) {
                *action = Some(TranscriptAction::Continue(start + messages.len() - 1));
            }
        }
    }
    let images: Vec<_> = messages
        .iter()
        .flat_map(|m| m.images.iter().cloned())
        .collect();
    for (idx, image) in images.iter().enumerate() {
        gallery.inline_photo(ui, image, &images, idx, None);
    }
    if !running {
        let files = crate::activity::turn_files(messages);
        if !files.is_empty() {
            let added: usize = files.iter().map(|f| f.added).sum();
            let removed: usize = files.iter().map(|f| f.removed).sum();
            let label = format!(
                "{} {} recorded · +{added} −{removed}",
                files.len(),
                if files.len() == 1 { "file" } else { "files" }
            );
            let id = ui.id().with("turn-recorded-files");
            let mut open = ui
                .ctx()
                .data(|data| data.get_temp::<bool>(id))
                .unwrap_or(false);
            if icons::labeled_button(ui, Icon::File, &label, true)
                .on_hover_text("Successful recorded edit/write operations in this turn, not a minimal Git diff. Write counts do not infer removed lines.")
                .clicked() {
                open = !open;
                changed = true;
                ui.ctx().data_mut(|data| data.insert_temp(id, open));
            }
            if open {
                egui::Frame::new()
                    .fill(colors.surface)
                    .corner_radius(7)
                    .inner_margin(10)
                    .show(ui, |ui| {
                        for file in &files {
                            ui.horizontal(|ui| {
                                ui.add(
                                    egui::Label::new(RichText::new(&file.path).size(11.0))
                                        .truncate(),
                                );
                                ui.label(
                                    RichText::new(format!("+{}", file.added))
                                        .size(10.0)
                                        .color(colors.green),
                                );
                                ui.label(
                                    RichText::new(format!("−{}", file.removed))
                                        .size(10.0)
                                        .color(colors.red),
                                );
                            });
                        }
                    });
            }
        }
        ui.add_space(4.0);
        let last = messages.last().unwrap();
        let cost: f64 = messages.iter().map(|m| m.cost).sum();
        let usage = PromptUsage::from_messages(messages);
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 10.0;
            ui.label(
                RichText::new(time_label(last.timestamp))
                    .size(10.0)
                    .color(colors.dim),
            );
            if cost > 0.0 {
                ui.label(
                    RichText::new(format!("·   ${cost:.2}"))
                        .size(10.0)
                        .color(colors.dim),
                );
            }
            if let Some(label) = usage.cache_label() {
                ui.label(
                    RichText::new(format!("·   {label}"))
                        .size(10.0)
                        .color(colors.dim),
                )
                .on_hover_text(usage.cache_tooltip());
            }
            if icons::labeled_button(ui, Icon::Copy, "Copy", true)
                .clicked()
            {
                ui.ctx().copy_text(
                    messages
                        .iter()
                        .map(|m| m.text.as_str())
                        .collect::<Vec<_>>()
                        .join("\n\n"),
                );
            }
            if icons::labeled_button(ui, Icon::Copy, "Quote", true)
                .on_hover_text("Append a reply excerpt to the draft without sending it")
                .clicked()
            {
                crate::app::context_capture::queue_parts(ui.ctx(), "Quoted assistant reply", messages.iter().map(|message| message.text.as_str()));
            }
            if last.role == "assistant" && last.error.is_none() && last.tools.is_empty()
                && icons::labeled_button(ui, Icon::Branch, "Fork", actions_enabled)
                    .on_hover_text("Start a separate chat including this response. The original chat and files stay unchanged.").clicked() { fork = true; }
        });
    }
    (changed, fork)
}

fn thinking_text(ui: &mut egui::Ui, text: &str, identity: impl Hash) -> bool {
    let colors = crate::theme::palette(ui.ctx());
    let id = ui.make_persistent_id(("reasoning-preview", identity));
    let mut expanded = ui
        .ctx()
        .data(|data| data.get_temp::<bool>(id))
        .unwrap_or(false);
    let long = text.chars().take(601).count() > 600 || text.lines().take(13).count() > 12;
    let mut changed = false;
    ui.add_space(6.0);
    let response = egui::Frame::new()
        .inner_margin(egui::Margin {
            left: 12,
            right: 4,
            top: 2,
            bottom: 2,
        })
        .show(ui, |ui| {
            ui.label(RichText::new("Thinking").size(11.0).color(colors.muted));
            if long && !expanded {
                // Bound layout work and height without replacing the source in history.
                let preview: String = text.chars().take(600).collect();
                let mut job = egui::text::LayoutJob::simple(
                    preview,
                    FontId::proportional(12.0),
                    colors.muted,
                    ui.available_width(),
                );
                job.wrap.max_rows = 5;
                ui.add(egui::Label::new(job).selectable(true));
            } else {
                markdown::show(ui, text);
            }
            if long
                && icons::labeled_button(
                    ui,
                    Icon::Spark,
                    if expanded { "Show less" } else { "Show more" },
                    true,
                )
                .clicked()
            {
                expanded = !expanded;
                changed = true;
                ui.ctx().data_mut(|data| data.insert_temp(id, expanded));
            }
        });
    ui.painter().line_segment(
        [
            response.response.rect.left_top(),
            response.response.rect.left_bottom(),
        ],
        Stroke::new(2.0, colors.border),
    );
    ui.add_space(6.0);
    changed
}

fn action_run(
    ui: &mut egui::Ui,
    tools: &[&crate::protocol::Tool],
    index: usize,
    running: bool,
    expand: bool,
    view: View,
    gallery: &mut crate::media::Gallery,
) -> bool {
    if view == View::Normal {
        activity(ui, tools, index, running, expand, gallery)
    } else {
        ui.push_id(("verbose-actions", index), |ui| {
            for tool in tools {
                crate::tool_view::row_verbose_with_gallery(ui, tool, gallery);
            }
        });
        false
    }
}
fn earlier_actions(ui: &mut egui::Ui, id: egui::Id, count: usize, expanded: &mut bool) -> bool {
    let colors = crate::theme::palette(ui.ctx());
    let (_, rect) = ui.allocate_space(Vec2::new(ui.available_width(), 22.0));
    let response = ui
        .interact(rect, id, Sense::click())
        .on_hover_cursor(egui::CursorIcon::PointingHand);
    let color = if response.hovered() || response.has_focus() {
        colors.text
    } else {
        colors.dim
    };
    icons::chevron(
        ui,
        Rect::from_center_size(rect.left_center() + egui::vec2(8.0, 0.0), Vec2::splat(12.0)),
        *expanded,
        color,
    );
    let label = format!(
        "{count} earlier {}",
        if count == 1 { "action" } else { "actions" }
    );
    ui.painter().text(
        rect.left_center() + egui::vec2(25.0, 0.0),
        egui::Align2::LEFT_CENTER,
        &label,
        FontId::proportional(11.0),
        color,
    );
    response.widget_info(|| {
        egui::WidgetInfo::labeled(egui::WidgetType::CollapsingHeader, ui.is_enabled(), &label)
    });
    let activate = response.clicked()
        || (response.has_focus()
            && ui.input(|i| i.key_pressed(egui::Key::Enter) || i.key_pressed(egui::Key::Space)));
    if activate {
        *expanded = !*expanded;
        response.request_focus();
    }
    activate
}
fn activity(
    ui: &mut egui::Ui,
    items: &[&crate::protocol::Tool],
    index: usize,
    running: bool,
    expand: bool,
    gallery: &mut crate::media::Gallery,
) -> bool {
    let colors = crate::theme::palette(ui.ctx());
    ui.add_space(4.0);
    let id = ui.make_persistent_id(("activity", index));
    let mut open = ui
        .ctx()
        .data(|d| d.get_temp::<bool>(id))
        .unwrap_or(running || expand);
    let before = open;
    let mut earlier_changed = false;
    let mut summary = Summary::default();
    let mut run_images: Vec<crate::media::Attachment> = Vec::new();
    for tool in items {
        summary.add(tool);
        for image in tool.all_images() {
            if !run_images.iter().any(|existing| existing.key == image.key) {
                run_images.push(image);
            }
        }
    }
    egui::Frame::new()
        .stroke(if open {
            Stroke::new(1.0, colors.border)
        } else {
            Stroke::NONE
        })
        .corner_radius(12)
        .inner_margin(if open {
            egui::Margin::symmetric(10, 6)
        } else {
            egui::Margin::ZERO
        })
        .show(ui, |ui| {
            if open {
                ui.spacing_mut().item_spacing.y = 2.0;
                ui.spacing_mut().interact_size.y = 22.0;
            }
            let (rect, _) = ui.allocate_exact_size(
                Vec2::new(ui.available_width(), if open { 24.0 } else { 27.0 }),
                Sense::hover(),
            );
            // Reserve space on the right for chevron, optional failed count, and up to 4 compact image thumbnails.
            let right_reserved_base = if summary.failed > 0 { 92.0 } else { 24.0 };
            let thumb_w = 38.0_f32;
            let thumb_h = 22.0_f32;
            let thumb_gap = 4.0_f32;
            let max_thumbs = if run_images.is_empty() {
                0
            } else {
                let avail_for_thumbs = (rect.width() - right_reserved_base - 140.0).max(0.0);
                ((avail_for_thumbs / (thumb_w + thumb_gap)).floor() as usize).min(4)
            };
            let shown_thumbs = run_images.len().min(max_thumbs);
            let extra_thumbs = run_images.len().saturating_sub(shown_thumbs);
            let extra_w = if extra_thumbs > 0 { 24.0 } else { 0.0 };
            let thumbs_total_w = if shown_thumbs > 0 {
                shown_thumbs as f32 * thumb_w
                    + shown_thumbs.saturating_sub(1) as f32 * thumb_gap
                    + extra_w
                    + 8.0
            } else {
                0.0
            };
            let thumbs_right = rect.right() - right_reserved_base;
            let thumbs_left = (thumbs_right - thumbs_total_w).max(rect.left() + 28.0);
            let toggle_left_rect = Rect::from_min_max(
                rect.min,
                egui::pos2(
                    if shown_thumbs > 0 {
                        thumbs_left
                    } else {
                        rect.right()
                    },
                    rect.max.y,
                ),
            );
            let toggle_left = ui
                .interact(toggle_left_rect, id.with("header"), Sense::click())
                .on_hover_cursor(egui::CursorIcon::PointingHand)
                .on_hover_text(summary.label());
            let toggle_right = if shown_thumbs > 0 {
                Some(
                    ui.interact(
                        Rect::from_min_max(egui::pos2(thumbs_right, rect.top()), rect.max),
                        id.with("header-chevron"),
                        Sense::click(),
                    )
                    .on_hover_cursor(egui::CursorIcon::PointingHand),
                )
            } else {
                None
            };
            icons::draw(
                ui,
                Rect::from_center_size(
                    rect.left_center() + egui::vec2(8.0, 0.0),
                    Vec2::splat(16.0),
                ),
                Icon::Spark,
                colors.accent,
            );
            let label = summary.label();
            let mut job = egui::text::LayoutJob::default();
            for (index, part) in label.split(" · ").enumerate() {
                if index > 0 {
                    job.append(
                        " · ",
                        0.0,
                        egui::TextFormat {
                            font_id: FontId::proportional(12.0),
                            color: colors.muted,
                            ..Default::default()
                        },
                    );
                }
                let (verb, count) = part.split_once(' ').unwrap_or((part, ""));
                job.append(
                    verb,
                    0.0,
                    egui::TextFormat {
                        font_id: FontId::proportional(12.0),
                        color: colors.muted,
                        ..Default::default()
                    },
                );
                if !count.is_empty() {
                    job.append(
                        &format!(" {count}"),
                        0.0,
                        egui::TextFormat {
                            font_id: FontId::new(12.0, egui::FontFamily::Name("body-bold".into())),
                            color: colors.muted,
                            ..Default::default()
                        },
                    );
                }
            }
            job.wrap.max_width =
                (rect.width() - right_reserved_base - thumbs_total_w - 30.0).max(20.0);
            job.wrap.max_rows = 1;
            let galley = ui.painter().layout_job(job);
            ui.painter().galley(
                rect.left_center() + egui::vec2(25.0, -galley.size().y / 2.0),
                galley,
                colors.muted,
            );
            if shown_thumbs > 0 {
                let mut cursor_x = thumbs_right - extra_w;
                if extra_thumbs > 0 {
                    ui.painter().text(
                        egui::pos2(thumbs_right - 4.0, rect.center().y),
                        egui::Align2::RIGHT_CENTER,
                        format!("+{extra_thumbs}"),
                        FontId::proportional(10.0),
                        colors.dim,
                    );
                }
                for rev_i in (0..shown_thumbs).rev() {
                    let thumb_rect = Rect::from_min_size(
                        egui::pos2(cursor_x - thumb_w, rect.center().y - thumb_h * 0.5),
                        Vec2::new(thumb_w, thumb_h),
                    );
                    cursor_x = thumb_rect.left() - thumb_gap;
                    if gallery
                        .compact_thumb(
                            ui,
                            thumb_rect,
                            &run_images[rev_i],
                            id.with(("thumb", rev_i)),
                        )
                        .clicked()
                    {
                        gallery.open(&run_images, rev_i);
                    }
                }
            }
            if summary.failed > 0 {
                ui.painter().text(
                    rect.right_center() - egui::vec2(25.0, 0.0),
                    egui::Align2::RIGHT_CENTER,
                    format!("{} failed", summary.failed),
                    FontId::proportional(10.0),
                    colors.red,
                );
            }
            icons::chevron(
                ui,
                Rect::from_center_size(
                    rect.right_center() - egui::vec2(7.0, 0.0),
                    Vec2::splat(12.0),
                ),
                open,
                colors.dim,
            );
            if toggle_left.clicked() || toggle_right.is_some_and(|r| r.clicked()) {
                open = !open;
                ui.ctx().data_mut(|d| d.insert_temp(id, open));
            }
            if open {
                ui.add(egui::Separator::default().spacing(1.0));
                let show_id = id.with("all");
                let mut all = ui
                    .ctx()
                    .data(|d| d.get_temp::<bool>(show_id))
                    .unwrap_or(false);
                let earlier = items.len().saturating_sub(5);
                if running && earlier > 0 && earlier_actions(ui, show_id, earlier, &mut all) {
                    ui.ctx().data_mut(|d| d.insert_temp(show_id, all));
                    earlier_changed = true;
                }
                let hidden = if running && !all { earlier } else { 0 };
                for (i, item) in items.iter().enumerate().skip(hidden) {
                    ui.push_id(i, |ui| {
                        crate::tool_view::row_with_gallery(ui, item, gallery)
                    });
                    if i + 1 < items.len() {
                        ui.add(egui::Separator::default().spacing(1.0));
                    }
                }
            }
        });
    ui.add_space(6.0);
    before != open || earlier_changed
}

pub fn time_label(timestamp: i64) -> String {
    time_label_at(timestamp, chrono::Local::now())
}

fn time_label_at(timestamp: i64, now: chrono::DateTime<impl chrono::TimeZone>) -> String {
    if timestamp == 0 {
        return String::new();
    }
    let Some(time) = chrono::DateTime::from_timestamp_millis(timestamp) else {
        return String::new();
    };
    let local = time.with_timezone(&now.timezone()).naive_local();
    local
        .format(if local.date() == now.date_naive() {
            "%H:%M"
        } else {
            "%Y-%m-%d %H:%M"
        })
        .to_string()
}

#[cfg(test)]
#[path = "transcript/agent_message_tests.rs"]
mod agent_message_tests;

#[cfg(test)]
#[path = "transcript/inline_media_tests.rs"]
mod inline_media_tests;

#[cfg(test)]
#[path = "transcript/reasoning_tests.rs"]
mod reasoning_tests;

#[cfg(test)]
#[path = "transcript/view_tests.rs"]
mod view_tests;

#[cfg(test)]
#[path = "transcript/retry_tests.rs"]
mod retry_tests;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn response_footer_uses_all_calls_for_both_cost_and_cache_rate() {
        fn labels(shape: &egui::Shape, out: &mut Vec<String>) {
            match shape {
                egui::Shape::Text(t) => out.push(t.galley.job.text.clone()),
                egui::Shape::Vec(shapes) => {
                    for shape in shapes {
                        labels(shape, out);
                    }
                }
                _ => {}
            }
        }
        let messages = [
            Message::from_json(&serde_json::json!({
                "role":"assistant", "content":[{"type":"text","text":"Working."}],
                "usage":{"input":15,"cacheRead":1187349,"cacheWrite":8446,"cost":{"total":0.2497498}}
            })),
            Message::from_json(&serde_json::json!({
                "role":"assistant", "content":[{"type":"text","text":"Done."}],
                "usage":{"input":3,"cacheRead":243947,"cacheWrite":205,"cost":{"total":0.0511654}}
            })),
        ];
        for mode in [crate::theme::Theme::Light, crate::theme::Theme::Dark] {
            let ctx = egui::Context::default();
            crate::theme::install(&ctx);
            mode.apply(&ctx);
            let mut gallery = crate::media::Gallery::default();
            let mut rendered = Vec::new();
            for _ in 0..3 {
                let output = ctx.run(
                    egui::RawInput {
                        screen_rect: Some(Rect::from_min_size(
                            egui::Pos2::ZERO,
                            egui::vec2(900.0, 600.0),
                        )),
                        ..Default::default()
                    },
                    |ctx| {
                        egui::CentralPanel::default().show(ctx, |ui| {
                            assistant(
                                ui,
                                &messages,
                                0,
                                "model",
                                "off",
                                (false, false, true, View::Normal),
                                false,
                                &mut gallery,
                                None,
                                &Arc::new(Mutex::new(None)),
                            );
                        });
                    },
                );
                rendered.clear();
                for shape in output.shapes {
                    labels(&shape.shape, &mut rendered);
                }
            }
            assert!(rendered.iter().any(|s| s.contains("$0.30")));
            assert!(rendered.iter().any(|s| s.contains("99.4% cached")));
            assert!(
                !rendered
                    .iter()
                    .any(|s| s.contains("100% cached") || s.contains("99.9% cached"))
            );
        }
    }

    #[test]
    fn compaction_is_a_bounded_separator_not_an_assistant_summary() {
        fn labels(shape: &egui::Shape, out: &mut Vec<(String, f32)>) {
            match shape {
                egui::Shape::Text(t) => out.push((t.galley.job.text.clone(), t.pos.y)),
                egui::Shape::Vec(shapes) => {
                    for shape in shapes {
                        labels(shape, out);
                    }
                }
                _ => {}
            }
        }
        for mode in [crate::theme::Theme::Light, crate::theme::Theme::Dark] {
            for width in [320.0, 900.0] {
                let ctx = egui::Context::default();
                crate::theme::install(&ctx);
                mode.apply(&ctx);
                let mut transcript = Transcript::default();
                let messages = vec![
                    Message {
                        role: "user".into(),
                        text: "Before compaction".into(),
                        ..Default::default()
                    },
                    Message {
                        role: "compaction".into(),
                        text: "HIDDEN SUMMARY".into(),
                        thinking: "HIDDEN THINKING".into(),
                        ..Default::default()
                    },
                    Message {
                        role: "assistant".into(),
                        text: "After compaction".into(),
                        ..Default::default()
                    },
                ];
                let mut rendered = Vec::new();
                for _ in 0..3 {
                    let output = ctx.run(
                        egui::RawInput {
                            screen_rect: Some(Rect::from_min_size(
                                egui::Pos2::ZERO,
                                egui::vec2(width, 850.0),
                            )),
                            ..Default::default()
                        },
                        |ctx| {
                            egui::CentralPanel::default().show(ctx, |ui| {
                                transcript.show(
                                    ui,
                                    &messages,
                                    "model",
                                    "high",
                                    false,
                                    "compaction-test",
                                );
                            });
                        },
                    );
                    rendered.clear();
                    for shape in output.shapes {
                        labels(&shape.shape, &mut rendered);
                    }
                }
                let y = |text: &str| rendered.iter().find(|(t, _)| t == text).unwrap().1;
                assert!(y("Before compaction") < y("Context compacted"));
                assert!(y("Context compacted") < y("After compaction"));
                assert!(!rendered.iter().any(|(t, _)| t.contains("HIDDEN")));
                assert_eq!(transcript.markers.len(), 1);
                assert_eq!(transcript.positions.len(), 3);
                assert!(transcript.message_action.is_none());

                let mut isolated = Transcript {
                    message_actions_enabled: true,
                    ..Default::default()
                };
                let output = ctx.run(
                    egui::RawInput {
                        screen_rect: Some(Rect::from_min_size(
                            egui::Pos2::ZERO,
                            egui::vec2(width, 850.0),
                        )),
                        ..Default::default()
                    },
                    |ctx| {
                        egui::CentralPanel::default().show(ctx, |ui| {
                            isolated.show(
                                ui,
                                &messages[1..2],
                                "model",
                                "high",
                                false,
                                "only-compaction",
                            );
                        });
                    },
                );
                let mut marker_labels = Vec::new();
                for shape in output.shapes {
                    labels(&shape.shape, &mut marker_labels);
                }
                assert_eq!(
                    marker_labels
                        .iter()
                        .map(|(t, _)| t.as_str())
                        .collect::<Vec<_>>(),
                    ["Context compacted"]
                );
                assert!(isolated.markers.is_empty());
                assert!(isolated.message_action.is_none());
            }
        }
    }

    #[test]
    fn chat_prose_lists_and_turns_use_compact_spacing_without_changing_form_defaults() {
        fn collect(shape: &egui::Shape, labels: &mut Vec<(String, f32)>) {
            match shape {
                egui::Shape::Text(text) => {
                    labels.push((text.galley.job.text.clone(), text.pos.y));
                }
                egui::Shape::Vec(shapes) => {
                    for shape in shapes {
                        collect(shape, labels);
                    }
                }
                _ => {}
            }
        }
        for theme in [crate::theme::Theme::Dark, crate::theme::Theme::Light] {
            let ctx = egui::Context::default();
            crate::theme::install(&ctx);
            theme.apply(&ctx);
            let mut transcript = Transcript::default();
            let messages = vec![
                Message {
                    role: "user".into(),
                    text: "A short prompt".into(),
                    ..Default::default()
                },
                Message {
                    role: "assistant".into(),
                    text: "First paragraph.\n\nSecond paragraph.\n\n- First item\n- Second item"
                        .into(),
                    ..Default::default()
                },
            ];
            for _ in 0..3 {
                let output = ctx.run(
                    egui::RawInput {
                        screen_rect: Some(Rect::from_min_size(
                            egui::Pos2::ZERO,
                            egui::vec2(900.0, 700.0),
                        )),
                        ..Default::default()
                    },
                    |ctx| {
                        egui::CentralPanel::default().show(ctx, |ui| {
                            transcript.show(ui, &messages, "model", "", false, "compact");
                            assert_eq!(ui.spacing().interact_size.y, crate::theme::CONTROL_HEIGHT);
                            assert_eq!(ui.spacing().item_spacing.y, 8.0);
                        });
                    },
                );
                let mut labels = Vec::new();
                for shape in output.shapes {
                    collect(&shape.shape, &mut labels);
                }
                let y = |needle: &str| {
                    labels
                        .iter()
                        .find(|(label, _)| label.contains(needle))
                        .unwrap()
                        .1
                };
                assert!(y("model") - y("A short prompt") < 100.0);
                assert!(y("First paragraph") - y("model") < 48.0);
                assert!(y("Second paragraph") - y("First paragraph") <= 32.0);
                assert!(y("Second item") - y("First item") <= 27.0);
            }
        }
    }

    #[test]
    fn timestamps_use_local_24_hour_time_and_date_only_outside_today() {
        let now = chrono::DateTime::parse_from_rfc3339("2026-01-01T00:15:00+05:30").unwrap();
        for (time, expected) in [
            ("2025-12-31T18:30:00Z", "00:00"),
            ("2026-01-01T08:04:00+05:30", "08:04"),
            ("2026-01-01T23:59:59+05:30", "23:59"),
            ("2025-12-31T18:29:59Z", "2025-12-31 23:59"),
            ("2026-01-02T00:00:00+05:30", "2026-01-02 00:00"),
            ("2025-01-01T00:15:00+05:30", "2025-01-01 00:15"),
        ] {
            let timestamp = chrono::DateTime::parse_from_rfc3339(time)
                .unwrap()
                .timestamp_millis();
            assert_eq!(time_label_at(timestamp, now), expected, "{time}");
        }
        let now = chrono::DateTime::parse_from_rfc3339("2026-01-01T23:30:00-07:00").unwrap();
        let timestamp = chrono::DateTime::parse_from_rfc3339("2026-01-02T06:04:00Z")
            .unwrap()
            .timestamp_millis();
        assert_eq!(time_label_at(timestamp, now), "23:04");
    }

    #[test]
    fn missing_or_invalid_timestamps_have_no_label() {
        for timestamp in [0, i64::MIN, i64::MAX] {
            assert_eq!(time_label(timestamp), "");
        }
    }

    const DIMENSION_NOTE: &str = "[Image: original 3024x1910, displayed at 2000x1263. Multiply coordinates by 1.51 to map to original image.]";
    fn image_message(text: String, count: usize) -> Message {
        Message {
            role: "user".into(),
            text,
            images: (0..count)
                .map(|_| {
                    crate::media::Attachment::from_bytes(include_bytes!("../assets/icon.png"))
                        .unwrap()
                })
                .collect(),
            ..Default::default()
        }
    }
    #[test]
    fn image_coordinate_notes_are_hidden_only_for_attached_trailing_metadata() {
        let text = format!("My actual question\n\n\n{DIMENSION_NOTE}\n{DIMENSION_NOTE}\n");
        let message = image_message(text.clone(), 2);
        assert_eq!(user_display_text(&message), "My actual question");
        assert_eq!(
            message.text, text,
            "presentation must never modify saved/model text"
        );
        assert_eq!(
            user_display_text(&image_message(DIMENSION_NOTE.into(), 1)),
            ""
        );
        assert_eq!(user_display_text(&image_message(text.clone(), 0)), text);
        assert_eq!(
            user_display_text(&image_message(text.clone(), 1)),
            text,
            "ambiguous excess notes are preserved"
        );
        for text in [format!("Please explain {DIMENSION_NOTE}"),format!("```\n{DIMENSION_NOTE}\n```"),format!("{DIMENSION_NOTE}\nThis is a user's explanation"),"[Image: original settings should stay unchanged]".into(),"[Image: original 0x1910, displayed at 2000x1263. Multiply coordinates by 1.51 to map to original image.]".into()] {
            assert_eq!(user_display_text(&image_message(text.clone(),1)),text);
        }
    }
    #[test]
    fn answer_card_remains_visible_before_continuation_without_expanding_tool_logs() {
        let ctx = egui::Context::default();
        crate::theme::install(&ctx);
        let raw = vec![
            serde_json::json!({"role":"assistant","content":[{"type":"toolCall","id":"q","name":"pui_ask_questions","arguments":{}}]}),
            serde_json::json!({"role":"toolResult","toolCallId":"q","content":[],"details":{"cancelled":false,"answers":[{"id":"scope","question":"Which scope?","answer":"Small"},{"id":"skip","question":"Anything else?","skipped":true}]}}),
            serde_json::json!({"role":"assistant","content":[{"type":"text","text":"Continuing below your answers."}]}),
        ];
        let messages = crate::protocol::parse_history(&raw);
        let mut gallery = crate::media::Gallery::default();
        fn labels(shape: &egui::Shape, out: &mut Vec<(String, f32)>) {
            match shape {
                egui::Shape::Text(t) => out.push((t.galley.job.text.clone(), t.pos.y)),
                egui::Shape::Vec(v) => {
                    for s in v {
                        labels(s, out);
                    }
                }
                _ => {}
            }
        }
        for running in [true, false] {
            let mut heights = vec![];
            for _ in 0..15 {
                let output = ctx.run(
                    egui::RawInput {
                        screen_rect: Some(Rect::from_min_size(
                            egui::Pos2::ZERO,
                            Vec2::new(900.0, 800.0),
                        )),
                        ..Default::default()
                    },
                    |ctx| {
                        egui::CentralPanel::default().show(ctx, |ui| {
                            let top = ui.cursor().top();
                            assistant(
                                ui,
                                &messages,
                                0,
                                "model",
                                "high",
                                (running, false, false, View::Normal),
                                false,
                                &mut gallery,
                                None,
                                &Arc::new(Mutex::new(None)),
                            );
                            heights.push(ui.cursor().top() - top);
                        });
                    },
                );
                let mut text = vec![];
                for s in output.shapes {
                    labels(&s.shape, &mut text);
                }
                let y = |label| {
                    text.iter()
                        .find(|(t, _)| t == label)
                        .unwrap_or_else(|| panic!("Missing {label}: {text:?}"))
                        .1
                };
                assert!(y("Your answers") < y("Which scope?"));
                assert!(y("Which scope?") < y("Small"));
                assert!(y("Small") < y("Skipped"));
                assert!(y("Skipped") < y("Continuing below your answers."));
                assert!(
                    !text.iter().any(|(t, _)| t == "Try again"),
                    "saved answers are read-only"
                );
            }
            assert!(
                heights[3..].iter().all(|h| (h - heights[3]).abs() < 0.5),
                "answer card height must stay stable"
            );
        }
    }

    #[test]
    fn short_user_bubbles_cover_their_action_footer_with_or_without_images() {
        fn collect(
            shape: &egui::Shape,
            fill: egui::Color32,
            bubbles: &mut Vec<Rect>,
            labels: &mut Vec<(String, Rect)>,
        ) {
            match shape {
                egui::Shape::Rect(r) if r.fill == fill => bubbles.push(r.rect),
                egui::Shape::Text(t) => labels.push((
                    t.galley.job.text.clone(),
                    t.galley.rect.translate(t.pos.to_vec2()),
                )),
                egui::Shape::Vec(shapes) => shapes
                    .iter()
                    .for_each(|s| collect(s, fill, bubbles, labels)),
                _ => {}
            }
        }
        for mode in [crate::theme::Theme::Dark, crate::theme::Theme::Light] {
            for zoom in [0.8, 1.0, 1.4] {
                for with_image in [false, true] {
                    let ctx = egui::Context::default();
                    crate::theme::install(&ctx);
                    mode.apply(&ctx);
                    ctx.set_zoom_factor(zoom);
                    let message = Message {
                        role: "user".into(),
                        text: "ok".into(),
                        timestamp: 60000,
                        images: if with_image {
                            vec![
                                crate::media::Attachment::from_bytes(include_bytes!(
                                    "../assets/icon.png"
                                ))
                                .unwrap(),
                            ]
                        } else {
                            vec![]
                        },
                        ..Default::default()
                    };
                    let mut gallery = crate::media::Gallery::default();
                    for _ in 0..3 {
                        let output = ctx.run(
                            egui::RawInput {
                                screen_rect: Some(Rect::from_min_size(
                                    egui::Pos2::ZERO,
                                    Vec2::new(420.0, 500.0),
                                )),
                                ..Default::default()
                            },
                            |ctx| {
                                egui::CentralPanel::default().show(ctx, |ui| {
                                    user_actions(ui, &message, &mut gallery, false);
                                });
                            },
                        );
                        let mut bubbles = vec![];
                        let mut labels = vec![];
                        for shape in &output.shapes {
                            collect(
                                &shape.shape,
                                crate::theme::palette(&ctx).user_bg,
                                &mut bubbles,
                                &mut labels,
                            );
                        }
                        let timestamp = labels
                            .iter()
                            .find(|(s, _)| s == &time_label(message.timestamp))
                            .unwrap()
                            .1;
                        let rewind = labels.iter().find(|(s, _)| s == "Rewind").unwrap().1;
                        assert!(
                            bubbles.iter().any(|r| r.left() <= timestamp.left() + 1.0
                                && r.right() >= rewind.right() - 1.0
                                && r.bottom() <= timestamp.top()),
                            "Bubble must span footer: {bubbles:?}, {labels:?}"
                        );
                    }
                }
            }
        }
    }

    #[test]
    fn user_footer_has_inline_copy_edit_rewind_with_matching_alignment_and_permissions() {
        fn labels(shape: &egui::Shape, out: &mut Vec<(String, Rect)>) {
            match shape {
                egui::Shape::Text(t) => out.push((
                    t.galley.job.text.clone(),
                    Rect::from_min_size(
                        t.pos
                            - egui::vec2(t.galley.size().x * t.galley.job.halign.to_factor(), 0.0),
                        t.galley.size(),
                    ),
                )),
                egui::Shape::Vec(v) => {
                    for s in v {
                        labels(s, out);
                    }
                }
                _ => {}
            }
        }
        for theme in [crate::theme::Theme::Dark, crate::theme::Theme::Light] {
            for enabled in [true, false] {
                let ctx = egui::Context::default();
                crate::theme::install(&ctx);
                theme.apply(&ctx);
                let mut actions = Vec::new();
                let mut frame = |events| {
                    ctx.run(
                        egui::RawInput {
                            screen_rect: Some(Rect::from_min_size(
                                egui::Pos2::ZERO,
                                Vec2::new(340.0, 200.0),
                            )),
                            events,
                            ..Default::default()
                        },
                        |ctx| {
                            egui::CentralPanel::default().show(ctx, |ui| {
                                if let Some(next) = user_footer(ui, "sent prompt", 60000, enabled) {
                                    actions.push(next);
                                }
                            });
                        },
                    )
                };
                frame(vec![]);
                let output = frame(vec![]);
                let mut text = vec![];
                for shape in output.shapes {
                    labels(&shape.shape, &mut text);
                }
                assert!(!text.iter().any(|(t, _)| t == "Message actions"));
                let timestamp = text
                    .iter()
                    .find(|(t, _)| t == &time_label(60000))
                    .unwrap()
                    .1;
                let mut previous = timestamp;
                for label in ["Copy", "Edit", "Rewind", "Fork"] {
                    let rect = text.iter().find(|(t, _)| t == label).expect(label).1;
                    assert!(
                        rect.left() > previous.right(),
                        "footer order must match assistant actions: {text:?}"
                    );
                    assert!((rect.center().y - timestamp.center().y).abs() < 0.5);
                    assert!((rect.height() - timestamp.height()).abs() < 0.5);
                    assert!(rect.right() < 340.0);
                    previous = rect;
                }
                for label in ["Copy", "Edit", "Rewind"] {
                    let pos = text.iter().find(|(t, _)| t == label).unwrap().1.center();
                    let mut copied = false;
                    for pressed in [true, false] {
                        let output = frame(vec![
                            egui::Event::PointerMoved(pos),
                            egui::Event::PointerButton {
                                pos,
                                button: egui::PointerButton::Primary,
                                pressed,
                                modifiers: egui::Modifiers::NONE,
                            },
                        ]);
                        copied |= output.platform_output.commands.iter().any(
                            |c| matches!(c, egui::OutputCommand::CopyText(t) if t == "sent prompt"),
                        );
                    }
                    if label == "Copy" {
                        assert!(copied, "Copy stays available during runs");
                    }
                }
                assert_eq!(
                    actions,
                    if enabled {
                        vec![MessageAction::Edit, MessageAction::Rewind]
                    } else {
                        vec![]
                    }
                );
            }
        }
    }
    #[test]
    fn native_user_bubble_omits_metadata_text_and_its_extra_height() {
        let ctx = egui::Context::default();
        crate::theme::install(&ctx);
        let clean = image_message("Actual user question".into(), 2);
        let annotated = image_message(
            format!("Actual user question\n\n{DIMENSION_NOTE}\n{DIMENSION_NOTE}"),
            2,
        );
        let mut gallery = crate::media::Gallery::default();
        let mut draw = |message: &Message| {
            let mut height = 0.0;
            let out = ctx.run(
                egui::RawInput {
                    screen_rect: Some(Rect::from_min_size(
                        egui::Pos2::ZERO,
                        Vec2::new(900.0, 600.0),
                    )),
                    ..Default::default()
                },
                |ctx| {
                    egui::CentralPanel::default().show(ctx, |ui| {
                        let top = ui.cursor().top();
                        user(ui, message, &mut gallery);
                        height = ui.cursor().top() - top;
                    });
                },
            );
            (out, height)
        };
        draw(&clean);
        let (_, clean_height) = draw(&clean);
        let (out, annotated_height) = draw(&annotated);
        fn collect(shape: &egui::Shape, text: &mut String) {
            match shape {
                egui::Shape::Text(t) => text.push_str(&t.galley.job.text),
                egui::Shape::Vec(v) => {
                    for s in v {
                        collect(s, text);
                    }
                }
                _ => {}
            }
        }
        let mut rendered = String::new();
        for shape in out.shapes {
            collect(&shape.shape, &mut rendered);
        }
        assert!(rendered.contains("Actual user question"));
        assert!(!rendered.contains("Multiply coordinates"));
        assert!(!rendered.contains("Image: original"));
        assert!(
            (clean_height - annotated_height).abs() < 0.1,
            "hidden notes must not leave empty bubble space"
        );
        assert_eq!(annotated.images.len(), 2);
    }
    #[test]
    fn replacing_preview_with_history_keeps_the_reading_anchor() {
        let ctx = egui::Context::default();
        crate::theme::install(&ctx);
        let make = |i| Message {
            role: if i % 2 == 0 { "user" } else { "assistant" }.into(),
            timestamp: i,
            text: format!("Message {i} {}", "body ".repeat(40)),
            ..Default::default()
        };
        let old: Vec<_> = (20..80).map(make).collect();
        let new: Vec<_> = (0..80).map(make).collect();
        let mut transcript = Transcript::default();
        let frame = |t: &mut Transcript, messages: &[Message]| {
            let _ = ctx.run(
                egui::RawInput {
                    screen_rect: Some(Rect::from_min_size(
                        egui::Pos2::ZERO,
                        egui::vec2(900.0, 600.0),
                    )),
                    ..Default::default()
                },
                |ctx| {
                    egui::CentralPanel::default().show(ctx, |ui| {
                        t.show(ui, messages, "model", "high", false, "anchor")
                    });
                },
            );
        };
        frame(&mut transcript, &old);
        frame(&mut transcript, &old);
        transcript.jump = Some(transcript.markers[5]);
        frame(&mut transcript, &old);
        frame(&mut transcript, &old);
        let index = transcript
            .positions
            .iter()
            .rev()
            .find(|(_, y)| *y <= transcript.scroll + 1.0)
            .unwrap()
            .0;
        let timestamp = old[index].timestamp;
        transcript.replace_history(&old, &new);
        frame(&mut transcript, &new);
        frame(&mut transcript, &new);
        let index = transcript
            .positions
            .iter()
            .rev()
            .find(|(_, y)| *y <= transcript.scroll + 1.0)
            .unwrap()
            .0;
        assert_eq!(
            new[index].timestamp, timestamp,
            "Reading position moved when full history arrived"
        );
    }
    #[test]
    fn image_heavy_history_does_not_decode_or_relayout_every_turn() {
        let ctx = egui::Context::default();
        crate::theme::install(&ctx);
        let mut messages: Vec<_> = (0..80)
            .map(|i| Message {
                role: if i % 2 == 0 { "user" } else { "assistant" }.into(),
                text: format!("message {i}"),
                images: if i % 2 == 0 {
                    vec![crate::media::Attachment {
                        key: i,
                        data: "invalid".into(),
                        mime: "image/png".into(),
                        file: None,
                        local_path: None,
                    }]
                } else {
                    Vec::new()
                },
                ..Default::default()
            })
            .collect();
        let mut transcript = Transcript::default();
        let mut totals = Vec::new();
        for frame in 0..24 {
            let events = if frame == 10 {
                vec![egui::Event::PointerButton {
                    pos: egui::pos2(500.0, 500.0),
                    button: egui::PointerButton::Primary,
                    pressed: false,
                    modifiers: egui::Modifiers::NONE,
                }]
            } else {
                Vec::new()
            };
            let _ = ctx.run(
                egui::RawInput {
                    screen_rect: Some(Rect::from_min_size(
                        egui::Pos2::ZERO,
                        egui::vec2(900.0, 600.0),
                    )),
                    events,
                    time: Some(frame as f64 / 60.0),
                    ..Default::default()
                },
                |ctx| {
                    egui::CentralPanel::default().show(ctx, |ui| {
                        transcript.show(ui, &messages, "model", "high", false, "images")
                    });
                },
            );
            totals.push(transcript.total);
            if frame > 2 {
                assert!(
                    transcript.rendered < 15,
                    "Unexpected whole-history layout: {}",
                    transcript.rendered
                );
            }
            std::thread::sleep(std::time::Duration::from_millis(2));
        }
        assert!(
            totals[2..].windows(2).all(|h| (h[0] - h[1]).abs() < 0.5),
            "Unstable image layout: {totals:?}"
        );
        // Avoid keeping a large synthetic vector alive during later parallel tests.
        messages.clear();
    }
    #[test]
    #[ignore = "local read-only history benchmark; set PUI_BENCH_SESSION"]
    fn local_history_layout_benchmark() {
        let Some(path) = std::env::var_os("PUI_BENCH_SESSION") else {
            return;
        };
        let started = std::time::Instant::now();
        let file = std::fs::read_to_string(path).unwrap();
        let entries: Vec<serde_json::Value> = file
            .lines()
            .filter_map(|l| serde_json::from_str::<serde_json::Value>(l).ok())
            .filter(|e| e["type"] != "session")
            .collect();
        let leaf = entries.last().and_then(|e| e["id"].as_str());
        let mut state = crate::protocol::State {
            messages: crate::history::from_entries(&entries, leaf),
            ..Default::default()
        };
        let compactions = state
            .messages
            .iter()
            .filter(|m| m.role == "compaction")
            .count();
        assert!(
            state
                .messages
                .iter()
                .filter(|m| m.role == "compaction")
                .all(|m| m.text.is_empty())
        );
        let first_user = entries
            .iter()
            .find(|e| e["message"]["role"] == "user")
            .unwrap();
        assert_eq!(
            state
                .messages
                .iter()
                .find(|m| m.role == "user")
                .unwrap()
                .text,
            crate::protocol::content_text(&first_user["message"]["content"])
        );
        // Full raw history, not just the context retained after compaction.
        eprintln!(
            "Recovered {} user prompts and {compactions} compaction boundaries",
            state.messages.iter().filter(|m| m.role == "user").count()
        );
        state.messages.shrink_to_fit();
        eprintln!(
            "Prepared {} messages in {:?}",
            state.messages.len(),
            started.elapsed()
        );
        let ctx = egui::Context::default();
        crate::theme::install(&ctx);
        let mut transcript = Transcript::default();
        let mut totals = Vec::new();
        let now = std::time::Instant::now();
        for frame in 0..20 {
            let _ = ctx.run(
                egui::RawInput {
                    screen_rect: Some(Rect::from_min_size(
                        egui::Pos2::ZERO,
                        egui::vec2(900.0, 750.0),
                    )),
                    time: Some(frame as f64 / 60.0),
                    ..Default::default()
                },
                |ctx| {
                    egui::CentralPanel::default().show(ctx, |ui| {
                        transcript.show(ui, &state.messages, "model", "high", false, "local")
                    });
                },
            );
            totals.push((transcript.total, transcript.scroll, transcript.rendered));
        }
        eprintln!("20 frames: {:?}; geometry {:?}", now.elapsed(), totals);
        assert!(
            totals[8..]
                .windows(2)
                .all(|w| (w[0].0 - w[1].0).abs() < 0.5),
            "Unstable history geometry"
        );
    }
    #[test]
    fn settled_transcript_geometry_stays_constant_for_many_frames() {
        let ctx = egui::Context::default();
        crate::theme::install(&ctx);
        let messages: Vec<_> = (0..120)
            .map(|i| Message {
                role: if i % 2 == 0 { "user" } else { "assistant" }.into(),
                text: format!("Message {i}\n\n{}", "paragraph ".repeat(80)),
                ..Default::default()
            })
            .collect();
        let mut transcript = Transcript::default();
        let mut totals = Vec::new();
        for frame in 0..30 {
            let _ = ctx.run(
                egui::RawInput {
                    screen_rect: Some(Rect::from_min_size(
                        egui::Pos2::ZERO,
                        egui::vec2(900.0, 600.0),
                    )),
                    time: Some(frame as f64 / 60.0),
                    ..Default::default()
                },
                |ctx| {
                    egui::CentralPanel::default().show(ctx, |ui| {
                        transcript.show(ui, &messages, "model", "high", false, "stable")
                    });
                },
            );
            totals.push((transcript.total, transcript.scroll, transcript.rendered));
        }
        let settled = &totals[10..];
        assert!(
            settled
                .windows(2)
                .all(|w| (w[0].0 - w[1].0).abs() < 0.5 && (w[0].1 - w[1].1).abs() < 0.5),
            "Unstable geometry: {settled:?}"
        );
    }
    #[test]
    fn progress_words_rotate_but_specific_states_remain_truthful() {
        assert_eq!(Progress::Working.label(0.0), "Thinking…");
        assert_eq!(Progress::Working.label(4.0), "Exploring…");
        assert_ne!(Progress::Working.label(8.0), Progress::Working.label(4.0));
        for elapsed in [0.0, 4.0, 12345.0] {
            assert_eq!(Progress::Compacting.label(elapsed), "Compacting context…");
            assert_eq!(
                Progress::Retrying.label(elapsed),
                "Provider interrupted · retrying…"
            );
            assert_eq!(Progress::Waiting.label(elapsed), "Waiting for input");
            assert_eq!(Progress::Connecting.label(elapsed), "Connecting…");
        }
    }
    #[test]
    fn earlier_action_disclosure_expands_collapses_and_reports_layout_changes() {
        let ctx = egui::Context::default();
        crate::theme::install(&ctx);
        let tools: Vec<_> = (0..9)
            .map(|i| crate::protocol::Tool {
                id: format!("earlier-{i}"),
                name: "read".into(),
                path: format!("src/file_{i}.rs"),
                finished: true,
                ..Default::default()
            })
            .collect();
        let items: Vec<_> = tools.iter().collect();
        let draw = |events: Vec<egui::Event>| {
            let mut changed = false;
            let mut height = 0.0;
            let output = ctx.run(
                egui::RawInput {
                    screen_rect: Some(Rect::from_min_size(
                        egui::Pos2::ZERO,
                        Vec2::new(800.0, 700.0),
                    )),
                    events,
                    ..Default::default()
                },
                |ctx| {
                    egui::CentralPanel::default().show(ctx, |ui| {
                        let top = ui.cursor().top();
                        let mut gallery = crate::media::Gallery::default();
                        changed = activity(ui, &items, 0, true, false, &mut gallery);
                        height = ui.cursor().top() - top;
                    });
                },
            );
            (output, changed, height)
        };
        draw(vec![]);
        let (output, changed, collapsed) = draw(vec![]);
        assert!(!changed);
        fn text(shape: &egui::Shape, rows: &mut Vec<(String, egui::Pos2)>) {
            match shape {
                egui::Shape::Text(t) => rows.push((t.galley.job.text.clone(), t.pos)),
                egui::Shape::Vec(shapes) => {
                    for shape in shapes {
                        text(shape, rows);
                    }
                }
                _ => {}
            }
        }
        let mut rows = Vec::new();
        for shape in output.shapes {
            text(&shape.shape, &mut rows);
        }
        assert!(!rows.iter().any(|(text, _)| text.starts_with("Show ")));
        let pos = rows
            .iter()
            .find(|(text, _)| text == "4 earlier actions")
            .unwrap()
            .1
            + egui::vec2(8.0, 5.0);
        let pointer = |pressed| {
            vec![
                egui::Event::PointerMoved(pos),
                egui::Event::PointerButton {
                    pos,
                    button: egui::PointerButton::Primary,
                    pressed,
                    modifiers: egui::Modifiers::NONE,
                },
            ]
        };
        draw(pointer(true));
        let (_, changed, expanded) = draw(pointer(false));
        assert!(
            changed,
            "expansion must invalidate this turn's height cache"
        );
        assert!(expanded > collapsed + 60.0);
        draw(pointer(true));
        let (_, changed, collapsed_again) = draw(pointer(false));
        assert!(changed);
        assert!((collapsed_again - collapsed).abs() < 0.1);
        let (_, changed, keyboard_expanded) = draw(vec![egui::Event::Key {
            key: egui::Key::Enter,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers: egui::Modifiers::NONE,
        }]);
        assert!(changed);
        assert!((keyboard_expanded - expanded).abs() < 0.1);
    }
    #[test]
    fn verbose_short_thinking_renders_markdown_with_a_label() {
        fn collect(shape: &egui::Shape, text: &mut Vec<egui::epaint::TextShape>) {
            match shape {
                egui::Shape::Text(t) => text.push(t.clone()),
                egui::Shape::Vec(shapes) => {
                    for shape in shapes {
                        collect(shape, text);
                    }
                }
                _ => {}
            }
        }
        let source = "## Research\n\n**Gathering information** from `README.md`.\n\n- Review docs\n\n```rust\nlet ready = true;\n```";
        for theme in [crate::theme::Theme::Light, crate::theme::Theme::Dark] {
            for width in [320.0, 800.0] {
                for running in [false, true] {
                    let ctx = egui::Context::default();
                    crate::theme::install(&ctx);
                    theme.apply(&ctx);
                    let draw = |events| {
                        let mut changed = false;
                        let output = ctx.run(
                            egui::RawInput {
                                screen_rect: Some(Rect::from_min_size(
                                    egui::Pos2::ZERO,
                                    egui::vec2(width, 800.0),
                                )),
                                events,
                                ..Default::default()
                            },
                            |ctx| {
                                egui::CentralPanel::default().show(ctx, |ui| {
                                    changed = assistant(
                                        ui,
                                        &[Message {
                                            role: "assistant".into(),
                                            blocks: vec![Block::Thinking(source.into())],
                                            ..Default::default()
                                        }],
                                        0,
                                        "model",
                                        "high",
                                        (running, false, false, View::Verbose),
                                        false,
                                        &mut crate::media::Gallery::default(),
                                        None,
                                        &Arc::new(Mutex::new(None)),
                                    )
                                    .0;
                                });
                            },
                        );
                        let mut text = Vec::new();
                        for shape in output.shapes {
                            collect(&shape.shape, &mut text);
                        }
                        (text, changed)
                    };
                    draw(vec![]);
                    let (text, _) = draw(vec![]);
                    assert!(text.iter().any(|t| t.galley.job.text == "Thinking"));
                    assert!(!text.iter().any(|t| t.galley.job.text == "Show more"));
                    for _ in 0..5 {
                        assert!(
                            !draw(vec![]).1,
                            "idle thinking must not cause a repaint loop"
                        );
                    }
                    for expected in [
                        "Research",
                        "Gathering information",
                        "README.md",
                        "Review docs",
                        "let ready = true;",
                    ] {
                        assert!(
                            text.iter().any(|t| t.galley.job.text.contains(expected)),
                            "missing {expected:?}: {:?}",
                            text.iter().map(|t| &t.galley.job.text).collect::<Vec<_>>()
                        );
                    }
                    for t in &text {
                        assert!(!t.galley.job.text.contains("**"));
                        assert!(!t.galley.job.text.contains('`'));
                        assert!(!t.galley.job.text.contains("##"));
                    }
                    let formats: Vec<_> = text
                        .iter()
                        .flat_map(|t| &t.galley.job.sections)
                        .map(|s| &s.format.font_id.family)
                        .collect();
                    assert!(formats.contains(&&egui::FontFamily::Name("body-bold".into())));
                    assert!(formats.contains(&&egui::FontFamily::Monospace));
                }
            }
        }
    }

    #[test]
    fn expanded_activity_keeps_eleven_rows_compact() {
        let ctx = egui::Context::default();
        crate::theme::install(&ctx);
        let tools: Vec<_> = (0..11)
            .map(|i| crate::protocol::Tool {
                id: format!("compact-{i}"),
                name: "edit".into(),
                path: format!("src/file_{i}.rs"),
                finished: true,
                ..Default::default()
            })
            .collect();
        let mut height = 0.0;
        let _ = ctx.run(
            egui::RawInput {
                screen_rect: Some(Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(800.0, 700.0),
                )),
                ..Default::default()
            },
            |ctx| {
                egui::CentralPanel::default().show(ctx, |ui| {
                    let before = ui.cursor().top();
                    let mut gallery = crate::media::Gallery::default();
                    activity(
                        ui,
                        &tools.iter().collect::<Vec<_>>(),
                        0,
                        false,
                        true,
                        &mut gallery,
                    );
                    height = ui.cursor().top() - before;
                });
            },
        );
        assert!(
            height <= 380.0,
            "Expanded activity is too padded: {height}px"
        );
    }

    #[test]
    fn activity_disclosure_opens_into_tool_rows() {
        let ctx = egui::Context::default();
        crate::theme::install(&ctx);
        let mut tools = Vec::new();
        for i in 0..6 {
            tools.push(crate::protocol::Tool {
                id: format!("t{i}"),
                name: "bash".into(),
                command: "echo fixture".into(),
                output: "fixture output".into(),
                finished: true,
                ..Default::default()
            });
        }
        let mut heights = Vec::new();
        let pos = egui::pos2(130.0, 32.0);
        for events in [
            vec![],
            vec![
                egui::Event::PointerMoved(pos),
                egui::Event::PointerButton {
                    pos,
                    button: egui::PointerButton::Primary,
                    pressed: true,
                    modifiers: egui::Modifiers::NONE,
                },
            ],
            vec![egui::Event::PointerButton {
                pos,
                button: egui::PointerButton::Primary,
                pressed: false,
                modifiers: egui::Modifiers::NONE,
            }],
            vec![],
        ] {
            let _ = ctx.run(
                egui::RawInput {
                    screen_rect: Some(Rect::from_min_size(
                        egui::Pos2::ZERO,
                        egui::vec2(800.0, 600.0),
                    )),
                    events,
                    ..Default::default()
                },
                |ctx| {
                    egui::CentralPanel::default().show(ctx, |ui| {
                        let before = ui.cursor().top();
                        let mut gallery = crate::media::Gallery::default();
                        activity(
                            ui,
                            &tools.iter().collect::<Vec<_>>(),
                            0,
                            false,
                            false,
                            &mut gallery,
                        );
                        heights.push(ui.cursor().top() - before);
                    });
                },
            );
        }
        assert!(
            heights.last().unwrap() > &(heights[0] + 120.0),
            "expanded activity should lay out tool rows: {heights:?}"
        );
    }
    #[test]
    fn rail_counts_only_user_messages_and_culls_offscreen_turns() {
        let ctx = egui::Context::default();
        crate::theme::install(&ctx);
        let messages: Vec<_> = (0..80)
            .map(|i| Message {
                role: if i % 2 == 0 { "user" } else { "assistant" }.into(),
                text: format!("Message {i}: a short body to lay out."),
                ..Default::default()
            })
            .collect();
        let mut transcript = Transcript::default();
        for _ in 0..4 {
            let _ = ctx.run(
                egui::RawInput {
                    screen_rect: Some(Rect::from_min_size(
                        egui::Pos2::ZERO,
                        egui::vec2(900.0, 600.0),
                    )),
                    ..Default::default()
                },
                |ctx| {
                    egui::CentralPanel::default().show(ctx, |ui| {
                        transcript.show(ui, &messages, "model", "high", false, "test")
                    });
                },
            );
        }
        assert_eq!(transcript.markers.len(), 40);
        assert!(
            transcript.rendered < 20,
            "Only visible messages should be laid out: {}",
            transcript.rendered
        );
        transcript.jump = Some(transcript.markers[5]);
        let _ = ctx.run(
            egui::RawInput {
                screen_rect: Some(Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(900.0, 600.0),
                )),
                ..Default::default()
            },
            |ctx| {
                egui::CentralPanel::default().show(ctx, |ui| {
                    transcript.show(ui, &messages, "model", "high", false, "test")
                });
            },
        );
        assert!(
            (transcript.scroll - transcript.markers[5]).abs() < 4.0,
            "scroll {}, marker {}, total {}",
            transcript.scroll,
            transcript.markers[5],
            transcript.total
        );
    }
    #[test]
    fn message_rail_has_only_marks_and_keeps_jump_wheel_and_keyboard_navigation() {
        for theme in [crate::theme::Theme::Dark, crate::theme::Theme::Light] {
            let ctx = egui::Context::default();
            crate::theme::install(&ctx);
            theme.apply(&ctx);
            let mut transcript = Transcript {
                total: 2000.0,
                viewport: 400.0,
                scroll: 500.0,
                markers: vec![100.0, 900.0, 1600.0],
                ..Default::default()
            };
            let rect = Rect::from_min_size(egui::pos2(10.0, 10.0), egui::vec2(24.0, 400.0));
            let render = |transcript: &mut Transcript, events: Vec<egui::Event>, wheel| {
                let modifiers = events
                    .iter()
                    .find_map(|event| match event {
                        egui::Event::Key { modifiers, .. } => Some(*modifiers),
                        _ => None,
                    })
                    .unwrap_or_default();
                ctx.run(
                    egui::RawInput {
                        modifiers,
                        events,
                        ..Default::default()
                    },
                    |ctx| {
                        egui::CentralPanel::default().show(ctx, |ui| {
                            if let Some(delta) = wheel {
                                ui.input_mut(|i| i.smooth_scroll_delta.y = delta);
                            }
                            transcript.rail(ui, rect);
                        });
                    },
                )
            };
            render(&mut transcript, vec![], None);
            let out = render(&mut transcript, vec![], None);
            let marks: Vec<_> = out.shapes.iter().filter(|s| matches!(&s.shape, egui::Shape::LineSegment { points, .. } if rect.contains(points[0]) && rect.contains(points[1]))).collect();
            assert_eq!(marks.len(), 3);
            assert!(
                !out.shapes.iter().any(
                    |s| matches!(&s.shape, egui::Shape::Rect(r) if rect.contains_rect(r.rect))
                ),
                "No track or scrollbar thumb in the message rail"
            );
            let click = |transcript: &mut Transcript, pos| {
                for pressed in [true, false] {
                    render(
                        transcript,
                        vec![
                            egui::Event::PointerMoved(pos),
                            egui::Event::PointerButton {
                                pos,
                                button: egui::PointerButton::Primary,
                                pressed,
                                modifiers: egui::Modifiers::NONE,
                            },
                        ],
                        None,
                    );
                }
            };
            click(&mut transcript, egui::pos2(25.0, 190.0));
            assert_eq!(
                transcript.jump.take(),
                Some(900.0),
                "mark jumps to its message"
            );
            click(&mut transcript, egui::pos2(25.0, 270.0));
            assert_eq!(
                transcript.jump.take(),
                None,
                "blank rail is not a scrollbar"
            );
            render(
                &mut transcript,
                vec![egui::Event::PointerMoved(egui::pos2(25.0, 270.0))],
                Some(-40.0),
            );
            assert_eq!(
                transcript.jump.take(),
                Some(540.0),
                "wheel scrolling over the rail still works"
            );
            render(
                &mut transcript,
                vec![egui::Event::Key {
                    key: egui::Key::End,
                    physical_key: None,
                    pressed: true,
                    repeat: false,
                    modifiers: egui::Modifiers::NONE,
                }],
                None,
            );
            assert_eq!(transcript.jump.take(), Some(1600.0));
            let focus = ctx.memory(|m| m.focused()).unwrap();
            for _ in 0..2 {
                for pressed in [true, false] {
                    render(
                        &mut transcript,
                        vec![
                            egui::Event::Key {
                                key: egui::Key::ArrowDown,
                                physical_key: None,
                                pressed,
                                repeat: false,
                                modifiers: egui::Modifiers {
                                    alt: true,
                                    ..Default::default()
                                },
                            },
                            egui::Event::PointerMoved(egui::pos2(300.0, 300.0)),
                        ],
                        Some(0.0),
                    );
                    assert!(
                        ctx.memory(|m| m.has_focus(focus)),
                        "rail retains arrow-key focus"
                    );
                }
                assert_eq!(transcript.jump.take(), Some(900.0));
            }
        }
    }

    #[test]
    fn mouse_wheel_still_scrolls_the_chat_without_a_scrollbar() {
        let ctx = egui::Context::default();
        crate::theme::install(&ctx);
        let messages: Vec<_> = (0..40)
            .map(|i| Message {
                role: "user".into(),
                text: format!("Message {i}"),
                ..Default::default()
            })
            .collect();
        let mut transcript = Transcript::default();
        let mut render = |events, time| {
            ctx.run(
                egui::RawInput {
                    events,
                    time: Some(time),
                    screen_rect: Some(Rect::from_min_size(
                        egui::Pos2::ZERO,
                        egui::vec2(900.0, 600.0),
                    )),
                    ..Default::default()
                },
                |ctx| {
                    egui::CentralPanel::default().show(ctx, |ui| {
                        transcript.show(ui, &messages, "model", "high", false, "wheel-test")
                    });
                },
            )
        };
        for i in 0..4 {
            render(vec![], i as f64 * 0.1);
        }
        render(
            vec![
                egui::Event::PointerMoved(egui::pos2(400.0, 300.0)),
                egui::Event::MouseWheel {
                    unit: egui::MouseWheelUnit::Point,
                    delta: egui::vec2(0.0, 150.0),
                    modifiers: egui::Modifiers::NONE,
                },
            ],
            0.5,
        );
        assert!(
            transcript.scroll < transcript.total - transcript.viewport - 20.0,
            "wheel must move the chat away from bottom"
        );
    }

    #[test]
    fn zoom_preserves_order_and_bounds() {
        let p = zoom_markers(&[10.0, 11.0, 12.0, 80.0], Some(11.0), 100.0);
        assert!(p.windows(2).all(|w| w[0] < w[1]));
        assert!(p.iter().all(|y| *y >= 0.0 && *y <= 100.0));
    }
    #[test]
    fn unhovered_markers_dont_move() {
        assert_eq!(zoom_markers(&[5.0, 7.0], None, 100.0), vec![5.0, 7.0]);
    }
}
