//! Shared display-row accounting for numbered diffs and explicit selection mapping.
pub(in crate::app) const HUNK_CAPTION: &str = "        …\n";
#[derive(Clone, Copy)]
pub(in crate::app) struct Row<'a> {
    pub text: &'a str,
    pub index: usize,
    pub hunk: usize,
    pub header: bool,
    pub added: bool,
    pub removed: bool,
    pub context: bool,
    pub old: Option<usize>,
    pub new: Option<usize>,
    pub located: bool,
}
impl Row<'_> {
    pub fn gutter(&self) -> String {
        let number = if self.added || self.context {
            self.new
        } else if self.removed {
            self.old
        } else {
            None
        };
        format!(
            "{:>5}   ",
            number.map(|value| value.to_string()).unwrap_or_default()
        )
    }
    pub fn selectable(&self) -> bool {
        self.added || self.removed || self.context
    }
}
fn coordinates(text: &str) -> Option<(usize, usize)> {
    let mut fields = text.strip_prefix("@@ -")?.split_whitespace();
    let before = fields.next()?;
    let after = fields.next()?.strip_prefix('+')?;
    if fields.next()? != "@@" {
        return None;
    }
    let parse = |field: &str| {
        let (start, count) = field.split_once(',').unwrap_or((field, "1"));
        let start = start.parse::<usize>().ok()?;
        let count = count.parse::<usize>().ok()?;
        (start > 0 || count == 0).then_some(start)
    };
    Some((parse(before)?, parse(after)?))
}

pub(in crate::app) fn rows(source: &str) -> impl Iterator<Item = Row<'_>> {
    let mut old = 0usize;
    let mut new = 0usize;
    let mut hunk = 0;
    let mut located = false;
    source.lines().enumerate().map(move |(index, text)| {
        let header = text.starts_with("@@");
        if header {
            hunk += 1;
            let coordinates = coordinates(text);
            located = coordinates.is_some();
            (old, new) = coordinates.unwrap_or((1, 1));
            old = old.saturating_sub(1);
            new = new.saturating_sub(1);
        }
        let added = text.starts_with('+') && !(hunk == 0 && text.starts_with("+++ "));
        let removed = text.starts_with('-') && !(hunk == 0 && text.starts_with("--- "));
        let context = text.starts_with(' ');
        if added || context {
            new = new.saturating_add(1);
        }
        if removed || context {
            old = old.saturating_add(1);
        }
        Row {
            text,
            index,
            hunk,
            header,
            added,
            removed,
            context,
            old: (removed || context).then_some(old),
            new: (added || context).then_some(new),
            located,
        }
    })
}
