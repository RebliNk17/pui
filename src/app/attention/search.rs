//! Frozen metadata for one explicit Spotlight opening; never a live run status.
use super::*;
use std::{cmp::Reverse, collections::BTreeMap};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(in crate::app) struct SearchEvent {
    kind: Kind,
    at: u64,
}
impl SearchEvent {
    pub(in crate::app) fn order(self) -> (u8, Reverse<u64>) {
        (
            match self.kind {
                Kind::NeedsInput => 0,
                Kind::Failed => 1,
                Kind::Completed => 2,
            },
            Reverse(self.at),
        )
    }
    pub(in crate::app) fn label(self) -> &'static str {
        match self.kind {
            Kind::NeedsInput => "Observed: needs input",
            Kind::Failed => "Observed: failed",
            Kind::Completed => "Observed: completed",
        }
    }
}
#[derive(Default)]
pub(in crate::app) struct SearchSnapshot {
    saved: BTreeMap<PathBuf, SearchEvent>,
    runtime: BTreeMap<String, SearchEvent>,
}
impl Pui {
    pub(in crate::app) fn search_attention_snapshot(&self) -> SearchSnapshot {
        let time = now();
        let mut snapshot = SearchSnapshot::default();
        for entry in self
            .attention
            .entries
            .iter()
            .filter(|entry| entry.unread_at(time))
        {
            let event = SearchEvent {
                kind: entry.metadata.kind,
                at: entry.metadata.at,
            };
            match &entry.owner {
                Owner::Saved(path) => {
                    snapshot.saved.insert(path.clone(), event);
                }
                Owner::Runtime(id) => {
                    snapshot.runtime.insert(id.clone(), event);
                }
            }
        }
        snapshot
    }
    pub(in crate::app) fn search_attention(
        &self,
        candidate: &crate::app::search::Candidate,
        snapshot: &SearchSnapshot,
    ) -> Option<SearchEvent> {
        if let Some(path) = &candidate.path {
            return snapshot
                .saved
                .get(&self.sync.attention_identity(path))
                .copied();
        }
        match &candidate.target {
            sidebar_tree::Target::Agent(id) => snapshot.runtime.get(id).copied(),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests;
