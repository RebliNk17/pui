//! Transcript progress row: quarter-turn spark vector animation and text shimmer.
use crate::icons::{self, Icon};
use eframe::egui::{
    self, Color32, FontId, Pos2, Rect, Sense, Shape, Stroke, TextFormat, Vec2, text::LayoutJob,
};
use std::time::Duration;

/// Bounded ~30 fps animation cadence while a visible active indicator is running.
const FRAME_INTERVAL: Duration = Duration::from_millis(33);
const TURN_CYCLE_SECS: f64 = 1.8;
const TURN_PORTION: f32 = 0.58;
const SHIMMER_CYCLE_SECS: f64 = 2.0;
const SHIMMER_PORTION: f32 = 0.72;
const SHIMMER_HALF_WIDTH: f32 = 0.26;
const PHRASE_PERIOD_SECS: f64 = 4.0;
const PHRASE_FADE_SECS: f32 = 0.24;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Progress {
    Interrupting,
    Working,
    Retrying,
    Compacting,
    Waiting,
    Connecting,
}

impl Progress {
    pub(super) fn label(self, elapsed: f64) -> &'static str {
        const WORDS: [&str; 12] = [
            "Thinking…",
            "Exploring…",
            "Pondering…",
            "Considering…",
            "Tinkering…",
            "Building…",
            "Connecting the dots…",
            "Working it out…",
            "Crafting…",
            "Untangling…",
            "Refining…",
            "Making progress…",
        ];
        match self {
            Self::Interrupting => "Interrupting current run…",
            Self::Working => WORDS[(elapsed.max(0.0) / PHRASE_PERIOD_SECS) as usize % WORDS.len()],
            Self::Retrying => "Provider interrupted · retrying…",
            Self::Compacting => "Compacting context…",
            Self::Waiting => "Waiting for input",
            Self::Connecting => "Connecting…",
        }
    }
}

fn ease_in_out_cubic(t: f32) -> f32 {
    let t = t.clamp(0.0, 1.0);
    if t < 0.5 {
        4.0 * t * t * t
    } else {
        1.0 - (-2.0 * t + 2.0).powi(3) / 2.0
    }
}

/// Returns `(rotation_radians, mid_turn_pulse)` for the quarter-turn cycle.
fn quarter_turn(elapsed: f64) -> (f32, f32) {
    let phase = ((elapsed % TURN_CYCLE_SECS) / TURN_CYCLE_SECS) as f32;
    let turn_frac = (phase / TURN_PORTION).clamp(0.0, 1.0);
    let rotation = ease_in_out_cubic(turn_frac) * std::f32::consts::FRAC_PI_2;
    let pulse = (turn_frac * std::f32::consts::PI).sin();
    (rotation, pulse)
}

/// Softly fades rotating `Working` words at phrase boundaries without delaying the initial word.
fn phrase_alpha(progress: Progress, elapsed: f64) -> f32 {
    if progress != Progress::Working || elapsed <= 0.5 {
        return 1.0;
    }
    let local = (elapsed % PHRASE_PERIOD_SECS) as f32;
    let period = PHRASE_PERIOD_SECS as f32;
    if local < PHRASE_FADE_SECS {
        (local / PHRASE_FADE_SECS).clamp(0.18, 1.0)
    } else if local > period - PHRASE_FADE_SECS {
        ((period - local) / PHRASE_FADE_SECS).clamp(0.18, 1.0)
    } else {
        1.0
    }
}

/// Normalized horizontal sweep center (`-0.22..1.22` during sweep, `2.0` during the calm pause).
fn shimmer_sweep(elapsed: f64) -> f32 {
    let phase = ((elapsed % SHIMMER_CYCLE_SECS) / SHIMMER_CYCLE_SECS) as f32;
    if phase < SHIMMER_PORTION {
        -0.22 + (phase / SHIMMER_PORTION) * 1.44
    } else {
        2.0
    }
}

fn spark_points(center: Pos2, outer_r: f32, inner_r: f32, rotation: f32) -> Vec<Pos2> {
    (0..8)
        .map(|i| {
            let r = if i % 2 == 0 { outer_r } else { inner_r };
            let angle =
                rotation + i as f32 * std::f32::consts::FRAC_PI_4 - std::f32::consts::FRAC_PI_2;
            let (sin, cos) = angle.sin_cos();
            Pos2::new(center.x + cos * r, center.y + sin * r)
        })
        .collect()
}

