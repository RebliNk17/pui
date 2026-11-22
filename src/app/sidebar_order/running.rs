//! Runtime-only ranks: activity can refresh freely without moving running rows.
use super::super::sidebar_tree::Branch;
use std::{cmp::Ordering, collections::HashMap};

#[derive(Default)]
pub(in crate::app) struct RunningOrder {
    ranks: HashMap<String, usize>,
}

impl RunningOrder {
    /// Observe the whole listing, not just expanded/visible groups. A running
    /// descendant keeps its parent tree above idle trees without being detached.
    pub(in crate::app) fn update(
        &mut self,
        pinned: &[Branch],
        branches: &[Branch],
        mut working: impl FnMut(&Branch) -> bool,
    ) {
        fn collect(
            rows: &[Branch],
            working: &mut impl FnMut(&Branch) -> bool,
            keys: &mut Vec<String>,
        ) -> bool {
            let mut any = false;
            for row in rows {
                let descendants = collect(&row.children, working, keys);
                if working(row) || descendants {
                    keys.push(row.key.clone());
                    any = true;
                }
            }
            any
        }
        let mut keys = Vec::new();
        collect(pinned, &mut working, &mut keys);
        collect(branches, &mut working, &mut keys);
        let current: std::collections::HashSet<_> = keys.iter().collect();
        self.ranks.retain(|key, _| current.contains(key));
        let mut next = self.ranks.values().max().map_or(0, |rank| rank + 1);
        for key in keys {
            self.ranks.entry(key).or_insert_with(|| {
                let rank = next;
                next += 1;
                rank
            });
        }
    }

    pub(super) fn compare(&self, a: &Branch, b: &Branch) -> Ordering {
        match (self.ranks.get(&a.key), self.ranks.get(&b.key)) {
            (Some(a), Some(b)) => a.cmp(b),
            (Some(_), None) => Ordering::Less,
            (None, Some(_)) => Ordering::Greater,
            (None, None) => Ordering::Equal,
        }
    }

    pub(in crate::app) fn compare_groups(
        &self,
        a: &[usize],
        b: &[usize],
        rows: &[Branch],
    ) -> Ordering {
        let first = |indices: &[usize]| {
            indices
                .iter()
                .filter_map(|index| self.ranks.get(&rows[*index].key))
                .min()
        };
        match (first(a), first(b)) {
            (Some(a), Some(b)) => a.cmp(b),
            (Some(_), None) => Ordering::Less,
            (None, Some(_)) => Ordering::Greater,
            (None, None) => Ordering::Equal,
        }
    }

    /// Children previously kept discovery order. Only promote running children;
    /// do not impose the root's date/name sort on idle worker histories.
    pub(in crate::app) fn sort_children(&self, rows: &mut [Branch]) {
        for row in rows {
            row.children.sort_by(|a, b| self.compare(a, b));
            self.sort_children(&mut row.children);
        }
    }
}
