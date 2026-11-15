use super::*;
fn svg(body: &str) -> String {
    format!("<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"16\" height=\"8\">{body}</svg>")
}
fn raster(source: &str) -> Raster {
    let (tree, width, height) = policy::prepare(source).unwrap();
    let mut pixels = resvg::tiny_skia::Pixmap::new(width, height).unwrap();
    resvg::render(
        &tree,
        resvg::tiny_skia::Transform::identity(),
        &mut pixels.as_mut(),
    );
    Raster {
        size: [width as usize, height as usize],
        rgba: pixels.data().to_vec(),
    }
}
#[test]
fn supported_shapes_path_transform_solid_paints_and_premultiplied_alpha() {
    let image = raster(&svg(
        "<g transform=\"translate(2 0)\"><path d=\"M0 0H8V8H0Z\" fill=\"#ff0000\" fill-opacity=\"0.5\"/></g>",
    ));
    assert_eq!(image.size, [16, 8]);
    let at = |x: usize, y: usize| &image.rgba[(y * 16 + x) * 4..(y * 16 + x + 1) * 4];
    assert_eq!(at(0, 4), [0, 0, 0, 0]);
    assert_eq!(at(5, 4), [128, 0, 0, 128]);
    assert_eq!(at(14, 4), [0, 0, 0, 0]);
    raster(&svg(
        "<rect width=\"4\" height=\"4\"/><circle cx=\"8\" cy=\"4\" r=\"2\"/><ellipse cx=\"12\" cy=\"4\" rx=\"2\" ry=\"1\"/><line x1=\"0\" y1=\"1\" x2=\"4\" y2=\"1\" stroke=\"blue\"/><polygon points=\"0,0 2,0 1,2\"/><polyline points=\"0,0 2,1\"/>",
    ));
}
#[test]
fn reject_active_resources_namespaces_effects_and_any_unsupported_content() {
    for body in [
        "<script/>",
        "<style/>",
        "<text/>",
        "<image href=\"file:///tmp/private\"/>",
        "<image href=\"data:image/svg+xml;base64,AA==\"/>",
        "<image href=\"https://localhost/private\"/>",
        "<use href=\"#a\"/>",
        "<defs/>",
        "<filter/>",
        "<mask/>",
        "<clipPath/>",
        "<foreignObject/>",
        "<a/>",
        "<animate/>",
        "<svg/>",
        "<other:rect xmlns:other=\"urn:other\"/>",
        "<g opacity=\"0.5\"/>",
        "<rect style=\"fill:red\"/>",
        "<rect fill=\"url(&#35;paint)\"/>",
        "<rect onload=\"x\"/>",
        "<rect stroke-dasharray=\"1,2\"/>",
        "<rect><circle r=\"2\"/></rect>",
        "<g xmlns=\"\"><rect width=\"4\" height=\"4\"/></g>",
        "<rect xlink:href=\"x\" xmlns:xlink=\"http://www.w3.org/1999/xlink\"/>",
    ] {
        assert!(policy::prepare(&svg(body)).is_err(), "{body}");
    }
    for source in [
        format!("<!DOCTYPE svg []>{}", svg("")),
        format!(
            "<!DOCTYPE svg [<!ENTITY x SYSTEM 'file:///tmp/private'>]>{}",
            svg("&x;")
        ),
        format!("<?xml-stylesheet href='https://localhost/x'?>{}", svg("")),
    ] {
        assert!(policy::prepare(&source).is_err());
    }
}
#[test]
fn utf8_declarations_and_viewbox_aspect_are_not_guessed() {
    policy::prepare(&format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>{}",
        svg("")
    ))
    .unwrap();
    assert!(
        policy::prepare(&format!(
            "<?xml version=\"1.0\" encoding=\"ISO-8859-1\"?>{}",
            svg("")
        ))
        .is_err()
    );
    let image = raster(
        "<svg width='16' height='8' viewBox='0 0 8 8'><rect width='8' height='8' fill='red'/></svg>",
    );
    let pixel = |x: usize| &image.rgba[(4 * 16 + x) * 4..(4 * 16 + x + 1) * 4];
    assert_eq!(pixel(0), [0, 0, 0, 0]);
    assert_eq!(pixel(8), [255, 0, 0, 255]);
    assert_eq!(pixel(15), [0, 0, 0, 0]);
}