fn draw_animated_spark(ui: &egui::Ui, center: Pos2, color: Color32, peak: Color32, elapsed: f64) {
    let (rotation, pulse) = quarter_turn(elapsed);
    let scale = 0.94 + 0.11 * pulse;
    let spark_color = color.lerp_to_gamma(peak, 0.45 * pulse);
    let painter = ui.painter();

    // Soft concentric halo that blooms subtly mid-turn.
    painter.circle_filled(center, 8.6, spark_color.gamma_multiply(0.05 + 0.06 * pulse));
    painter.circle_filled(center, 5.4, spark_color.gamma_multiply(0.06 + 0.09 * pulse));

    let points = spark_points(center, 6.6 * scale, 2.4 * scale, rotation);

    // Non-convex star interior filled via a triangle fan from center (no seam hairlines).
    let fill = spark_color.gamma_multiply(0.08 + 0.14 * pulse);
    let mut mesh = egui::Mesh::default();
    mesh.colored_vertex(center, fill);
    for &point in &points {
        mesh.colored_vertex(point, fill);
    }
    for i in 0..8u32 {
        mesh.add_triangle(0, i + 1, (i + 1) % 8 + 1);
    }
    painter.add(Shape::mesh(mesh));

    painter.add(Shape::closed_line(
        points,
        Stroke::new(1.3_f32, spark_color.gamma_multiply(0.85 + 0.15 * pulse)),
    ));
}

fn draw_shimmer_label(
    ui: &egui::Ui,
    left_center: Pos2,
    label: &str,
    base: Color32,
    peak: Color32,
    alpha: f32,
    elapsed: f64,
) {
    let sweep = shimmer_sweep(elapsed);
    let count = label.chars().count().max(1) as f32;
    let font = FontId::proportional(12.0);
    let mut job = LayoutJob::default();
    job.wrap.max_width = f32::INFINITY;

    for (index, (start, ch)) in label.char_indices().enumerate() {
        let end = start + ch.len_utf8();
        let u = (index as f32 + 0.5) / count;
        let band = (1.0 - (u - sweep).abs() / SHIMMER_HALF_WIDTH).clamp(0.0, 1.0);
        let glow = band * band * (3.0 - 2.0 * band);
        let color = base.lerp_to_gamma(peak, glow).gamma_multiply(alpha);
        job.append(
            &label[start..end],
            0.0,
            TextFormat::simple(font.clone(), color),
        );
    }

    let painter = ui.painter();
    let galley = painter.layout_job(job);
    let pos = Pos2::new(left_center.x, left_center.y - galley.size().y * 0.5);
    painter.galley(pos, galley, base);
}

