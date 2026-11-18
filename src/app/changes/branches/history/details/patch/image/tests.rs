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
