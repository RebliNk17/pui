//! Per-file read/unread tracking with automatic invalidation when code changes the file.
use std::collections::{BTreeMap, hash_map::DefaultHasher};
use std::hash::{Hash, Hasher};

#[derive(Clone, Copy, PartialEq, Eq)]
struct ReadMark {
    summary_sig: u64,
    diff_sig: Option<u64>,
}

pub(super) fn hash_str(value: &str) -> u64 {
    let mut hasher = DefaultHasher::new();
    value.hash(&mut hasher);
    hasher.finish()
}

pub(super) fn git_summary_sig(status: &str, added: &str, removed: &str) -> u64 {
    let mut hasher = DefaultHasher::new();
    status.hash(&mut hasher);
    added.hash(&mut hasher);
    removed.hash(&mut hasher);
    hasher.finish()
}

pub(super) fn recorded_summary_sig(added: usize, removed: usize, diff: &str) -> u64 {
    let mut hasher = DefaultHasher::new();
    added.hash(&mut hasher);
    removed.hash(&mut hasher);
    diff.hash(&mut hasher);
    hasher.finish()
}

#[derive(Default)]
pub(super) struct ReadState {
    marks: BTreeMap<(bool, String), ReadMark>,
}

impl ReadState {
    pub(super) fn clear(&mut self) {
        self.marks.clear();
    }

    pub(super) fn toggle(
        &mut self,
        git: bool,
        path: &str,
        summary_sig: u64,
        diff_sig: Option<u64>,
    ) -> bool {
        let key = (git, path.to_owned());
        if self.marks.remove(&key).is_some() {
            false
        } else {
            self.marks.insert(
                key,
                ReadMark {
                    summary_sig,
                    diff_sig,
                },
            );
            true
        }
    }

    /// Reconciles an existing read mark against the latest file summary and loaded diff.
    /// Automatically marks the file unread if either changed.
    pub(super) fn reconcile(
        &mut self,
        git: bool,
        path: &str,
        summary_sig: u64,
        loaded_diff_sig: Option<u64>,
    ) -> bool {
        let key = (git, path.to_owned());
        let Some(mark) = self.marks.get_mut(&key) else {
            return false;
        };
        if mark.summary_sig != summary_sig {
            self.marks.remove(&key);
            return false;
        }
        if let (Some(old), Some(new)) = (mark.diff_sig, loaded_diff_sig)
            && old != new
        {
            self.marks.remove(&key);
            return false;
        }
        if mark.diff_sig.is_none()
            && let Some(new) = loaded_diff_sig
        {
            mark.diff_sig = Some(new);
        }
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn manual_unmark_and_code_change_mark_file_unread() {
        let mut state = ReadState::default();
        let s1 = git_summary_sig(" M", "3", "1");
        let d1 = Some(hash_str("+a\n-b"));

        assert!(state.toggle(true, "src/lib.rs", s1, d1));
        assert!(state.reconcile(true, "src/lib.rs", s1, d1));

        // 1. User unmarks it manually
        assert!(!state.toggle(true, "src/lib.rs", s1, d1));
        assert!(!state.reconcile(true, "src/lib.rs", s1, d1));

        // Re-mark as read, then simulate code change (summary or diff content changed)
        assert!(state.toggle(true, "src/lib.rs", s1, d1));
        let s2 = git_summary_sig(" M", "4", "1");
        assert!(!state.reconcile(true, "src/lib.rs", s2, d1));

        // Same summary counts (+3/-1) but different diff content also marks unread
        assert!(state.toggle(true, "src/lib.rs", s1, d1));
        let d2 = Some(hash_str("+c\n-b"));
        assert!(!state.reconcile(true, "src/lib.rs", s1, d2));
    }
}
