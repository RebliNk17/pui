use super::*;

fn values(source: &str, delimiter: u8) -> Vec<Vec<String>> {
    let table = parse(source, delimiter).unwrap();
    table
        .records
        .iter()
        .enumerate()
        .map(|(record, fields)| {
            (0..fields.len())
                .map(|column| table.value(source, record, column).to_owned())
                .collect()
        })
        .collect()
}

#[test]
fn csv_and_tsv_preserve_quoted_unicode_multiline_and_empty_values() {
    assert_eq!(
        values(
            "\u{feff}name,note,empty\r\n\"שלום,世界\",\"first\r\nsecond \"\"quoted\"\"\",\r\n",
            b','
        ),
        vec![
            vec!["name", "note", "empty"],
            vec!["שלום,世界", "first\r\nsecond \"quoted\"", ""],
        ]
    );
    assert_eq!(
        values("name\tnote\n\"a\tb\"\t=SUM(A1:A2)\n\t", b'\t'),
        vec![
            vec!["name", "note"],
            vec!["a\tb", "=SUM(A1:A2)"],
            vec!["", ""],
        ]
    );
    for (source, expected) in [
        ("", vec![]),
        ("\u{feff}", vec![]),
        ("\n", vec![vec![""]]),
        (",", vec![vec!["", ""]]),
        ("a\rb", vec![vec!["a"], vec!["b"]]),
    ] {
        assert_eq!(values(source, b','), expected, "{source:?}");
    }
}

#[test]
fn malformed_quoting_is_explicit_but_ragged_rows_keep_every_value() {
    for source in ["a,\"unclosed", "a\"b,c", "\"a\"x,b", "\"\"\""] {
        assert!(parse(source, b',').is_err(), "{source:?}");
    }
    let source = "a,b\nx\nx,y,z\n";
    let table = parse(source, b',').unwrap();
    assert!(table.ragged);
    assert_eq!(table.columns, 3);
    assert_eq!(table.value(source, 1, 1), "");
    assert_eq!(table.value(source, 2, 2), "z");
}

#[test]
fn parsing_limits_bound_records_columns_cells_and_bytes() {
    assert!(parse(&"x\n".repeat(MAX_RECORDS), b',').is_ok());
    assert!(
        parse(&"x\n".repeat(MAX_RECORDS + 1), b',')
            .unwrap_err()
            .contains("10,000 records")
    );
    assert!(parse(&vec!["x"; MAX_COLUMNS].join(","), b',').is_ok());
    assert!(
        parse(&vec!["x"; MAX_COLUMNS + 1].join(","), b',')
            .unwrap_err()
            .contains("128 columns")
    );
    let record = vec!["x"; 100].join(",") + "\n";
    assert!(parse(&record.repeat(1000), b',').is_ok());
    assert!(
        parse(&record.repeat(1001), b',')
            .unwrap_err()
            .contains("100,000 cells")
    );
    assert!(parse(&"x".repeat(MAX_TEXT + 1), b',').is_err());
}

#[test]
fn source_ranges_share_storage_and_escaped_fields_decode_once() {
    let source = "header,note\nplain,\"escaped \"\"quote\"\"\"\n";
    let table = parse(source, b',').unwrap();
    assert!(table.cells[2].decoded.is_none());
    assert_eq!(
        table.cells[2].value(source).as_ptr(),
        source[table.cells[2].range.clone()].as_ptr()
    );
    let value = table.cells[3].value(source);
    assert_eq!(value, "escaped \"quote\"");
    assert_eq!(value.as_ptr(), table.cells[3].value(source).as_ptr());
}

