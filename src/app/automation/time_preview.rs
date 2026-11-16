//! Presentation conversion only: persisted schedules always remain UTC.
use super::*;
fn label<T: chrono::TimeZone>(seconds: i64, zone: &T) -> Option<String>
where
    T::Offset: std::fmt::Display,
{
    let utc = chrono::DateTime::from_timestamp(seconds, 0)?;
    let local = utc.with_timezone(zone);
    Some(format!(
        "{} · local {}",
        utc.format("%Y-%m-%d %H:%M UTC"),
        local.format("%Y-%m-%d %H:%M %:z")
    ))
}
pub(super) fn show(ui: &mut egui::Ui, times: &[i64]) {
    for time in times {
        ui.add(
            egui::Label::new(theme::small(
                ui,
                label(*time, &chrono::Local).unwrap_or_else(|| "Time unavailable".into()),
            ))
            .wrap(),
        );
    }
    if !times.is_empty() {
        ui.add(egui::Label::new(theme::small(ui,"Local is a display conversion using this Mac’s timezone. Cron still runs in UTC; local wall-clock time may shift with DST or timezone changes.")).wrap());
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn offsets_cross_date_boundaries_and_leave_utc_instant_unchanged() {
        let t = chrono::DateTime::parse_from_rfc3339("2026-01-01T23:30:00Z")
            .unwrap()
            .timestamp();
        assert_eq!(
            label(t, &chrono::FixedOffset::east_opt(19800).unwrap()).unwrap(),
            "2026-01-01 23:30 UTC · local 2026-01-02 05:00 +05:30"
        );
        assert!(
            label(t, &chrono::FixedOffset::west_opt(28800).unwrap())
                .unwrap()
                .ends_with("2026-01-01 15:30 -08:00")
        );
        assert!(label(i64::MAX, &chrono::Utc).is_none());
    }
    #[test]
    fn time_preview_narrow_themes_zoom_has_no_clipboard_or_state_changes() {
        for mode in [theme::Theme::Light, theme::Theme::Dark] {
            for zoom in [0.8, 1.4] {
                let ctx = egui::Context::default();
                theme::install(&ctx);
                mode.apply(&ctx);
                ctx.set_zoom_factor(zoom);
                let times = [1767310200, 1767396600, 1767483000];
                let output = ctx.run(
                    egui::RawInput {
                        screen_rect: Some(egui::Rect::from_min_size(
                            egui::Pos2::ZERO,
                            Vec2::new(420.0, 620.0),
                        )),
                        ..Default::default()
                    },
                    |ctx| {
                        egui::CentralPanel::default().show(ctx, |ui| show(ui, &times));
                    },
                );
                assert!(output.platform_output.commands.is_empty());
            }
        }
    }
}
