use super::*;
use std::{fs, path::Path};

fn git(root: &Path, args: &[&str]) -> Vec<u8> {
    crate::workspace::tests::git(root, args)
}

fn repo() -> PathBuf {
    let root = std::env::temp_dir().join(format!("pui-ui-stash-{}", uuid::Uuid::new_v4()));
    fs::create_dir(&root).unwrap();
    let root = fs::canonicalize(root).unwrap();
    git(&root, &["init", "--template=", "-b", "main"]);
    git(&root, &["config", "user.name", "Fixture"]);
    git(&root, &["config", "user.email", "fixture@example.invalid"]);
    fs::write(
        root.join("file.txt"),
        "base
",
    )
    .unwrap();
    git(&root, &["add", "file.txt"]);
    git(
        &root,
        &["-c", "commit.gpgsign=false", "commit", "-qm", "base"],
    );
    fs::write(
        root.join("file.txt"),
        "modified
",
    )
    .unwrap();
    root
}

fn frame(
    ctx: &egui::Context,
    changes: &mut Changes,
    workspace: &Workspace,
    width: f32,
    events: Vec<egui::Event>,
) -> egui::FullOutput {
    ctx.run(
        egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                Vec2::new(width, 760.0),
            )),
            events,
            ..Default::default()
        },
        |ctx| {
            changes.review_inputs(ctx, true, &[], 0, workspace);
            changes.show(ctx, &workspace.root, "session", workspace, (&[], 0), false);
        },
    )
}

#[test]
fn stash_dialog_fits_themes_zoom_and_escape_cancels_without_stashing() {
    let root = repo();
    let ws = workspace::inspect(&root);
    let review =
        workspace::stash::inspect_push(&root, "test note", &AtomicBool::new(false)).unwrap();

    for mode in [theme::Theme::Light, theme::Theme::Dark] {
        for (width, zoom) in [(360.0, 1.0), (420.0, 1.4)] {
            let ctx = egui::Context::default();
            theme::install(&ctx);
            mode.apply(&ctx);
            ctx.set_zoom_factor(zoom);
            let mut changes = Changes {
                index_writable: true,
                source: Source::Git,
                ..Default::default()
            };
            changes.bind(&root, "session");
            changes.stash.owner = Some(changes.identity.clone());
            changes.stash.review = Some(PendingReview::Push(review.clone()));

            for _ in 0..3 {
                frame(&ctx, &mut changes, &ws, width, vec![]);
            }
            let dialog = ctx
                .read_response(egui::Id::new("git-stash-confirm"))
                .unwrap()
                .rect;
            assert!(
                ctx.screen_rect().contains_rect(dialog),
                "dialog {dialog:?} outside screen at {width}/{zoom}"
            );
            frame(
                &ctx,
                &mut changes,
                &ws,
                width,
                vec![egui::Event::Key {
                    key: egui::Key::Escape,
                    physical_key: None,
                    pressed: true,
                    repeat: false,
                    modifiers: egui::Modifiers::NONE,
                }],
            );
            assert!(changes.stash.review.is_none());
            assert!(changes.stash.applying.is_none());
            assert!(
                workspace::stash::list(&root, &AtomicBool::new(false))
                    .unwrap()
                    .is_empty()
            );
        }
    }
}
