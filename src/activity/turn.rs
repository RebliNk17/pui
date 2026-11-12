//! Lightweight turn rollups: no copies of recorded patch bodies for collapsed footers.
use super::*;
use std::collections::BTreeMap;

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct FileSummary {
    pub path: String,
    pub added: usize,
    pub removed: usize,
}

pub fn turn_files(messages: &[Message]) -> Vec<FileSummary> {
    fn visit(tool: &Tool, files: &mut BTreeMap<String, FileSummary>) {
        // Codemode wrappers can fail after successfully completed child edits.
        if tool.finished
            && !tool.failed
            && matches!(tool.name.as_str(), "edit" | "write")
            && !tool.path.is_empty()
        {
            let file = files
                .entry(tool.path.clone())
                .or_insert_with(|| FileSummary {
                    path: tool.path.clone(),
                    ..Default::default()
                });
            file.added = file.added.saturating_add(tool.added);
            file.removed = file.removed.saturating_add(tool.removed);
        }
        for child in &tool.children {
            visit(child, files);
        }
    }
    let mut files = BTreeMap::new();
    for message in messages {
        for tool in &message.tools {
            visit(tool, &mut files);
        }
    }
    files.into_values().collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn counts_only_recorded_successes_and_deduplicates_paths_without_patch_copies() {
        let edit = |path: &str, added, failed| Tool {
            name: "edit".into(),
            path: path.into(),
            added,
            removed: 2,
            finished: true,
            failed,
            diff: "patch".repeat(100),
            ..Default::default()
        };
        let tools = vec![
            edit("a.rs", 3, false),
            edit("a.rs", 4, false),
            edit("failed.rs", 20, true),
            Tool {
                name: "codemode".into(),
                finished: true,
                failed: true,
                children: vec![edit("b.rs", 1, false)],
                ..Default::default()
            },
        ];
        let files = turn_files(&[Message {
            tools,
            ..Default::default()
        }]);
        assert_eq!(files.len(), 2);
        assert_eq!((files[0].added, files[0].removed), (7, 4));
        assert_eq!(files[1].path, "b.rs");
    }
}