pub(super) fn row(ui: &mut egui::Ui, progress: Progress, elapsed: f64) {
    let colors = crate::theme::palette(ui.ctx());
    let color = if matches!(
        progress,
        Progress::Waiting | Progress::Interrupting | Progress::Retrying
    ) {
        colors.amber
    } else {
        colors.accent
    };
    let (rect, _) = ui.allocate_exact_size(Vec2::new(ui.available_width(), 24.0), Sense::hover());
    if !ui.is_rect_visible(rect) {
        return;
    }

    let elapsed = if elapsed.is_finite() {
        elapsed.max(0.0)
    } else {
        0.0
    };
    let spark_center = rect.left_center() + egui::vec2(9.0, 0.0);
    let text_anchor = rect.left_center() + egui::vec2(26.0, 0.0);
    let label = progress.label(elapsed);

    if progress == Progress::Waiting {
        // Vector glyph: native body fonts do not consistently contain sparkle symbols.
        icons::draw(
            ui,
            Rect::from_center_size(spark_center, Vec2::splat(16.0)),
            Icon::Spark,
            color,
        );
        ui.painter().text(
            text_anchor,
            egui::Align2::LEFT_CENTER,
            label,
            FontId::proportional(12.0),
            color,
        );
        return;
    }

    let dark = ui.visuals().dark_mode;
    let peak = color.lerp_to_gamma(Color32::WHITE, if dark { 0.58 } else { 0.44 });
    let text_base = if dark {
        color.lerp_to_gamma(colors.bg, 0.16)
    } else {
        color
    };

    draw_animated_spark(ui, spark_center, color, peak, elapsed);
    draw_shimmer_label(
        ui,
        text_anchor,
        label,
        text_base,
        peak,
        phrase_alpha(progress, elapsed),
        elapsed,
    );

    ui.ctx().request_repaint_after(FRAME_INTERVAL);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn paint(progress: Progress, elapsed: f64, visible: bool, dark: bool) -> egui::FullOutput {
        let ctx = egui::Context::default();
        crate::theme::install(&ctx);
        let mode = if dark {
            crate::theme::Theme::Dark
        } else {
            crate::theme::Theme::Light
        };
        mode.apply(&ctx);

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
                        if !visible {
                            ui.set_clip_rect(Rect::from_min_size(
                                Pos2::new(0.0, 300.0),
                                Vec2::new(400.0, 100.0),
                            ));
                        }
                        row(ui, progress, elapsed);
                    });
                },
            ));
        }
        output.unwrap()
    }

    fn collect_text(shape: &Shape, out: &mut Vec<(String, Vec<Color32>)>) {
        match shape {
            Shape::Text(text) => {
                let colors = text
                    .galley
                    .job
                    .sections
                    .iter()
                    .map(|section| section.format.color)
                    .collect();
                out.push((text.galley.job.text.clone(), colors));
            }
            Shape::Vec(shapes) => {
                for child in shapes {
                    collect_text(child, out);
                }
            }
            _ => {}
        }
    }

    #[test]
    fn quarter_turn_completes_smooth_90_degrees_then_rests() {
        let (start_rot, start_pulse) = quarter_turn(0.0);
        let (mid_rot, mid_pulse) = quarter_turn(TURN_CYCLE_SECS * f64::from(TURN_PORTION) * 0.5);
        let (rest_rot, rest_pulse) = quarter_turn(TURN_CYCLE_SECS * 0.85);

        assert!(start_rot.abs() < 1e-4);
        assert!(start_pulse.abs() < 1e-4);
        assert!((mid_rot - std::f32::consts::FRAC_PI_4).abs() < 1e-3);
        assert!((mid_pulse - 1.0).abs() < 1e-3);
        assert!((rest_rot - std::f32::consts::FRAC_PI_2).abs() < 1e-4);
        assert!(rest_pulse.abs() < 1e-4);
    }

    #[test]
    fn working_phrases_stay_full_opacity_initially_and_fade_only_at_transitions() {
        assert_eq!(phrase_alpha(Progress::Working, 0.0), 1.0);
        assert_eq!(phrase_alpha(Progress::Working, 0.2), 1.0);
        assert_eq!(phrase_alpha(Progress::Working, 2.0), 1.0);
        assert!(phrase_alpha(Progress::Working, 3.92) < 0.5);
        assert!(phrase_alpha(Progress::Working, 4.05) < 0.5);
        assert_eq!(phrase_alpha(Progress::Working, 4.5), 1.0);
        assert_eq!(phrase_alpha(Progress::Retrying, 3.92), 1.0);
    }

    #[test]
    fn active_visible_row_animates_with_bounded_timer_and_shimmer_gradient() {
        for dark in [true, false] {
            let output = paint(Progress::Working, 0.7, true, dark);
            let delay = output.viewport_output[&egui::ViewportId::ROOT].repaint_delay;
            assert!(
                delay <= FRAME_INTERVAL && delay > Duration::ZERO,
                "dark={dark} delay={delay:?}"
            );

            let mut texts = Vec::new();
            for clipped in &output.shapes {
                collect_text(&clipped.shape, &mut texts);
            }
            let (label, section_colors) = texts
                .into_iter()
                .find(|(text, _)| text == "Thinking…")
                .expect("progress label should render intact");
            assert_eq!(label, "Thinking…");
            let first = section_colors[0];
            assert!(
                section_colors.iter().any(|&c| c != first),
                "shimmer sweep should vary character colors across the label (dark={dark})"
            );
        }
    }

    #[test]
    fn waiting_and_offscreen_rows_never_schedule_repaints() {
        let waiting = paint(Progress::Waiting, 1.2, true, true);
        assert_eq!(
            waiting.viewport_output[&egui::ViewportId::ROOT].repaint_delay,
            Duration::MAX
        );

        let offscreen = paint(Progress::Working, 1.2, false, true);
        assert_eq!(
            offscreen.viewport_output[&egui::ViewportId::ROOT].repaint_delay,
            Duration::MAX
        );
    }

    #[test]
    fn spark_rotates_and_handles_amber_and_edge_elapsed_values() {
        let start = spark_points(Pos2::ZERO, 6.6, 2.4, quarter_turn(0.0).0);
        let mid = spark_points(
            Pos2::ZERO,
            6.6,
            2.4,
            quarter_turn(TURN_CYCLE_SECS * f64::from(TURN_PORTION) * 0.5).0,
        );
        assert!(
            (start[0].x - mid[0].x).abs() > 1.0 && (start[0].y - mid[0].y).abs() > 1.0,
            "spark tip should move smoothly during the quarter-turn"
        );

        for (progress, elapsed) in [
            (Progress::Retrying, 0.6),
            (Progress::Interrupting, 0.6),
            (Progress::Compacting, -3.0),
            (Progress::Connecting, f64::NAN),
            (Progress::Working, 1e9),
        ] {
            let output = paint(progress, elapsed, true, true);
            let delay = output.viewport_output[&egui::ViewportId::ROOT].repaint_delay;
            assert!(
                delay <= FRAME_INTERVAL && delay > Duration::ZERO,
                "{progress:?} at {elapsed} delay={delay:?}"
            );
        }
    }
}