#[test]
fn large_table_lays_out_only_visible_rows_and_columns_and_settles() {
    for mode in [theme::Theme::Light, theme::Theme::Dark] {
        for zoom in [0.8, 1.0, 1.4] {
            let ctx = egui::Context::default();
            theme::install(&ctx);
            mode.apply(&ctx);
            ctx.set_zoom_factor(zoom);
            let source = (0..5000)
                .map(|row| format!("{row},hello,world,שלום,{},end\n", "long".repeat(12)))
                .collect::<String>();
            let mut preview = Preview::new(source, b',');
            assert!(preview.index.is_ok());
            let path = Path::new("large.csv");
            let mut final_output = None;
            for frame in 0..8 {
                final_output = Some(ctx.run(
                    egui::RawInput {
                        time: Some(frame as f64),
                        screen_rect: Some(egui::Rect::from_min_size(
                            egui::Pos2::ZERO,
                            Vec2::new(400.0, 600.0),
                        )),
                        ..Default::default()
                    },
                    |ctx| {
                        egui::CentralPanel::default().show(ctx, |ui| preview.show(ui, path));
                    },
                ));
                let painted = ctx
                    .data(|data| data.get_temp::<usize>(egui::Id::new("delimited-painted-cells")))
                    .unwrap();
                if frame >= 2 {
                    assert!(
                        painted > 0 && painted < 100,
                        "Expected visible-only work, got {painted}"
                    );
                }
            }
            assert!(
                final_output.unwrap().viewport_output[&egui::ViewportId::ROOT].repaint_delay
                    > Duration::from_secs(1),
                "Settled table must not request an animation timer"
            );
        }
    }
}

#[test]
fn opening_and_source_toggle_preserve_original_bytes_draft_and_file_access_policy() {
    let ctx = egui::Context::default();
    let mut app = super::super::tests::app(&ctx);
    let root = app.prefs.cwd.clone();
    let path = root.join("data.csv");
    let source = b"Name,Note\r\nAlpha,\"two, values\"\r\nBeta,=1+2\r\n";
    fs::write(&path, source).unwrap();
    app.draft = "Keep my draft".into();
    app.context_chips.clear_quotes();
    app.state.busy = true;
    let messages = app.state.messages.len();
    app.files
        .open_target(&ctx, &root, &path.display().to_string())
        .unwrap();
    let result = app.files.tabs[0]
        .rx
        .take()
        .unwrap()
        .recv_timeout(Duration::from_secs(5))
        .unwrap();
    assert!(
        matches!(&result, Ok(Content::Table(preview)) if preview.index.is_ok() && preview.source.as_bytes() == source)
    );
    app.files.tabs[0].content = Some(result);
    for _ in 0..3 {
        super::super::tests::frame(&ctx, &mut app, vec![]);
    }
    let output = super::super::tests::frame(&ctx, &mut app, vec![]);
    let source_button = super::super::tests::text(&output, "Source");
    super::super::tests::click(&ctx, &mut app, source_button);
    assert!(app.files.tabs[0].source);
    assert!(app.files.saved_layout().source_paths.contains(&path));
    assert_eq!(fs::read(&path).unwrap(), source);
    assert_eq!(app.outgoing_text(), "Keep my draft");
    assert!(app.state.busy);
    assert_eq!(app.state.messages.len(), messages);
    fs::write(root.join(".env.csv"), b"secret,value").unwrap();
    assert!(read_content(&root, &root.join(".env.csv"), false).is_err());
    let binary = root.join("binary.csv");
    fs::write(&binary, b"a,\0b").unwrap();
    assert!(read_content(&root, &binary, false).is_err());
    let huge = root.join("huge.csv");
    fs::write(&huge, vec![b'x'; MAX_TEXT + 1]).unwrap();
    assert!(read_content(&root, &huge, false).is_err());
}