#[test]
fn malformed_path_points_suffixes_do_not_silently_render_prefixes() {
    for body in [
        "<path d=\"M0 0L2 2 ERROR\"/>",
        "<path d=\"M0 0Z 2\"/>",
        "<path d=\"M0 0L\"/>",
        "<polygon points=\"0,0 1,1 2\"/>",
        "<polyline points=\"0,0 1,1 garbage\"/>",
        "<rect transform=\"translate(2) garbage\"/>",
        "<rect x=\"1px\"/>",
    ] {
        assert!(policy::prepare(&svg(body)).is_err(), "{body}");
    }
}
#[test]
fn size_depth_nodes_segments_attributes_and_extreme_numbers_are_bounded() {
    assert!(pixel_bytes(u32::MAX, u32::MAX).is_err());
    assert!(pixel_bytes(4096, 4096).is_err());
    assert_eq!(pixel_bytes(2048, 2048), Ok(16 * 1024 * 1024));
    for body in [
        "<rect x=\"1e999\"/>",
        "<rect transform=\"scale(1e50)\"/>",
        "<g transform=\"scale(1000000)\"><g transform=\"scale(1000000)\"/></g>",
        "<rect width=\"-1\"/>",
        "<path d=\"M0 0A1e100 1 0 0 0 1 1\"/>",
        "<g transform=\"scale(1000000)\"><g transform=\"scale(1000000)\"/></g>",
    ] {
        assert!(policy::prepare(&svg(body)).is_err(), "{body}");
    }
    assert!(policy::prepare(&svg(&("<g>".repeat(65) + &"</g>".repeat(65)))).is_err());
    assert!(policy::prepare(&svg(&"<rect/>".repeat(4097))).is_err());
    assert!(
        policy::prepare(&svg(&format!(
            "<path d=\"M0 0 {}\"/>",
            "1 1 ".repeat(50_001)
        )))
        .is_err()
    );
    assert!(policy::prepare(&svg(&format!("<path d=\"{}\"/>", " ".repeat(65_537)))).is_err());
    for source in [
        "<svg/>",
        "<svg width='0' height='1'/>",
        "<svg width='4096' height='4096'/>",
        "<svg width='16' height='8' viewBox='0 0 0 1'/>",
        "<svg width='16' height='8' viewBox='0 0 1e-100 1'/>",
    ] {
        assert!(policy::prepare(source).is_err());
    }
}
#[test]
fn maximum_raster_and_many_implicit_segments_have_bounded_resource_work() {
    let start = std::time::Instant::now();
    let image = raster(
        "<svg width='2048' height='2048'><rect width='2048' height='2048' fill='red'/></svg>",
    );
    assert_eq!(image.rgba.len(), 16 * 1024 * 1024);
    assert_eq!(&image.rgba[..4], &[255, 0, 0, 255]);
    let raster_time = start.elapsed();
    // Split across elements so the per-attribute64KiB limit is respected.
    let paths = (0..8)
        .map(|_| format!("<path d='M0 0 {}'/>", "1 1 ".repeat(6000)))
        .collect::<String>();
    let start = std::time::Instant::now();
    policy::prepare(&svg(&paths)).unwrap();
    let parse_time = start.elapsed();
    let too_many = paths + &format!("<path d='M0 0 {}'/>", "1 1 ".repeat(3000));
    assert!(policy::prepare(&svg(&too_many)).is_err());
    eprintln!(
        "SVG4,194,304pixel raster {raster_time:?}; 16,777,216B premultiplied output; 48,008 implicit segments parse {parse_time:?}"
    );
}

#[test]
fn worker_requires_parent_admission_ack_before_allocating_or_emitting_pixels() {
    let source = svg("<rect width=\"16\" height=\"8\"/>");
    let mut frame = Vec::new();
    frame.extend_from_slice(MAGIC);
    frame.extend_from_slice(&(source.len() as u32).to_le_bytes());
    frame.extend_from_slice(source.as_bytes());
    let mut output = Vec::new();
    assert!(worker(&mut std::io::Cursor::new(&frame), &mut output).is_err());
    assert_eq!(output, header(16, 8));
    frame.push(ACK);
    let mut output = Vec::new();
    worker(&mut std::io::Cursor::new(&frame), &mut output).unwrap();
    assert_eq!(output.len(), 12 + 16 * 8 * 4);
}
