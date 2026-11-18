use super::*;
use std::path::PathBuf;
fn owner() -> Owner {
    let ws = Workspace {
        is_git: true,
        branch: "main".into(),
        ..Default::default()
    };
    Owner::new(&(PathBuf::from("/root"), "session".into()), &ws).unwrap()
}
fn detail() -> workspace::history::details::Detail {
    workspace::history::details::Detail {
        oid: "b".repeat(40),
        time: 1,
        parents: vec![],
        message: "m".into(),
        signature: String::new(),
    }
}
#[test]
fn actual_image_activation_decodes_off_thread_uploads_once_and_releases_budget() {
    use std::io::Cursor;
    let root = std::fs::canonicalize(workspace::tests::fixture()).unwrap();
    let mut png = Cursor::new(Vec::new());
    ::image::DynamicImage::new_rgba8(2, 3)
        .write_to(&mut png, ::image::ImageFormat::Png)
        .unwrap();
    std::fs::write(root.join("preview.png"), png.into_inner()).unwrap();
    workspace::tests::git(&root, &["add", "preview.png"]);
    workspace::tests::git(
        &root,
        &[
            "-c",
            "user.name=Fixture",
            "-c",
            "user.email=fixture@example.invalid",
            "-c",
            "commit.gpgsign=false",
            "commit",
            "-qm",
            "image",
        ],
    );
    let oid = String::from_utf8(workspace::tests::git(&root, &["rev-parse", "HEAD"]))
        .unwrap()
        .trim()
        .to_owned();
    let ctx = egui::Context::default();
    theme::install(&ctx);
    let owner = Owner {
        root,
        identity: ("/fixture".into(), "s".into()),
        observed: "main".into(),
    };
    let detail = workspace::history::details::Detail { oid, ..detail() };
    let mut image = Image::default();
    let draw = |image: &mut Image| {
        ctx.run(
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    Vec2::new(420.0, 620.0),
                )),
                ..Default::default()
            },
            |ctx| {
                egui::CentralPanel::default().show(ctx, |ui| {
                    image.show(ui, &owner, &detail, false, "preview.png", 0)
                });
            },
        )
    };
    draw(&mut image);
    image.key = image.control;
    draw(&mut image);
    assert!(image.pending());
    let result = image
        .pending
        .as_ref()
        .unwrap()
        .recv_timeout(std::time::Duration::from_secs(10))
        .unwrap()
        .unwrap();
    image.pending = None;
    image.loaded = Some(result);
    draw(&mut image);
    assert!(image.textures[0].is_none());
    assert_eq!(image.textures[1].as_ref().unwrap().size(), [2, 3]);
    let id = image.textures[1].as_ref().unwrap().id();
    draw(&mut image);
    assert_eq!(image.textures[1].as_ref().unwrap().id(), id);
    drop(image);
    let all = Lease::new(&ctx);
    all.reserve(64 * 1024 * 1024).unwrap();
}
#[test]
fn image_diff_button_renders_enabled_for_image_path() {
    let ctx = egui::Context::default();
    theme::install(&ctx);
    let mut image = Image::default();
    let _ = ctx.run(
        egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                Vec2::new(420.0, 620.0),
            )),
            ..Default::default()
        },
        |ctx| {
            egui::CentralPanel::default().show(ctx, |ui| {
                image.show(ui, &owner(), &detail(), false, "img.png", 0);
            });
        },
    );
    let (_id, _rect, enabled) = ctx
        .data(|d| {
            d.get_temp::<(egui::Id, egui::Rect, bool)>(popup_id().with(("image-diff", "img.png")))
        })
        .unwrap();
    assert!(enabled);
}
#[test]
fn image_diff_hides_for_non_image_path() {
    let ctx = egui::Context::default();
    theme::install(&ctx);
    let mut image = Image::default();
    let _ = ctx.run(
        egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                Vec2::new(420.0, 620.0),
            )),
            ..Default::default()
        },
        |ctx| {
            egui::CentralPanel::default().show(ctx, |ui| {
                image.show(ui, &owner(), &detail(), false, "src/main.rs", 0);
            });
        },
    );
    assert!(ctx.data(|d| {
        d.get_temp::<(egui::Id, egui::Rect, bool)>(popup_id().with(("image-diff", "src/main.rs")))
            .is_none()
    }));
}
