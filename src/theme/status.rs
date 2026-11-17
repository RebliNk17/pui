//! Tiny truthful activity indicators. Settled/offscreen rows never schedule animation.
use super::*;
use egui::{Pos2, Rect, Shape, Vec2};
use std::time::Duration;

pub(crate) fn is_working(status: &str) -> bool {
    matches!(
        status,
        "Working"
            | "Working…"
            | "Connecting"
            | "Connecting…"
            | "Changing history…"
            | "Compacting context…"
            | "Retrying provider…"
            | "Interrupting…"
            | "Proposing checks"
            | "Preparing workspace"
            | "Implementing"
            | "Fingerprinting candidate"
            | "Running checks"
            | "Independent review"
            | "Running"
    )
}

/// Draw a 12px activity ring, or the existing high-contrast settled-state dot.
/// The timer is bounded to 12.5 fps and only armed when this ring intersects the clip.
pub(crate) fn indicator(ui: &egui::Ui, center: Pos2, status: &str, color: Color32, radius: f32) {
    if !is_working(status) {
        status_dot(ui, center, color, radius);
        return;
    }
    let rect = Rect::from_center_size(center, Vec2::splat(14.0));
    if !ui.is_rect_visible(rect) {
        return;
    }
    let color = palette(ui.ctx()).accent;
    ui.painter()
        .circle_stroke(center, 5.0, Stroke::new(1.5, color.gamma_multiply(0.22)));
    // No generic Spinner: it requests an unrestricted repaint on every frame.
    let angle = ui.input(|input| input.time) as f32 * 4.0;
    let points: Vec<_> = (0..=16)
        .map(|index| {
            let phase = angle + index as f32 / 16.0 * std::f32::consts::PI * 1.35;
            center + egui::vec2(phase.cos(), phase.sin()) * 5.0
        })
        .collect();
    ui.painter()
        .add(Shape::line(points, Stroke::new(1.5, color)));
    ui.ctx().request_repaint_after(Duration::from_millis(80));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_observed_active_states_animate() {
        for status in [
            "Working…",
            "Connecting",
            "Compacting context…",
            "Retrying provider…",
            "Running checks",
        ] {
            assert!(is_working(status), "{status}");
        }
        for status in [
            "Idle",
            "Queued",
            "Waiting for input",
            "Saved",
            "Saved · status unknown",
            "Stopped",
            "Owner unavailable",
            "Interrupted · not resumed",
            "Completed · checks and review passed",
            "Unknown",
        ] {
            assert!(!is_working(status), "{status}");
        }
    }

    fn paint(status: &str, visible: bool) -> egui::FullOutput {
        let ctx = egui::Context::default();
        install(&ctx);
        let mut output = None;
        for frame in 0..3 {
            output = Some(ctx.run(
                egui::RawInput {
                    time: Some(frame as f64 + 1.0),
                    screen_rect: Some(Rect::from_min_size(Pos2::ZERO, egui::vec2(400.0, 200.0))),
                    ..Default::default()
                },
                |ctx| {
                    egui::CentralPanel::default().show(ctx, |ui| {
                        indicator(
                            ui,
                            egui::pos2(20.0, if visible { 20.0 } else { -100.0 }),
                            status,
                            palette(ctx).accent,
                            3.0,
                        );
                    });
                },
            ));
        }
        output.unwrap()
    }

    #[test]
    fn visible_ring_uses_a_bounded_timer_not_an_idle_repaint_loop() {
        let output = paint("Working…", true);
        let delay = output.viewport_output[&egui::ViewportId::ROOT].repaint_delay;
        // egui subtracts its predicted frame interval from the requested deadline.
        assert!((Duration::from_millis(50)..=Duration::from_millis(80)).contains(&delay));
        assert!(
            output
                .shapes
                .iter()
                .any(|shape| matches!(&shape.shape, Shape::Path(path) if path.points.len() == 17))
        );
        for (status, visible) in [
            ("Idle", true),
            ("Saved", true),
            ("Waiting for input", true),
            ("Working…", false),
        ] {
            let output = paint(status, visible);
            assert!(
                output.viewport_output[&egui::ViewportId::ROOT].repaint_delay
                    > Duration::from_secs(1),
                "{status}, visible={visible}"
            );
        }
    }
}
