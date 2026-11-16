use super::*;
fn tempdir() -> std::io::Result<std::path::PathBuf> {
    let path = std::env::temp_dir().join(format!("pui-table-export-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir(&path)?;
    Ok(path)
}
#[test]
fn frozen_sorted_filtered_view_roundtrips_and_never_overwrites() {
    let (ctx, mut preview) = grid::tests::setup(
        "Name,Note\r\nBeta,=1+2\r\nAlpha,\"世界\n\"\"quote\"\"\"\r\nshort\r\n".into(),
        true,
    );
    projection::tests::apply(&ctx, &mut preview, "", projection::Order::Ascending);
    let index = preview.index.as_ref().unwrap();
    let body = serialize(
        &preview.source,
        index,
        preview.projection.rows(index.records.len(), true),
        b',',
    )
    .unwrap();
    assert_eq!(
        body,
        "Name,Note\nAlpha,\"世界\n\"\"quote\"\"\"\nBeta,=1+2\nshort\n"
    );
    let dir = tempdir().unwrap();
    let path = dir.join("new.csv");
    save_new(&path, &body).unwrap();
    assert_eq!(std::fs::read_to_string(&path).unwrap(), body);
    assert!(save_new(&path, "replacement").is_err());
    assert_eq!(std::fs::read_to_string(&path).unwrap(), body);
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(
            std::fs::metadata(&path).unwrap().permissions().mode() & 0o777,
            0o600
        );
    }
    projection::tests::apply(&ctx, &mut preview, "Beta", projection::Order::Original);
    let index = preview.index.as_ref().unwrap();
    let filtered = serialize(
        &preview.source,
        index,
        preview.projection.rows(index.records.len(), true),
        b'\t',
    )
    .unwrap();
    assert_eq!(filtered, "Name\tNote\nBeta\t=1+2\n");
    assert_ne!(body, filtered);
    assert_eq!(std::fs::read_to_string(path).unwrap(), body);
}
#[test]
fn destination_symlink_directory_missing_parent_relative_and_oversize_refuse() {
    let dir = tempdir().unwrap();
    assert!(save_new(&dir, "x").is_err());
    assert!(save_new(&dir.join("missing/file"), "x").is_err());
    assert!(save_new(Path::new("relative.csv"), "x").is_err());
    let too_big = dir.join("large");
    assert!(save_new(&too_big, &"x".repeat(MAX_OUTPUT + 1)).is_err());
    assert!(!too_big.exists());
    #[cfg(unix)]
    {
        let target = dir.join("target");
        std::fs::write(&target, "untouched").unwrap();
        let link = dir.join("link");
        std::os::unix::fs::symlink(&target, &link).unwrap();
        assert!(save_new(&link, "changed").is_err());
        assert_eq!(std::fs::read_to_string(target).unwrap(), "untouched");
    }
}
#[test]
fn native_menu_bounds_source_fallback_and_busy_state() {
    for mode in [theme::Theme::Light, theme::Theme::Dark] {
        for zoom in [0.8, 1.4] {
            let (ctx, mut preview) = grid::tests::setup("A,B\nx,y\n".into(), true);
            mode.apply(&ctx);
            ctx.set_zoom_factor(zoom);
            for source_view in [false, true] {
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
                            preview.export.disk_menu(
                                ui,
                                &preview.source,
                                preview.index.as_ref().ok().map(|index| index.as_ref()),
                                true,
                                &mut preview.projection,
                                source_view,
                                Path::new("/table.csv"),
                            )
                        });
                    },
                );
                for extension in ["csv", "tsv"] {
                    let (_, rect, enabled) = ctx
                        .data(|d| {
                            d.get_temp::<(egui::Id, egui::Rect, bool)>(egui::Id::new((
                                "table-export-disk",
                                extension,
                            )))
                        })
                        .unwrap();
                    assert_eq!(enabled, !source_view);
                    assert!(rect.right() <= 420.0);
                }
            }
            assert!(preview.export.disk.is_none());
        }
    }
}
#[test]
fn completion_after_preview_drop_is_bounded_and_reports_failures() {
    let mut export = Export::default();
    let (tx, rx) = mpsc::channel();
    export.disk = Some(rx);
    tx.send(Err("controlled write refusal".into())).unwrap();
    export.poll_disk();
    assert_eq!(export.notice(), Some("controlled write refusal"));
    assert!(export.disk.is_none());
    let (tx, rx) = mpsc::channel::<Result<(), String>>();
    drop(rx);
    assert!(tx.send(Ok(())).is_err());
}
