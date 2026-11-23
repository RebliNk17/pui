//! Responsive layout for the composer's bottom toolbar.
//!
//! The left cluster (attach, model, effort, permission) and the right
//! cluster (mic, context ring, stop, send) must never overlap. As the
//! composer narrows, the model label truncates, then the effort label
//! collapses to its icon, then the pickers move to their own row above the
//! action row.

use super::model_picker::model_label;
use super::*;

/// Minimum free space kept between the left and right clusters.
const CLUSTER_GAP: f32 = 8.0;
/// Model width below which the effort label is collapsed first.
const MODEL_COMFORT: f32 = 110.0;
/// Width of the icon-only effort chip (sliders icon + chevron).
pub(super) const EFFORT_COMPACT: f32 = 36.0;
const MIC: f32 = 24.0;
const RING: f32 = 24.0;

#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) struct Widths {
    pub attach: f32,
    pub model: f32,
    pub model_min: f32,
    pub effort: f32,
    pub effort_compact: f32,
    pub permission: f32,
    pub right: f32,
    pub spacing: f32,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) struct Layout {
    /// Pickers render on their own row above attach/actions.
    pub wrap: bool,
    pub effort_compact: bool,
    pub model: f32,
}

pub(super) fn layout(available: f32, w: &Widths) -> Layout {
    let natural = w.model.max(w.model_min);
    let comfort = natural.min(MODEL_COMFORT).max(w.model_min);
    // Room left for the model chip on one row with everything else.
    let single_row = |effort: f32| {
        available - CLUSTER_GAP - w.right - w.attach - effort - w.permission - w.spacing * 4.0
    };
    let room = single_row(w.effort);
    if room >= comfort {
        return Layout {
            wrap: false,
            effort_compact: false,
            model: natural.min(room),
        };
    }
    let room = single_row(w.effort_compact);
    if room >= w.model_min {
        return Layout {
            wrap: false,
            effort_compact: true,
            model: natural.min(room),
        };
    }
    // Pickers get the full width on their own row.
    let row = |effort: f32| available - effort - w.permission - w.spacing * 2.0;
    let room = row(w.effort);
    let effort_compact = room < comfort;
    let room = if effort_compact {
        row(w.effort_compact)
    } else {
        room
    };
    Layout {
        wrap: true,
        effort_compact,
        model: natural.min(room).max(w.model_min),
    }
}

/// Shift a popup anchor horizontally so a `TOP_START` popup of `width`
/// stays inside `bounds` (the chat pane). Popups spilling over a docked
/// native browser would otherwise force the page to hide while open.
pub(super) fn fit_anchor(anchor: egui::Rect, width: f32, bounds: egui::Rect) -> egui::Rect {
    let left = anchor.left().min(bounds.right() - width).max(bounds.left());
    anchor.translate(Vec2::new(left - anchor.left(), 0.0))
}

impl Pui {
    pub(super) fn model_chip_widths(&self, ui: &egui::Ui) -> (f32, f32) {
        let colors = theme::palette(ui.ctx());
        let hint = if !self.state.provider.is_empty() {
            self.state.provider.as_str()
        } else {
            self.state.model.as_str()
        };
        let logo_pad = if icons::has_provider_logo(hint) {
            18.0
        } else {
            0.0
        };
        let natural = ui
            .painter()
            .layout_no_wrap(
                model_label(&self.state),
                FontId::proportional(11.0),
                colors.muted,
            )
            .size()
            .x
            + 22.0
            + logo_pad;
        (natural, (36.0 + logo_pad).max(45.0))
    }

    pub(super) fn effort_chip_width(&self, ui: &egui::Ui) -> f32 {
        let colors = theme::palette(ui.ctx());
        ui.painter()
            .layout_no_wrap(
                thinking_label(&self.state.thinking),
                FontId::proportional(11.0),
                colors.muted,
            )
            .size()
            .x
            + 40.0
    }

