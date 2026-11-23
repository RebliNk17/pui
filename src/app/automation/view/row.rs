//! Collapsed job row and its status/cadence summaries.
use super::*;
pub(super) fn job_counts(jobs: &[Job]) -> [usize; 3] {
    let mut counts = [0; 3];
    for job in jobs {
        counts[if job.completed {
            2
        } else if job.enabled {
            0
        } else {
            1
        }] += 1;
    }
    counts
}
pub(super) fn job_status(
    job: &Job,
    running: bool,
    colors: &theme::Palette,
) -> (&'static str, egui::Color32) {
    if job.dispatching {
        ("Delivering", colors.green)
    } else if job.error.is_some() {
        ("Needs review", colors.amber)
    } else if job.completed {
        ("Completed", colors.dim)
    } else if !job.enabled {
        ("Paused", colors.amber)
    } else if running {
        ("Checking", colors.green)
    } else if job.pending.is_some() {
        ("Waiting to deliver", colors.amber)
    } else {
        ("Active", colors.green)
    }
}
pub(super) fn next_activity(job: &Job, at: i64) -> String {
    if job.dispatching {
        "Sending to creator chat".into()
    } else if job.completed {
        format!("{} delivered", job.deliveries)
    } else if !job.enabled {
        "Not scheduled".into()
    } else if job.pending.is_some() {
        "Waiting for creator chat".into()
    } else {
        let prefix = if matches!(job.trigger, Trigger::Monitor { .. }) {
            "Check"
        } else {
            "Prompt"
        };
        let seconds = job.next_run.saturating_sub(at);
        if seconds <= 0 {
            format!("{prefix} due")
        } else if seconds < 60 {
            format!("{prefix} in {seconds}s")
        } else if seconds < 3600 {
            format!("{prefix} in {}m", (seconds + 59) / 60)
        } else {
            format!(
                "{prefix}: {}",
                chrono::DateTime::from_timestamp(job.next_run, 0)
                    .map(|t| t.format("%b %d, %H:%M UTC").to_string())
                    .unwrap_or_else(|| "Unavailable".into())
            )
        }
    }
}
pub(super) fn cadence(trigger: &Trigger) -> String {
    match trigger {
        Trigger::Monitor {
            interval_seconds: 60,
            ..
        } => "Monitor · Every minute".into(),
        Trigger::Monitor {
            interval_seconds, ..
        } => format!("Monitor · Every {interval_seconds}s"),
        Trigger::Cron { schedule } => {
            let fields: Vec<_> = schedule.split_whitespace().collect();
            if let [minute, hour, "*", "*", day] = fields.as_slice()
                && let (Ok(minute), Ok(hour)) = (minute.parse::<u8>(), hour.parse::<u8>())
                && minute < 60
                && hour < 24
                && let Some(day) = match *day {
                    "*" => Some("Daily"),
                    "1-5" => Some("Weekdays"),
                    "1" => Some("Mondays"),
                    "2" => Some("Tuesdays"),
                    "3" => Some("Wednesdays"),
                    "4" => Some("Thursdays"),
                    "5" => Some("Fridays"),
                    "6" => Some("Saturdays"),
                    "0" | "7" => Some("Sundays"),
                    _ => None,
                }
            {
                format!("Cron · {day} at {hour:02}:{minute:02} UTC")
            } else {
                format!("Cron · {schedule} · UTC")
            }
        }
    }
}
pub(super) fn row_text(
    ui: &egui::Ui,
    rect: egui::Rect,
    text: &str,
    size: f32,
    color: egui::Color32,
) {
    let mut layout = egui::text::LayoutJob::simple(
        text.into(),
        egui::FontId::proportional(size),
        color,
        rect.width().max(0.0),
    );
    layout.wrap.max_rows = 1;
    layout.wrap.break_anywhere = true;
    let galley = ui.painter().layout_job(layout);
    ui.painter().galley(rect.min, galley, color);
}
pub(super) fn job_row(ui: &mut egui::Ui, job: &Job, running: bool, open: bool) -> egui::Response {
    let colors = theme::palette(ui.ctx());
    let wide = ui.available_width() >= 650.0;
    let (rect, response) = ui.allocate_exact_size(
        Vec2::new(ui.available_width(), if wide { 72.0 } else { 88.0 }),
        Sense::click(),
    );
    response.widget_info(|| {
        egui::WidgetInfo::labeled(
            egui::WidgetType::Button,
            ui.is_enabled(),
            format!(
                "{} · {} details",
                job.name,
                if open { "Hide" } else { "Show" }
            ),
        )
    });
    if response.hovered() || response.has_focus() {
        ui.painter().rect_filled(rect, 6, colors.surface);
    }
    if response.has_focus() {
        ui.painter().rect_stroke(
            rect,
            6,
            Stroke::new(1.0_f32, colors.accent),
            egui::StrokeKind::Inside,
        );
    }
    let icon_rect = egui::Rect::from_center_size(
        egui::pos2(rect.left() + 14.0, rect.top() + 32.0),
        Vec2::splat(26.0),
    );
    ui.painter().rect_stroke(
        icon_rect,
        7,
        Stroke::new(1.0_f32, colors.border),
        egui::StrokeKind::Inside,
    );
    icons::draw(
        ui,
        icon_rect,
        if matches!(job.trigger, Trigger::Monitor { .. }) {
            Icon::Terminal
        } else {
            Icon::Clock
        },
        colors.muted,
    );
    let left = rect.left() + 40.0;
    let status_left = rect.right() - if wide { 310.0 } else { 126.0 };
    let line = |x, y, width| {
        egui::Rect::from_min_size(egui::pos2(x, rect.top() + y), Vec2::new(width, 18.0))
    };
    row_text(
        ui,
        line(left, 17.0, status_left - left - 12.0),
        &job.name,
        13.0,
        colors.text,
    );
    let cadence = cadence(&job.trigger);
    row_text(
        ui,
        line(
            left,
            38.0,
            if wide {
                status_left - left - 12.0
            } else {
                rect.right() - left - 28.0
            },
        ),
        &format!("{cadence} · {}", job.creator.name),
        11.0,
        colors.muted,
    );
    let (status, color) = job_status(job, running, colors);
    theme::status_dot(ui, egui::pos2(status_left, rect.top() + 25.0), color, 2.5);
    row_text(
        ui,
        line(status_left + 9.0, 18.0, if wide { 125.0 } else { 96.0 }),
        status,
        11.0,
        color,
    );
    let next = next_activity(job, now());
    row_text(
        ui,
        if wide {
            line(rect.right() - 166.0, 20.0, 140.0)
        } else {
            line(left, 59.0, rect.width() - 70.0)
        },
        &next,
        11.0,
        colors.muted,
    );
    let center = egui::pos2(rect.right() - 10.0, rect.top() + 32.0);
    let points = if open {
        vec![
            center + egui::vec2(-4.0, -2.0),
            center + egui::vec2(0.0, 2.0),
            center + egui::vec2(4.0, -2.0),
        ]
    } else {
        vec![
            center + egui::vec2(-2.0, -4.0),
            center + egui::vec2(2.0, 0.0),
            center + egui::vec2(-2.0, 4.0),
        ]
    };
    ui.painter()
        .add(egui::Shape::line(points, Stroke::new(1.2_f32, colors.dim)));
    response
        .on_hover_cursor(egui::CursorIcon::PointingHand)
        .on_hover_text(format!(
            "{}\n{cadence}\nCreated by {} ({})\n{next}\nClick to {} details",
            job.name,
            job.creator.name,
            job.creator.id,
            if open { "hide" } else { "show" }
        ))
}

pub(super) fn timestamp(seconds: i64) -> String {
    chrono::DateTime::from_timestamp(seconds, 0)
        .map(|t| t.format("%Y-%m-%d %H:%M UTC").to_string())
        .unwrap_or_else(|| "Unavailable".into())
}
