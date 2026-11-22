//! Presentation-only ordering. Never reads session files on the UI thread.
#[cfg(test)]
#[path = "sidebar_order_tests.rs"]
mod tests;

mod running;
pub(super) use running::RunningOrder;

use super::sidebar_tree::Branch;
use serde::{Deserialize, Serialize};
use std::{cmp::Ordering, path::Path};

#[derive(Clone, Copy, Default, PartialEq, Serialize, Deserialize)]
pub(super) enum Sort {
    #[default]
    Activity,
    Created,
    Name,
}
impl Sort {
    pub fn label(self) -> &'static str {
        match self {
            Self::Activity => "Last updated",
            Self::Created => "Date created",
            Self::Name => "Name",
        }
    }
}

#[derive(Clone, Copy, Default, Serialize, Deserialize)]
#[serde(default)]
pub(super) struct Options {
    pub sort: Sort,
    pub reverse: bool,
}
impl Options {
    fn direction(self, order: Ordering) -> Ordering {
        if self.reverse { order.reverse() } else { order }
    }
    // Unknown creation times stay at the end in either direction.
    fn dates(self, a: Option<std::time::SystemTime>, b: Option<std::time::SystemTime>) -> Ordering {
        match (a, b) {
            (Some(a), Some(b)) => self.direction(b.cmp(&a)),
            _ => b.is_some().cmp(&a.is_some()),
        }
    }
    pub fn compare(self, a: &Branch, b: &Branch) -> Ordering {
        match self.sort {
            Sort::Activity => self.dates(Some(a.modified), Some(b.modified)),
            Sort::Created => self.dates(a.created, b.created),
            Sort::Name => self.direction(a.title.to_lowercase().cmp(&b.title.to_lowercase())),
        }
    }
    pub fn compare_running(self, a: &Branch, b: &Branch, running: &RunningOrder) -> Ordering {
        running.compare(a, b).then_with(|| self.compare(a, b))
    }
    pub fn compare_groups(
        self,
        a: (&Path, &[usize]),
        b: (&Path, &[usize]),
        rows: &[Branch],
    ) -> Ordering {
        (match self.sort {
            Sort::Name => self.direction(
                crate::sessions::project_name(a.0)
                    .to_lowercase()
                    .cmp(&crate::sessions::project_name(b.0).to_lowercase()),
            ),
            sort => {
                let latest = |indices: &[usize]| {
                    indices
                        .iter()
                        .filter_map(|i| {
                            if sort == Sort::Created {
                                rows[*i].created
                            } else {
                                Some(rows[*i].modified)
                            }
                        })
                        .max()
                };
                self.dates(latest(a.1), latest(b.1))
            }
        })
        .then_with(|| a.0.cmp(b.0))
    }
}