    fn composer_actions_width(&self, spacing: f32) -> f32 {
        let send = theme::COMPACT_HEIGHT;
        let mut width = send + spacing + MIC;
        if self.demo || crate::context::usage_fraction(&self.state).is_some() {
            width += spacing + RING;
        }
        if !self.state.ready() {
            width += spacing + theme::COMPACT_HEIGHT;
        }
        width
    }

    pub(super) fn composer_toolbar_layout(&self, ui: &egui::Ui, spacing: f32) -> Layout {
        let (model, model_min) = self.model_chip_widths(ui);
        let widths = Widths {
            attach: theme::COMPACT_HEIGHT,
            model,
            model_min,
            effort: self.effort_chip_width(ui),
            effort_compact: EFFORT_COMPACT,
            permission: self.permission_chip_width(ui),
            right: self.composer_actions_width(spacing),
            spacing,
        };
        layout(ui.available_width(), &widths)
    }

    pub(super) fn composer_pickers(&mut self, ui: &mut egui::Ui, layout: &Layout) {
        self.model_button_sized(ui, layout.model);
        self.effort_button_sized(ui, layout.effort_compact);
        self.permission_button(ui);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn widths() -> Widths {
        Widths {
            attach: 28.0,
            model: 160.0,
            model_min: 45.0,
            effort: 80.0,
            effort_compact: EFFORT_COMPACT,
            permission: 50.0,
            right: 112.0,
            spacing: 4.0,
        }
    }

    fn used(l: &Layout, w: &Widths) -> f32 {
        let effort = if l.effort_compact {
            w.effort_compact
        } else {
            w.effort
        };
        if l.wrap {
            return l.model + effort + w.permission + w.spacing * 2.0;
        }
        w.attach + l.model + effort + w.permission + w.right + w.spacing * 4.0 + CLUSTER_GAP
    }

    #[test]
    fn wide_composer_shows_everything_at_natural_width() {
        let l = layout(800.0, &widths());
        assert_eq!(
            l,
            Layout {
                wrap: false,
                effort_compact: false,
                model: 160.0
            }
        );
    }

    #[test]
    fn narrowing_degrades_in_order_without_overlap() {
        let w = widths();
        let mut last = (false, false);
        for available in (120..=800).rev().map(|x| x as f32) {
            let l = layout(available, &w);
            assert!(l.model >= w.model_min && l.model <= w.model);
            if !l.wrap || l.model > w.model_min {
                assert!(
                    used(&l, &w) <= available + 0.01,
                    "overflow at {available}: {l:?}"
                );
            }
            assert!(!(last.0 && !l.wrap), "unwrapped while narrowing");
            assert!(
                !(last.1 && !l.effort_compact && !l.wrap),
                "effort label returned on the same row while narrowing"
            );
            last = (l.wrap, l.effort_compact);
        }
        assert!(layout(250.0, &w).wrap);
    }

    #[test]
    fn screenshot_width_keeps_actions_clear_of_pickers() {
        // ~260pt toolbar from the reported narrow-window bug.
        let w = widths();
        let l = layout(260.0, &w);
        assert!(used(&l, &w) <= 260.0 || l.model == w.model_min);
    }

    #[test]
    fn popup_anchor_stays_inside_the_chat_pane() {
        let bounds = egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(530.0, 900.0));
        let button = egui::Rect::from_min_size(egui::pos2(270.0, 860.0), Vec2::new(40.0, 28.0));
        let fitted = fit_anchor(button, 275.0, bounds);
        assert_eq!(fitted.right() - fitted.left(), button.width());
        assert!(fitted.left() + 275.0 <= bounds.right());
        assert_eq!(fitted.top(), button.top());
        // Already fits: untouched. Pane narrower than popup: pinned left.
        let narrow = egui::Rect::from_min_size(egui::pos2(20.0, 860.0), Vec2::new(40.0, 28.0));
        assert_eq!(fit_anchor(narrow, 275.0, bounds), narrow);
        let tiny = egui::Rect::from_min_max(egui::pos2(10.0, 0.0), egui::pos2(200.0, 900.0));
        assert_eq!(fit_anchor(button, 275.0, tiny).left(), 10.0);
    }
}
