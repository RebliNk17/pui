//! Compact last-activity lane; independent of optional metadata and live status.
use super::*;
use std::time::SystemTime;

fn label(time: Option<SystemTime>, now: SystemTime) -> String {
    let Some(time) = time.filter(|time| *time > SystemTime::UNIX_EPOCH) else {
        return "—".into();
    };
    let seconds = now.duration_since(time).unwrap_or_default().as_secs();
    match seconds {
        0..60 => "now".into(),
        60..3600 => format!("{}m", seconds / 60),
        3600..86400 => format!("{}h", seconds / 3600),
        86400..31_536_000 => format!("{}d", seconds / 86400),
        _ => format!("{}y", (seconds / 31_536_000).min(999)),
    }
}

pub(super) fn paint(ui: &mut egui::Ui, rect: egui::Rect, time: Option<SystemTime>) {
    let now = SystemTime::now();
    let time = time.filter(|time| *time > SystemTime::UNIX_EPOCH);
    let text = label(time, now);
    let lane = egui::Rect::from_center_size(
        egui::pos2(rect.right() - 48.0, rect.center().y),
        egui::vec2(40.0, 20.0),
    );
    if !ui.is_rect_visible(lane) {
        return;
    }
    ui.painter().text(
        lane.right_center(),
        egui::Align2::RIGHT_CENTER,
        &text,
        FontId::proportional(10.0),
        theme::palette(ui.ctx()).muted,
    );
    ui.interact(lane, ui.id().with("last-activity"), Sense::hover())
        .on_hover_ui(|ui| {
            let date = time.and_then(|time| time.duration_since(SystemTime::UNIX_EPOCH).ok())
                .and_then(|elapsed| i64::try_from(elapsed.as_secs()).ok().and_then(|secs|
                    chrono::DateTime::<chrono::Utc>::from_timestamp(secs, elapsed.subsec_nanos())));
            if let Some(date) = date {
                ui.label(format!("Last recorded activity: {}", date.with_timezone(&chrono::Local).format("%Y-%m-%d %H:%M:%S %Z")));
                ui.label("Session-file modification or observed Pi activity—not elapsed runtime.");
            } else {
                ui.label("Last activity unavailable. No supported timestamp has been observed for this row.");
            }
        });
    // Wake only a visible dated row at its next displayed unit boundary. Never
    // scan Git/files, repaint continuously, or animate unknown/offscreen rows.
    if let Some(time) = time {
        let seconds = now.duration_since(time).unwrap_or_default().as_secs();
        let unit = if seconds < 3600 {
            60
        } else if seconds < 86400 {
            3600
        } else {
            86400
        };
        ui.ctx()
            .request_repaint_after(Duration::from_secs(unit - seconds % unit));
    }
}

impl Pui {
    pub(in crate::app) fn demo_sidebar_density(&mut self) {
        self.projects = false;
        self.prefs.sidebar_view.group = sidebar_view::Group::Date;
        self.prefs.sidebar_view.metadata = false;
        self.prefs.hide_inspector = true;
        self.close_model_popovers();
        let now = SystemTime::now();
        let Some(session) = self.sessions.first() else {
            return;
        };
        self.state.session_file = Some(session.path.display().to_string());
        self.state.messages.clear();
        self.state.busy = false;
        let root = self.agents.active.clone();
        if let Some(node) = self.agents.nodes.get_mut(&root) {
            node.path = Some(session.path.clone());
            node.status = "Idle".into();
            node.last_activity = Some(now - Duration::from_secs(300));
            let children = node.children.clone();
            for (index, child) in children.iter().enumerate() {
                if let Some(node) = self.agents.nodes.get_mut(child) {
                    node.status = "Idle".into();
                    node.last_activity = Some(now - Duration::from_secs(120 + index as u64 * 60));
                }
            }
        }
        if let Some(session) = self.sessions.first_mut() {
            session.modified = now - Duration::from_secs(300);
        }
        self.focus_composer = false;
    }
}

#[cfg(test)]
mod idle_tests;
#[cfg(test)]
mod tests;