#[test]
fn cell_context_actions_copy_and_quote_without_changing_file_or_sending() {
    for action in ["Copy cell", "Add cell to chat"] {
        let ctx = egui::Context::default();
        theme::install(&ctx);
        let mut preview = Preview::new("name,note\nAlpha,\"two, values\"\n".into(), b',');
        let mut draft_app = super::super::tests::app(&ctx);
        draft_app.draft = "Existing draft".into();
        draft_app.context_chips.clear_quotes();
        let messages = draft_app.state.messages.len();
        let mut draw = |events| {
            ctx.run(
                egui::RawInput {
                    screen_rect: Some(egui::Rect::from_min_size(
                        egui::Pos2::ZERO,
                        Vec2::new(620.0, 400.0),
                    )),
                    events,
                    ..Default::default()
                },
                |ctx| {
                    egui::CentralPanel::default()
                        .show(ctx, |ui| preview.show(ui, Path::new("sample.csv")));
                },
            )
        };
        for _ in 0..3 {
            draw(vec![]);
        }
        let target = ctx
            .data(|data| {
                data.get_temp::<egui::Rect>(egui::Id::new((
                    "delimited-cell",
                    2_usize,
                    2_usize,
                    false,
                )))
            })
            .unwrap()
            .center();
        for pressed in [true, false] {
            draw(vec![
                egui::Event::PointerMoved(target),
                egui::Event::PointerButton {
                    pos: target,
                    button: egui::PointerButton::Secondary,
                    pressed,
                    modifiers: egui::Modifiers::NONE,
                },
            ]);
        }
        let output = draw(vec![]);
        let button = super::super::tests::text(&output, action);
        let mut commands = Vec::new();
        for pressed in [true, false] {
            commands.extend(
                draw(vec![
                    egui::Event::PointerMoved(button),
                    egui::Event::PointerButton {
                        pos: button,
                        button: egui::PointerButton::Primary,
                        pressed,
                        modifiers: egui::Modifiers::NONE,
                    },
                ])
                .platform_output
                .commands,
            );
        }
        if action == "Copy cell" {
            assert!(commands.iter().any(|command| matches!(command, egui::OutputCommand::CopyText(text) if text == "two, values")));
        } else {
            draft_app.collect_context_capture();
            assert!(draft_app.outgoing_text().starts_with("Existing draft\n\n"));
            assert!(
                draft_app
                    .outgoing_text()
                    .contains("sample.csv · record 2, column 2")
            );
            assert!(draft_app.outgoing_text().contains("two, values"));
        }
        assert_eq!(draft_app.state.messages.len(), messages);
        assert_eq!(
            preview.source.as_str(),
            "name,note\nAlpha,\"two, values\"\n"
        );
    }
}

#[test]
fn scrolling_keeps_header_visible_and_reaches_last_record_and_column() {
    let ctx = egui::Context::default();
    theme::install(&ctx);
    let source = (0..200)
        .map(|r| {
            (0..20)
                .map(|c| format!("r{r}c{c}"))
                .collect::<Vec<_>>()
                .join(",")
                + "\n"
        })
        .collect();
    let mut preview = Preview::new(source, b',');
    let mut draw = |events| {
        ctx.run(
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    Vec2::new(450.0, 500.0),
                )),
                events,
                ..Default::default()
            },
            |ctx| {
                egui::CentralPanel::default()
                    .show(ctx, |ui| preview.show(ui, Path::new("scroll.csv")));
            },
        )
    };
    for _ in 0..4 {
        draw(vec![]);
    }
    let mut output = draw(vec![
        egui::Event::PointerMoved(egui::pos2(220.0, 300.0)),
        egui::Event::MouseWheel {
            unit: egui::MouseWheelUnit::Point,
            delta: egui::vec2(-100_000.0, -100_000.0),
            modifiers: egui::Modifiers::NONE,
        },
    ]);
    for _ in 0..12 {
        output = draw(vec![]);
    }
    super::super::tests::text(&output, "r0c19");
    super::super::tests::text(&output, "r199c19");
    let header = super::super::tests::text(&output, "r0c19");
    let last = super::super::tests::text(&output, "r199c19");
    assert!(header.y < last.y);
    let count = ctx
        .data(|data| data.get_temp::<usize>(egui::Id::new("delimited-painted-cells")))
        .unwrap();
    assert!(count < 100);
}

#[test]
fn generated_quoted_records_roundtrip_without_normalizing_values() {
    let options = [
        "",
        "שלום",
        "a,b",
        "tab\tinside",
        "line\nbreak",
        "cr\rbreak",
        "a\"b",
        "=1+2",
        "  whitespace  ",
    ];
    for delimiter in [b',', b'\t'] {
        for row in 0..80 {
            let values: Vec<_> = (0..7)
                .map(|column| options[(row * 3 + column) % options.len()])
                .collect();
            let encoded = values
                .iter()
                .map(|value| format!("\"{}\"", value.replace('"', "\"\"")))
                .collect::<Vec<_>>()
                .join(&(delimiter as char).to_string())
                + "\r\n";
            assert_eq!(super::tests::values(&encoded, delimiter), vec![values]);
        }
    }
}

