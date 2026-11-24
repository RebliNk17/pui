//! Explicit scope inventory: no automatic traversal into frame documents.
use serde_json::Value;
#[cfg(test)]
#[path = "scope_tests.rs"]
mod tests;

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(in crate::app::webview) struct Scope {
    pub(in crate::app::webview) frame_id: Option<String>,
    pub(in crate::app::webview) shadow_host: Option<String>,
}
impl Scope {
    pub(super) fn valid(&self) -> bool {
        self.frame_id.as_ref().is_none_or(|id| valid_id(id))
            && self
                .shadow_host
                .as_deref()
                .is_none_or(|v| super::selectors::selector(v).is_ok())
    }
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub(in crate::app::webview) struct FrameChoice {
    pub(in crate::app::webview) id: String,
    pub(in crate::app::webview) label: String,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct FrameIdentity {
    pub(super) id: String,
    loader: String,
    parent: Option<String>,
    origin: String,
    allowed: bool,
}
impl FrameIdentity {
    pub(super) fn expected_origin(&self) -> Option<&str> {
        self.parent.as_ref().map(|_| self.origin.as_str())
    }
}
pub(super) fn valid_id(value: &str) -> bool {
    !value.is_empty() && value.len() <= 256 && !value.chars().any(char::is_control)
}
fn origin(frame: &Value) -> Option<String> {
    let supplied = frame.get("securityOrigin")?.as_str()?;
    let parsed = url::Url::parse(frame.get("url")?.as_str()?).ok()?;
    if !matches!(parsed.scheme(), "http" | "https")
        || parsed.origin().ascii_serialization() != supplied
    {
        return None;
    }
    Some(supplied.into())
}
pub(super) fn inventory(value: &Value) -> Option<Vec<FrameIdentity>> {
    fn walk(
        tree: &Value,
        parent: Option<&FrameIdentity>,
        top_origin: &str,
        depth: usize,
        out: &mut Vec<FrameIdentity>,
    ) -> Option<()> {
        if depth > 8 || out.len() >= 32 {
            return None;
        }
        let frame = tree.get("frame")?;
        let id = frame.get("id")?.as_str().filter(|v| valid_id(v))?;
        if out.iter().any(|f| f.id == id) {
            return None;
        }
        let parent_id = frame.get("parentId").and_then(Value::as_str);
        if parent_id != parent.map(|p| p.id.as_str()) {
            return None;
        }
        let actual_origin = origin(frame).unwrap_or_default();
        let identity = FrameIdentity {
            id: id.into(),
            loader: frame
                .get("loaderId")
                .and_then(Value::as_str)
                .filter(|v| valid_id(v))
                .unwrap_or_default()
                .into(),
            parent: parent_id.map(str::to_owned),
            allowed: parent.is_none()
                || parent.is_some_and(|p| p.allowed)
                    && !actual_origin.is_empty()
                    && actual_origin == top_origin,
            origin: actual_origin,
        };
        out.push(identity.clone());
        if let Some(children) = tree.get("childFrames") {
            for child in children.as_array()? {
                walk(child, Some(&identity), top_origin, depth + 1, out)?;
            }
        }
        Some(())
    }
    let tree = value.get("frameTree")?;
    let top_origin = origin(tree.get("frame")?).unwrap_or_default();
    let mut frames = Vec::new();
    walk(tree, None, &top_origin, 0, &mut frames)?;
    Some(frames)
}
pub(super) fn choices(frames: &[FrameIdentity]) -> Vec<FrameChoice> {
    frames
        .iter()
        .enumerate()
        .filter(|(_, f)| f.parent.is_none() || f.allowed && !f.loader.is_empty())
        .map(|(index, f)| FrameChoice {
            id: f.id.clone(),
            label: if index == 0 {
                "Top document".into()
            } else {
                format!("Same-origin frame {index}")
            },
        })
        .collect()
}
pub(super) fn selected<'a>(
    frames: &'a [FrameIdentity],
    previous: &[FrameIdentity],
    scope: &Scope,
) -> Option<&'a str> {
    let Some(id) = &scope.frame_id else {
        return Some(&frames.first()?.id);
    };
    let frame = frames.iter().find(|f| &f.id == id)?;
    if frame.parent.is_none() {
        return Some(&frame.id);
    }
    if !frame.allowed || frame.loader.is_empty() || !previous.iter().any(|f| f == frame) {
        return None;
    }
    // Bind every parent document too: replacing an embedding document cannot
    // preserve a stale descendant scope merely by reusing an ID.
    let mut parent = frame.parent.as_ref();
    while let Some(id) = parent {
        let ancestor = frames.iter().find(|f| &f.id == id)?;
        if ancestor.loader.is_empty() || !previous.iter().any(|f| f == ancestor) {
            return None;
        }
        parent = ancestor.parent.as_ref();
    }
    Some(&frame.id)
}
