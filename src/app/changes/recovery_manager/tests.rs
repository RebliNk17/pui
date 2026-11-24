use super::*;
use sha2::{Digest, Sha256};
use std::{fs, path::Path};

fn git(root: &Path, args: &[&str]) -> Vec<u8> {
    crate::workspace::tests::git(root, args)
}

fn repo_with_backup() -> (PathBuf, PathBuf) {
    let root = std::env::temp_dir().join(format!("pui-ui-recmgr-{}", uuid::Uuid::new_v4()));
    fs::create_dir(&root).unwrap();
    let root = fs::canonicalize(root).unwrap();
    git(&root, &["init", "--template=", "-b", "main"]);
    git(&root, &["config", "user.name", "Fixture"]);
    git(&root, &["config", "user.email", "fixture@example.invalid"]);
    fs::write(
        root.join("file.txt"),
        "committed
",
    )
    .unwrap();
    git(&root, &["add", "file.txt"]);
    git(
        &root,
        &["-c", "commit.gpgsign=false", "commit", "-qm", "base"],
    );
    let content = b"saved draft
";
    let mut digest = Sha256::new();
    digest.update(b"file.txt");
    digest.update(Sha256::digest(content));
    let name = format!(
        "pui-discard-recovery-{}",
        crate::hex::lower(digest.finalize())
    );
    let backup = root.join(".git").join(name);
    fs::write(&backup, content).unwrap();
    (root, backup)
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
fn recovery_manager_dialogs_fit_themes_zoom_and_escape_cancels() {
    let (root, backup) = repo_with_backup();
    let ws = workspace::inspect(&root);
    let restore = workspace::recovery_manager::inspect_restore(
        &root,
        &backup,
        "file.txt",
        &AtomicBool::new(false),
    )
    .unwrap();
    let delete =
        workspace::recovery_manager::inspect_delete(&root, &backup, &AtomicBool::new(false))
            .unwrap();

    for mode in [theme::Theme::Light, theme::Theme::Dark] {
        for (width, zoom) in [(360.0, 1.0), (420.0, 1.4)] {
            for is_delete in [false, true] {
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
                changes.recovery_manager.owner = Some(changes.identity.clone());
                changes.recovery_manager.review = Some(if is_delete {
                    PendingReview::Delete(delete.clone())
                } else {
                    PendingReview::Restore(restore.clone())
                });

                for _ in 0..3 {
                    frame(&ctx, &mut changes, &ws, width, vec![]);
                }
                let dialog = ctx
                    .read_response(egui::Id::new("git-recovery-manager-confirm"))
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
                assert!(changes.recovery_manager.review.is_none());
                assert!(changes.recovery_manager.applying.is_none());
                assert!(backup.exists());
                assert_eq!(
                    fs::read(root.join("file.txt")).unwrap(),
                    b"committed
"
                );
            }
        }
    }
}