#[test]
fn maximum_cell_index_has_bounded_retention_and_no_per_frame_parse() {
    let source = ((vec![""; 100].join(",")) + "\n").repeat(1000);
    let start = std::time::Instant::now();
    let preview = Preview::new(source, b',');
    let elapsed = start.elapsed();
    let index = preview.index.as_ref().unwrap();
    assert_eq!(index.cells.len(), MAX_CELLS);
    assert!(index.cells.iter().all(|cell| cell.decoded.is_none()));
    let bytes = preview.source.capacity()
        + preview.line_starts.capacity() * std::mem::size_of::<u32>()
        + index.cells.capacity() * std::mem::size_of::<Cell>()
        + index.records.capacity() * std::mem::size_of::<Range<usize>>();
    assert!(
        bytes < 5 * 1024 * 1024,
        "Bounded max-cell index retained {bytes} bytes"
    );
    eprintln!(
        "100,000-cell parse/index: {elapsed:?}, retained vector/source capacity {bytes} bytes"
    );
}

#[test]
fn source_view_is_virtualized_and_copy_preserves_exact_text() {
    let ctx = egui::Context::default();
    theme::install(&ctx);
    let source = "name,value\r\nAlpha,\"two\r\nlines\"\r\n".to_owned();
    let mut preview = Preview::new(source.clone(), b',');
    let mut draw = |events| {
        ctx.run(
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    Vec2::new(450.0, 400.0),
                )),
                events,
                ..Default::default()
            },
            |ctx| {
                egui::CentralPanel::default().show(ctx, |ui| {
                    preview.show_view(ui, Path::new("source.csv"), &mut true)
                });
            },
        )
    };
    for _ in 0..3 {
        draw(vec![]);
    }
    let id = ctx
        .data(|data| data.get_temp::<egui::Id>(egui::Id::new(("table-toolbar", 3usize))))
        .unwrap();
    let pos = ctx.read_response(id).unwrap().rect.center();
    for pressed in [true, false] {
        draw(vec![
            egui::Event::PointerMoved(pos),
            egui::Event::PointerButton {
                pos,
                pressed,
                button: egui::PointerButton::Primary,
                modifiers: egui::Modifiers::NONE,
            },
        ]);
    }
    let button = super::super::tests::text(&draw(vec![]), "Copy source");
    let mut commands = Vec::new();
    for pressed in [true, false] {
        commands.extend(
            draw(vec![
                egui::Event::PointerMoved(button),
                egui::Event::PointerButton {
                    pos: button,
                    button: egui::PointerButton::Primary,
                    pressed,
                    modifiers: egui::Modifiers::NONE,
                },
            ])
            .platform_output
            .commands,
        );
    }
    assert!(
        commands.iter().any(
            |command| matches!(command, egui::OutputCommand::CopyText(text) if text == &source)
        )
    );
    let big = Preview::new("a,b\n".repeat(20_000), b',');
    assert!(big.index.is_err());
    let mut output = None;
    for _ in 0..4 {
        output = Some(ctx.run(
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    Vec2::new(450.0, 400.0),
                )),
                ..Default::default()
            },
            |ctx| {
                egui::CentralPanel::default()
                    .show(ctx, |ui| big.show_source(ui, Path::new("big.csv")));
            },
        ));
    }
    let shapes = output.unwrap().shapes;
    let lines = shapes.iter().filter(|shape| matches!(&shape.shape, egui::Shape::Text(text) if text.galley.job.text == "a,b")).count();
    assert!(
        lines > 0 && lines < 40,
        "Source should only lay out visible lines, got {lines}"
    );
}

#[test]
fn header_can_be_disabled_without_losing_the_first_record() {
    let ctx = egui::Context::default();
    theme::install(&ctx);
    let mut preview = Preview::new("first,record\nsecond,record\n".into(), b',');
    preview.header = false;
    let mut output = None;
    for _ in 0..3 {
        output = Some(ctx.run(
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    Vec2::new(500.0, 400.0),
                )),
                ..Default::default()
            },
            |ctx| {
                egui::CentralPanel::default()
                    .show(ctx, |ui| preview.show(ui, Path::new("headerless.csv")));
            },
        ));
    }
    let output = output.unwrap();
    super::super::tests::text(&output, "Column 1");
    super::super::tests::text(&output, "first");
    super::super::tests::text(&output, "second");
    super::super::tests::text(&output, "2 rows");
}

#[test]
fn invalid_table_keeps_full_original_source_available() {
    let source = "name,value\n\"unterminated".to_owned();
    let preview = Preview::new(source.clone(), b',');
    assert!(preview.index.is_err());
    assert_eq!(preview.source.as_str(), source);
}
