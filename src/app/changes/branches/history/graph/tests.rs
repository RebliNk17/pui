use super::super::tests::fixture;
use super::*;
use crate::app::files::tests::frame;
#[test]
fn graph_connects_only_exact_adjacent_visible_first_parents() {
    let (_, mut app) = fixture();
    let history = &mut app.changes.branches.history;
    let snapshot = history.snapshot.as_mut().unwrap();
    assert!(connected(snapshot, &history.filter, 0));
    snapshot.commits[0].first_parent = Some("c".repeat(40));
    assert!(!connected(snapshot, &history.filter, 0));
    snapshot.commits[0].first_parent = Some("b".repeat(40));
    history.filter.project(snapshot);
    assert!(connected(snapshot, &history.filter, 0));
    snapshot.commits.remove(1);
    assert!(!connected(snapshot, &history.filter, 0));
}
#[test]
fn filtering_breaks_graph_lane_instead_of_linking_across_hidden_commits() {
    let (ctx, mut app) = fixture();
    ctx.memory_mut(|memory| memory.request_focus(super::super::filter::id()));
    frame(&ctx, &mut app, vec![egui::Event::Text("Synthetic".into())]);
    let history = &app.changes.branches.history;
    assert_eq!(history.commit_controls.len(), 1);
    assert!(!connected(
        history.snapshot.as_ref().unwrap(),
        &history.filter,
        0
    ));
    assert_eq!(history.requests, 0);
}
#[test]
fn graph_rows_native_themes_narrow_zoom_keep_readonly_controls_and_no_idle_jobs() {
    for mode in [theme::Theme::Light, theme::Theme::Dark] {
        for zoom in [0.8, 1.4] {
            let (ctx, mut app) = fixture();
            app.prefs.theme = mode;
            ctx.set_zoom_factor(zoom);
            let start = std::time::Instant::now();
            for _ in 0..15 {
                let mut native = eframe::Frame::_new_kittest();
                let output = ctx.run(
                    egui::RawInput {
                        screen_rect: Some(egui::Rect::from_min_size(
                            egui::Pos2::ZERO,
                            Vec2::new(420.0, 780.0),
                        )),
                        ..Default::default()
                    },
                    |ctx| eframe::App::update(&mut app, ctx, &mut native),
                );
                assert!(output.platform_output.commands.is_empty());
            }
            assert_eq!(app.changes.branches.history.commit_controls.len(), 2);
            assert!(app.changes.branches.history.pending.is_none());
            assert_eq!(app.changes.branches.history.requests, 0);
            eprintln!("15graphframes {:?}", start.elapsed());
        }
    }
}
