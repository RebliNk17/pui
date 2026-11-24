use super::*;
use std::{fs, path::Path};

fn git(root: &Path, args: &[&str]) -> Vec<u8> {
    crate::workspace::tests::git(root, args)
}

fn conflicted_repo() -> PathBuf {
    let root = std::env::temp_dir().join(format!("pui-ui-op-abort-{}", uuid::Uuid::new_v4()));
    fs::create_dir(&root).unwrap();
    let root = fs::canonicalize(root).unwrap();
    git(&root, &["init", "--template=", "-b", "main"]);
    git(&root, &["config", "user.name", "Fixture"]);
    git(&root, &["config", "user.email", "fixture@example.invalid"]);
    fs::write(
        root.join("conflict.txt"),
        "base
",
    )
    .unwrap();
    git(&root, &["add", "--all"]);
    git(
        &root,
        &["-c", "commit.gpgsign=false", "commit", "-qm", "base"],
    );
    git(&root, &["branch", "other"]);
    fs::write(
        root.join("conflict.txt"),
        "ours
",
    )
    .unwrap();
    git(&root, &["add", "--all"]);
    git(
        &root,
        &["-c", "commit.gpgsign=false", "commit", "-qm", "ours"],
    );
    git(&root, &["switch", "-q", "other"]);
    fs::write(
        root.join("conflict.txt"),
        "theirs
",
    )
    .unwrap();
    git(&root, &["add", "--all"]);
    git(
        &root,
        &["-c", "commit.gpgsign=false", "commit", "-qm", "theirs"],
    );
    git(&root, &["switch", "-q", "main"]);
    let status = std::process::Command::new("git")
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .arg("-C")
        .arg(&root)
        .args([
            "-c",
            "core.hooksPath=/dev/null",
            "merge",
            "--no-edit",
            "other",
        ])
        .output()
        .unwrap()
        .status;
    assert!(!status.success());
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
fn abort_dialog_fits_themes_zoom_escape_cancels_and_confirm_aborts_merge() {
    let root = conflicted_repo();
    let ws = workspace::inspect(&root);
    assert!(ws.files.iter().any(|f| f.status == "UU"));

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
            let review = workspace::operation::inspect(&root, &AtomicBool::new(false)).unwrap();
            changes.operation.owner = Some(changes.identity.clone());
            changes.operation.review = Some(review);

            for _ in 0..3 {
                frame(&ctx, &mut changes, &ws, width, vec![]);
            }
            let dialog = ctx
                .read_response(egui::Id::new("git-operation-abort-confirm"))
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
            assert!(changes.operation.review.is_none());
            assert!(changes.operation.applying.is_none());
            assert!(root.join(".git/MERGE_HEAD").exists());
        }
    }

    // Real review -> confirm -> abort restores clean state
    let ctx = egui::Context::default();
    theme::install(&ctx);
    let mut changes = Changes {
        index_writable: true,
        source: Source::Git,
        ..Default::default()
    };
    changes.bind(&root, "session");
    changes.trigger_operation_abort_review(&ctx, &ws);
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
    while changes.operation.review.is_none() && std::time::Instant::now() < deadline {
        frame(&ctx, &mut changes, &ws, 800.0, vec![]);
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    assert!(changes.operation.review.is_some());
    for _ in 0..2 {
        frame(&ctx, &mut changes, &ws, 800.0, vec![]);
    }
    let out = frame(&ctx, &mut changes, &ws, 800.0, vec![]);
    let mut labels = Vec::new();
    fn collect(shape: &egui::Shape, out: &mut Vec<(String, egui::Rect)>) {
        match shape {
            egui::Shape::Text(t) => out.push((
                t.galley.job.text.clone(),
                egui::Rect::from_min_size(t.pos, t.galley.size()),
            )),
            egui::Shape::Vec(v) => v.iter().for_each(|s| collect(s, out)),
            _ => {}
        }
    }
    for s in out.shapes {
        collect(&s.shape, &mut labels);
    }
    let confirm_pos = labels
        .iter()
        .find(|(text, _)| text == "Abort operation")
        .unwrap()
        .1
        .center();
    for pressed in [true, false] {
        frame(
            &ctx,
            &mut changes,
            &ws,
            800.0,
            vec![
                egui::Event::PointerMoved(confirm_pos),
                egui::Event::PointerButton {
                    pos: confirm_pos,
                    button: egui::PointerButton::Primary,
                    pressed,
                    modifiers: egui::Modifiers::NONE,
                },
            ],
        );
    }
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
    while changes.operation.applying.is_some() && std::time::Instant::now() < deadline {
        frame(&ctx, &mut changes, &ws, 800.0, vec![]);
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    frame(&ctx, &mut changes, &ws, 800.0, vec![]);
    assert!(!root.join(".git/MERGE_HEAD").exists());
    assert!(changes.take_refresh());
}
