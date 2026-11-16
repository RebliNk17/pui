use super::*;
fn fixture() -> (Comments, diff_viewer::Document) {
    let diff = "@@ -1 +1 @@\n-old\n+new\n";
    let document: diff_viewer::Document = ("file.rs".into(), diff.into()).into();
    let mut notes = Comments::default();
    notes.bind(Path::new("/repo"), "owner", 1);
    notes.source_root = Some("/repo".into());
    assert!(notes.begin(
        "file.rs",
        diff,
        true,
        document.2.comment_digest(),
        super::super::tests::range(diff, "+new")
    ));
    notes.editor.as_mut().unwrap().text = "Original review note".into();
    assert!(notes.save());
    notes.items[0].stale = true;
    (notes, document)
}
#[test]
fn exact_original_only_recheck_refuses_working_changed_source_owner_and_range() {
    let (mut notes, document) = fixture();
    let id = notes.items[0].id;
    assert!(notes.can_recheck(id, "file.rs", true, &document.1, &document.2));
    assert!(!notes.can_recheck(id, "file.rs", false, &document.1, &document.2));
    assert!(!notes.can_recheck(id, "other.rs", true, &document.1, &document.2));
    let different: diff_viewer::Document =
        ("file.rs".into(), document.1.replace("+new", "+changed")).into();
    assert!(!notes.can_recheck(id, "file.rs", true, &different.1, &different.2));
    notes.items[0].anchor.selection.chars = 0..1;
    assert!(!notes.can_recheck(id, "file.rs", true, &document.1, &document.2));
}
#[test]
fn confirmation_cancel_themes_zoom_and_explicit_enter_clear_only_stale() {
    for mode in [theme::Theme::Light, theme::Theme::Dark] {
        for zoom in [0.8, 1.4] {
            for confirm in [false, true] {
                let (mut notes, document) = fixture();
                let id = notes.items[0].id;
                let text = notes.items[0].text.clone();
                let ctx = egui::Context::default();
                theme::install(&ctx);
                mode.apply(&ctx);
                ctx.set_zoom_factor(zoom);
                notes.recheck_requested = Some((id, document.2.focus_id()));
                let draw = |notes: &mut Comments, events| {
                    ctx.run(
                        egui::RawInput {
                            screen_rect: Some(egui::Rect::from_min_size(
                                egui::Pos2::ZERO,
                                Vec2::new(420.0, 620.0),
                            )),
                            events,
                            ..Default::default()
                        },
                        |ctx| notes.recheck_dialog(ctx, "file.rs", &document.1, true, &document.2),
                    )
                };
                for _ in 0..3 {
                    draw(&mut notes, vec![]);
                }
                let key = |key, modifiers| egui::Event::Key {
                    key,
                    modifiers,
                    pressed: true,
                    repeat: false,
                    physical_key: None,
                };
                if confirm {
                    draw(
                        &mut notes,
                        vec![key(egui::Key::Tab, egui::Modifiers::SHIFT)],
                    );
                    draw(&mut notes, vec![]);
                    draw(
                        &mut notes,
                        vec![key(egui::Key::Enter, egui::Modifiers::NONE)],
                    );
                } else {
                    draw(
                        &mut notes,
                        vec![key(egui::Key::Escape, egui::Modifiers::NONE)],
                    );
                }
                assert_eq!(notes.items[0].stale, !confirm);
                assert_eq!(notes.items[0].text, text);
                assert!(!notes.items[0].resolved);
                assert!(notes.recheck_requested.is_none());
            }
        }
    }
}
