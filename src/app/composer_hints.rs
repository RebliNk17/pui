//! Empty-composer guidance. Paint overlays rather than reserving draft space.
use super::*;

const INTERVAL: f64 = 5.0;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Action {
    Send,
    Steer,
    Queue,
}
impl Action {
    fn label(self) -> &'static str {
        match self {
            Self::Send => "send",
            Self::Steer => "steer",
            Self::Queue => "queue",
        }
    }
    fn opposite(self) -> Self {
        match self {
            Self::Send => Self::Send,
            Self::Steer => Self::Queue,
            Self::Queue => Self::Steer,
        }
    }
}

#[derive(Default)]
pub(super) struct Hints {
    session: Option<String>,
    action: Option<Action>,
    index: usize,
    next_at: Option<f64>,
}
impl Hints {
    fn update(
        &mut self,
        session: Option<&str>,
        action: Action,
        now: f64,
        rotating: bool,
        count: usize,
    ) -> (usize, Option<Duration>) {
        if self.session.as_deref() != session || self.action != Some(action) {
            self.session = session.map(str::to_owned);
            self.action = Some(action);
            self.index = 0;
            self.next_at = None;
        }
        if !rotating {
            self.next_at = None;
            return (self.index, None);
        }
        let next_at = self.next_at.get_or_insert(now + INTERVAL);
        if now >= *next_at {
            self.index = (self.index + 1) % count;
            *next_at = now + INTERVAL;
        }
        (self.index, Some(Duration::from_secs_f64(*next_at - now)))
    }
}

impl Pui {
    pub(super) fn composer_hints(
        &mut self,
        ui: &egui::Ui,
        output: &egui::widgets::text_edit::TextEditOutput,
    ) {
        let ctx = ui.ctx();
        let response = &output.response;
        // TextEdit keeps its text margins even with frame(false). Use its actual
        // text origin/clip, not the surrounding scroll viewport, for overlays.
        let rect = output.text_clip_rect;
        let origin = output.galley_pos;
        let action = if self.state.ready() {
            Action::Send
        } else if self.prefs.settings.delivery == settings::DeliveryMode::Steer {
            Action::Steer
        } else {
            Action::Queue
        };
        let command = if ctx.os() == egui::os::OperatingSystem::Mac {
            "⌘"
        } else {
            "Ctrl+"
        };
        let prompts = if action == Action::Send {
            vec![
                "Message Pi…".to_owned(),
                "Shift+Enter for a new line…".to_owned(),
                "/ for commands · @ for files…".to_owned(),
            ]
        } else {
            vec![
                "Ask a follow-up…".to_owned(),
                format!("{command}Enter to {}…", action.opposite().label()),
                "Shift+Enter for a new line…".to_owned(),
            ]
        };
        let empty = self.draft.is_empty();
        let (now, window_focused) = ctx.input(|i| (i.time, i.focused));
        let preview = self.demo && !self.demo_composer_hints;
        let rotating = empty
            && !response.has_focus()
            && !preview
            && window_focused
            && ctx.style().animation_time > 0.0;
        let (index, repaint) = self.composer_hints.update(
            self.state.session_file.as_deref(),
            action,
            now,
            rotating,
            prompts.len(),
        );
        if let Some(delay) = repaint {
            ctx.request_repaint_after(delay);
        }
        if !empty {
            return;
        }
        response.widget_info(|| {
            egui::WidgetInfo::text_edit(
                response.enabled(),
                "",
                "",
                if preview {
                    "Start a real session to talk to Pi…".to_owned()
                } else {
                    format!(
                        "Message Pi. Enter to {}. {command}Enter to {}. Shift+Enter for a new line. / for commands, @ for files.",
                        action.label(),
                        action.opposite().label()
                    )
                },
            )
        });
        let colors = theme::palette(ctx);
        let painter = ui.painter().with_clip_rect(rect.intersect(ui.clip_rect()));
        if preview {
            let galley = painter.layout_no_wrap(
                "Start a real session to talk to Pi…".into(),
                FontId::proportional(14.0),
                colors.dim,
            );
            painter.galley(origin, galley, colors.dim);
            return;
        }
        let label = painter.layout_no_wrap(
            format!("to {}", action.label()),
            FontId::proportional(10.0),
            colors.dim,
        );
        let key = painter.layout_no_wrap("↵".into(), FontId::proportional(11.0), colors.muted);
        let key_size = egui::vec2(key.size().x + 10.0, 17.0);
        let hint_width = key_size.x + 6.0 + label.size().x;
        let key_rect = egui::Rect::from_min_size(
            egui::pos2((rect.right() - hint_width).max(origin.x), origin.y),
            key_size,
        );
        painter.rect(
            key_rect,
            3,
            colors.surface,
            Stroke::new(1.0, colors.border),
            egui::StrokeKind::Inside,
        );
        painter.galley(key_rect.center() - key.size() / 2.0, key, colors.muted);
        painter.galley(
            egui::pos2(
                key_rect.right() + 6.0,
                key_rect.center().y - label.size().y / 2.0,
            ),
            label,
            colors.dim,
        );
        // Truncate only the empty-state hint. Drafts retain the editor's full width.
        let width = (key_rect.left() - 16.0 - origin.x).max(0.0);
        if width > 24.0 {
            let mut job = egui::text::LayoutJob::simple_singleline(
                prompts[index].clone(),
                FontId::proportional(14.0),
                colors.dim,
            );
            job.wrap = egui::text::TextWrapping::truncate_at_width(width);
            let galley = painter.layout_job(job);
            painter.galley(origin, galley, colors.dim);
        }
    }
}

#[cfg(test)]
mod tests;
