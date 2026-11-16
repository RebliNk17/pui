//! Deterministic hostile-input/mutation coverage, not an exhaustive fuzz audit.
use super::*;
fn svg(body: &str) -> String {
    format!("<svg xmlns='http://www.w3.org/2000/svg' width='16' height='8'>{body}</svg>")
}
#[test]
fn unsupported_attributes_references_and_encoded_names_are_never_partially_accepted() {
    for element in [
        "g", "rect", "circle", "ellipse", "line", "polygon", "polyline", "path",
    ] {
        for (attribute, value) in [
            ("href", "file:///tmp/private"),
            ("href", "data:image/svg+xml;base64,AA=="),
            ("style", "fill:url(https://localhost/private)"),
            ("onload", "never()"),
            ("clip-path", "url(#clip)"),
            ("mask", "url(#mask)"),
            ("filter", "url(#blur)"),
            ("marker-start", "url(#marker)"),
            ("marker-end", "url(#marker)"),
            ("opacity", ".5"),
            ("stroke-dasharray", "1,2"),
            ("class", "active"),
            ("display", "none"),
            ("visibility", "hidden"),
            ("fill", "url(&#35;paint)"),
            ("fill", "u&#114;l(#paint)"),
            ("fill", "context-fill"),
            ("transform-origin", "0 0"),
            ("vector-effect", "non-scaling-stroke"),
        ] {
            let source = svg(&format!(
                "<rect width='2' height='2'/><{element} {attribute}=\"{value}\"/>"
            ));
            assert!(
                policy::prepare(&source).is_err(),
                "{element} {attribute}={value}"
            );
        }
    }
    for namespace in [
        "urn:foreign",
        "http://www.w3.org/1999/xhtml",
        "http://www.w3.org/1999/xlink",
    ] {
        assert!(
            policy::prepare(&svg(&format!(
                "<g xmlns='{namespace}'><rect width='2' height='2'/></g>"
            )))
            .is_err()
        );
    }
    for value in [
        "1e999", "-1e999", "NaN", "inf", "-inf", "1px", "100%", "calc(2)", "1 2", "1e50",
        "-1000001",
    ] {
        assert!(
            policy::prepare(&svg(&format!("<rect x='{value}' width='2' height='2'/>"))).is_err(),
            "{value}"
        );
    }
}
#[test]
fn seeded_xml_mutations_never_panic_or_return_unbounded_output() {
    let source =
        svg("<g transform='translate(1 1)'><path d='M0 0L4 0 4 4 0 4Z' fill='#806bff'/></g>");
    let start = std::time::Instant::now();
    let mut rejected = 0;
    let mut accepted = 0;
    for index in (0..source.len()).step_by(3) {
        for replacement in [b'<', b'>', b'&', b'\'', b'/', b'0'] {
            let mut bytes = source.as_bytes().to_vec();
            bytes[index] = replacement;
            let candidate = String::from_utf8(bytes).unwrap();
            match policy::prepare(&candidate) {
                Ok((_, width, height)) => {
                    assert!(pixel_bytes(width, height).unwrap() <= MAX_PIXELS * 4);
                    accepted += 1;
                }
                Err(_) => rejected += 1,
            }
        }
    }
    assert!(rejected > 100);
    assert!(accepted > 0);
    eprintln!(
        "deterministic SVG mutation corpus: {rejected} refused/{accepted} bounded accepted in {:?}",
        start.elapsed()
    );
}
