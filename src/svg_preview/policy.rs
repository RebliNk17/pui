//! Deliberately restricted first-slice policy, rejecting whole unsupported SVGs.
use super::*;
use resvg::{
    tiny_skia::Transform,
    usvg::{self, roxmltree},
};
use std::str::FromStr;

const SVG: &str = "http://www.w3.org/2000/svg";
const MAX_SEGMENTS: usize = 50_000;
const COORD: f64 = 1_000_000.0;
fn bounded(value: f64) -> Result<(), Failure> {
    if value.is_finite() && value.abs() <= COORD {
        Ok(())
    } else {
        Err(Failure::Limit)
    }
}
fn number(value: &str) -> Result<f64, Failure> {
    let value = value.trim().parse::<f64>().map_err(|_| Failure::Invalid)?;
    bounded(value)?;
    Ok(value)
}
fn numbers(value: &str, max: usize) -> Result<Vec<f64>, Failure> {
    let mut values = Vec::new();
    for value in svgtypes::NumberListParser::from(value) {
        let value = value.map_err(|_| Failure::Invalid)?;
        bounded(value)?;
        if values.len() == max {
            return Err(Failure::Limit);
        }
        values.push(value);
    }
    Ok(values)
}
fn path(value: &str, segments: &mut usize) -> Result<(), Failure> {
    // Bound raw radii/angles/control coordinates too: arc conversion can
    // otherwise hide extreme raw values behind ordinary normalized endpoints.
    for (index, segment) in svgtypes::PathParser::from(value).enumerate() {
        if index >= MAX_SEGMENTS {
            return Err(Failure::Limit);
        }
        use svgtypes::PathSegment as P;
        let values: &[f64] = match segment.map_err(|_| Failure::Invalid)? {
            P::MoveTo { x, y, .. } | P::LineTo { x, y, .. } | P::SmoothQuadratic { x, y, .. } => {
                &[x, y]
            }
            P::HorizontalLineTo { x, .. } => &[x],
            P::VerticalLineTo { y, .. } => &[y],
            P::CurveTo {
                x1,
                y1,
                x2,
                y2,
                x,
                y,
                ..
            } => &[x1, y1, x2, y2, x, y],
            P::SmoothCurveTo { x2, y2, x, y, .. } => &[x2, y2, x, y],
            P::Quadratic { x1, y1, x, y, .. } => &[x1, y1, x, y],
            P::EllipticalArc {
                rx,
                ry,
                x_axis_rotation,
                x,
                y,
                ..
            } => &[rx, ry, x_axis_rotation, x, y],
            P::ClosePath { .. } => &[],
        };
        for &value in values {
            bounded(value)?;
        }
    }
    use svgtypes::SimplePathSegment::*;
    for segment in svgtypes::SimplifyingPathParser::from(value) {
        *segments += 1;
        if *segments > MAX_SEGMENTS {
            return Err(Failure::Limit);
        }
        let coordinates: &[f64] = match segment.map_err(|_| Failure::Invalid)? {
            MoveTo { x, y } | LineTo { x, y } => &[x, y],
            CurveTo {
                x1,
                y1,
                x2,
                y2,
                x,
                y,
            } => &[x1, y1, x2, y2, x, y],
            Quadratic { x1, y1, x, y } => &[x1, y1, x, y],
            ClosePath => &[],
        };
        for &coordinate in coordinates {
            bounded(coordinate)?;
        }
    }
    Ok(())
}
fn paint(value: &str) -> Result<(), Failure> {
    if matches!(value, "none" | "inherit") || svgtypes::Color::from_str(value).is_ok() {
        Ok(())
    } else {
        Err(Failure::Unsupported)
    }
}
fn transform(value: &str) -> Result<(), Failure> {
    for (index, token) in svgtypes::TransformListParser::from(value).enumerate() {
        if index == 64 {
            return Err(Failure::Limit);
        }
        use svgtypes::TransformListToken::*;
        let coordinates: &[f64] = match token.map_err(|_| Failure::Invalid)? {
            Matrix { a, b, c, d, e, f } => &[a, b, c, d, e, f],
            Translate { tx, ty } => &[tx, ty],
            Scale { sx, sy } => &[sx, sy],
            Rotate { angle } | SkewX { angle } | SkewY { angle } => &[angle],
        };
        for &value in coordinates {
            bounded(value)?;
        }
    }
    let t = svgtypes::Transform::from_str(value).map_err(|_| Failure::Invalid)?;
    for value in [t.a, t.b, t.c, t.d, t.e, t.f] {
        bounded(value)?;
    }
    Ok(())
}
fn attribute(element: &str, name: &str, value: &str, segments: &mut usize) -> Result<(), Failure> {
    let value = value.trim();
    match name {
        "id" if value.len() <= 128 => Ok(()),
        "version" if element == "svg" && matches!(value, "1.0" | "1.1" | "2.0") => Ok(()),
        "width" | "height" if element == "svg" => {
            if number(value)? > 0.0 {
                Ok(())
            } else {
                Err(Failure::Invalid)
            }
        }
        "viewBox" if element == "svg" => {
            let values = numbers(value, 4)?;
            if values.len() == 4 && values[2] > 0.0 && values[3] > 0.0 {
                Ok(())
            } else {
                Err(Failure::Invalid)
            }
        }
        "preserveAspectRatio" if element == "svg" => {
            if matches!(
                value,
                "none" | "xMidYMid" | "xMidYMid meet" | "xMidYMid slice"
            ) {
                Ok(())
            } else {
                Err(Failure::Unsupported)
            }
        }
        "fill" | "stroke" => paint(value),
        "fill-opacity" | "stroke-opacity" => {
            if (0.0..=1.0).contains(&number(value)?) {
                Ok(())
            } else {
                Err(Failure::Invalid)
            }
        }
        "fill-rule" if matches!(value, "nonzero" | "evenodd" | "inherit") => Ok(()),
        "stroke-linecap" if matches!(value, "butt" | "round" | "square" | "inherit") => Ok(()),
        "stroke-linejoin" if matches!(value, "miter" | "round" | "bevel" | "inherit") => Ok(()),
        "stroke-width" if (0.0..=4096.0).contains(&number(value)?) => Ok(()),
        "stroke-miterlimit" if (1.0..=100.0).contains(&number(value)?) => Ok(()),
        "transform" if element != "svg" => transform(value),
        "d" if element == "path" => path(value, segments),
        "points" if matches!(element, "polygon" | "polyline") => {
            // PointsParser silently truncates malformed/odd suffixes; use its
            // fallible number tokenizer and validate the complete even list.
            let values = numbers(value, MAX_SEGMENTS * 2)?;
            if values.len() < 4 || values.len() % 2 != 0 {
                return Err(Failure::Invalid);
            }
            *segments += values.len() / 2;
            if *segments > MAX_SEGMENTS {
                Err(Failure::Limit)
            } else {
                Ok(())
            }
        }
        _ => {
            let geometry = match element {
                "rect" => matches!(name, "x" | "y" | "width" | "height" | "rx" | "ry"),
                "circle" => matches!(name, "cx" | "cy" | "r"),
                "ellipse" => matches!(name, "cx" | "cy" | "rx" | "ry"),
                "line" => matches!(name, "x1" | "y1" | "x2" | "y2"),
                _ => false,
            };
            if !geometry {
                return Err(Failure::Unsupported);
            }
            let n = number(value)?;
            if matches!(name, "width" | "height" | "rx" | "ry" | "r") && n < 0.0 {
                Err(Failure::Invalid)
            } else {
                Ok(())
            }
        }
    }
}
fn tree_group(group: &usvg::Group) -> Result<(), Failure> {
    if group.opacity().get() != 1.0
        || group.isolate()
        || group.clip_path().is_some()
        || group.mask().is_some()
        || !group.filters().is_empty()
        || group.blend_mode() != usvg::BlendMode::Normal
    {
        return Err(Failure::Unsupported);
    }
    check_transform(group.abs_transform())?;
    for node in group.children() {
        match node {
            usvg::Node::Group(group) => tree_group(group)?,
            usvg::Node::Path(path) => {
                check_transform(path.abs_transform())?;
                if path
                    .fill()
                    .is_some_and(|f| !matches!(f.paint(), usvg::Paint::Color(_)))
                    || path
                        .stroke()
                        .is_some_and(|s| !matches!(s.paint(), usvg::Paint::Color(_)))
                {
                    return Err(Failure::Unsupported);
                }
            }
            _ => return Err(Failure::Unsupported),
        }
    }
    Ok(())
}
fn check_transform(t: Transform) -> Result<(), Failure> {
    for value in [t.sx, t.kx, t.ky, t.sy, t.tx, t.ty] {
        bounded(f64::from(value))?;
    }
    Ok(())
}
pub(super) fn prepare(source: &str) -> Result<(usvg::Tree, u32, u32), Failure> {
    if source.is_empty() || source.len() > MAX_SOURCE {
        return Err(Failure::Limit);
    }
    // roxmltree permits an empty DTD even with allow_dtd=false. Reject all DTDs;
    // do not let entity/reference expansion or a stylesheet PI reach usvg.
    if source.contains("<!DOCTYPE") {
        return Err(Failure::Unsupported);
    }
    // roxmltree accepts but does not interpret declaration encodings. This
    // byte-framed UTF-8-only slice permits only unambiguous common declarations.
    if let Some(declaration) = source.trim_start_matches('\u{feff}').strip_prefix("<?xml") {
        let end = declaration.find("?>").ok_or(Failure::Invalid)?;
        let declaration = declaration[..end].trim();
        if !matches!(
            declaration,
            "version=\"1.0\""
                | "version='1.0'"
                | "version=\"1.0\" encoding=\"UTF-8\""
                | "version='1.0' encoding='UTF-8'"
                | "version=\"1.0\" encoding=\"utf-8\""
                | "version='1.0' encoding='utf-8'"
        ) {
            return Err(Failure::Unsupported);
        }
    }
    let document = roxmltree::Document::parse_with_options(
        source,
        roxmltree::ParsingOptions {
            allow_dtd: false,
            nodes_limit: 4096,
            entity_resolver: None,
        },
    )
    .map_err(|_| Failure::Invalid)?;
    let root = document.root_element();
    if root.tag_name().name() != "svg" {
        return Err(Failure::Unsupported);
    }
    let mut segments = 0;
    let mut attr_bytes = 0;
    let mut composed = std::collections::HashMap::<roxmltree::NodeId, svgtypes::Transform>::new();
    for node in document.descendants() {
        if node.is_pi() {
            return Err(Failure::Unsupported);
        }
        if node.is_text() && node.text().is_some_and(|text| !text.trim().is_empty()) {
            return Err(Failure::Unsupported);
        }
        if !node.is_element() {
            continue;
        }
        if node.ancestors().count() > 65 {
            return Err(Failure::Limit);
        }
        let tag = node.tag_name();
        let name = tag.name();
        if tag.namespace().is_some_and(|ns| ns != SVG)
            || tag.namespace() != root.tag_name().namespace()
            || node
                .parent_element()
                .is_some_and(|parent| !matches!(parent.tag_name().name(), "svg" | "g"))
            || node.namespaces().any(|ns| ns.uri() != SVG)
            || !matches!(
                name,
                "svg"
                    | "g"
                    | "rect"
                    | "circle"
                    | "ellipse"
                    | "line"
                    | "polygon"
                    | "polyline"
                    | "path"
            )
            || name == "svg" && node != root
        {
            return Err(Failure::Unsupported);
        }
        if node.attributes().len() > 32 {
            return Err(Failure::Limit);
        }
        let parent = node
            .parent_element()
            .and_then(|parent| composed.get(&parent.id()))
            .copied()
            .unwrap_or_default();
        let local = node
            .attribute("transform")
            .map(svgtypes::Transform::from_str)
            .transpose()
            .map_err(|_| Failure::Invalid)?
            .unwrap_or_default();
        let combined = svgtypes::Transform::new(
            parent.a * local.a + parent.c * local.b,
            parent.b * local.a + parent.d * local.b,
            parent.a * local.c + parent.c * local.d,
            parent.b * local.c + parent.d * local.d,
            parent.a * local.e + parent.c * local.f + parent.e,
            parent.b * local.e + parent.d * local.f + parent.f,
        );
        for value in [
            combined.a, combined.b, combined.c, combined.d, combined.e, combined.f,
        ] {
            bounded(value)?;
        }
        composed.insert(node.id(), combined);
        for attr in node.attributes() {
            attr_bytes += attr.value().len();
            if attr.value().len() > 64 * 1024 || attr_bytes > 256 * 1024 {
                return Err(Failure::Limit);
            }
            if attr.namespace().is_some() {
                return Err(Failure::Unsupported);
            }
            attribute(name, attr.name(), attr.value(), &mut segments)?;
        }
    }
    let width = number(root.attribute("width").ok_or(Failure::Unsupported)?)?;
    let height = number(root.attribute("height").ok_or(Failure::Unsupported)?)?;
    if width.fract() != 0.0 || height.fract() != 0.0 {
        return Err(Failure::Unsupported);
    }
    if let Some(viewbox) = root.attribute("viewBox") {
        let values = numbers(viewbox, 4)?;
        if values.len() != 4 || values[2] < 1e-6 || values[3] < 1e-6 {
            return Err(Failure::Limit);
        }
        bounded(width / values[2])?;
        bounded(height / values[3])?;
    }
    let (width, height) = (width as u32, height as u32);
    pixel_bytes(width, height)?;
    let options = usvg::Options {
        resources_dir: None,
        style_sheet: None,
        max_depth: 64,
        image_href_resolver: usvg::ImageHrefResolver {
            resolve_data: Box::new(|_, _, _| None),
            resolve_string: Box::new(|_, _| None),
        },
        ..Default::default()
    };
    // Never from_str/from_data: those opt into DTD parsing internally.
    let tree = usvg::Tree::from_xmltree(&document, &options).map_err(|_| Failure::Invalid)?;
    if tree.size().width() != width as f32 || tree.size().height() != height as f32 {
        return Err(Failure::Invalid);
    }
    tree_group(tree.root())?;
    Ok((tree, width, height))
}
