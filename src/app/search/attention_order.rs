//! Stable chat identity order for one explicit search opening. No filesystem work.
use super::*;
use std::collections::BTreeMap;

const MAX_KEYS: usize = 512;
const MAX_KEY_BYTES: usize = 4096;
const MAX_TOTAL_BYTES: usize = 128 * 1024;
type Rank = (usize, Option<attention::SearchEvent>);

#[derive(Default)]
pub(super) struct Order {
    saved: BTreeMap<PathBuf, Rank>,
    agents: BTreeMap<String, Rank>,
    tasks: BTreeMap<String, BTreeMap<Option<String>, Rank>>,
}
impl Order {
    pub(super) fn snapshot(
        candidates: impl Iterator<Item = (Candidate, Option<attention::SearchEvent>)>,
    ) -> Self {
        let mut order = Self::default();
        let mut bytes = 0usize;
        for (index, (candidate, event)) in candidates.take(MAX_KEYS).enumerate() {
            let cost = if let Some(path) = &candidate.path {
                path.capacity()
            } else {
                match &candidate.target {
                    sidebar_tree::Target::Agent(id) => id.capacity(),
                    sidebar_tree::Target::Task(task, run) => {
                        task.capacity() + run.as_ref().map_or(0, String::capacity)
                    }
                    _ => continue,
                }
            };
            if cost > MAX_KEY_BYTES || bytes + cost > MAX_TOTAL_BYTES {
                continue;
            }
            let rank = (index, event);
            let old = if let Some(path) = candidate.path {
                order.saved.entry(path).or_insert(rank)
            } else {
                match candidate.target {
                    sidebar_tree::Target::Agent(id) => order.agents.entry(id).or_insert(rank),
                    sidebar_tree::Target::Task(task, run) => order
                        .tasks
                        .entry(task)
                        .or_default()
                        .entry(run)
                        .or_insert(rank),
                    _ => continue,
                }
            };
            if old.0 == index {
                bytes += cost;
            }
        }
        order
    }
    pub(super) fn get(&self, candidate: &Candidate) -> Rank {
        // Borrow identity keys in settled frames; no path/string copies or stat.
        if let Some(path) = &candidate.path {
            return self.saved.get(path).copied().unwrap_or((MAX_KEYS, None));
        }
        match &candidate.target {
            sidebar_tree::Target::Agent(id) => self.agents.get(id),
            sidebar_tree::Target::Task(task, run) => {
                self.tasks.get(task).and_then(|runs| runs.get(run))
            }
            _ => None,
        }
        .copied()
        .unwrap_or((MAX_KEYS, None))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn candidate(index: usize) -> Candidate {
        Candidate {
            title: String::new(),
            project: PathBuf::new(),
            cwd: PathBuf::new(),
            path: Some(PathBuf::from(format!("/fixture/{index:03}"))),
            target: sidebar_tree::Target::Saved(PathBuf::new()),
        }
    }
    #[test]
    fn frozen_identity_order_caps_keys_and_capacity_without_titles_bodies_or_duplicate_growth() {
        let order = Order::snapshot((0..600).map(|index| (candidate(index), None)));
        assert_eq!(order.saved.len(), MAX_KEYS);
        assert_eq!(order.get(&candidate(3)).0, 3);
        assert_eq!(order.get(&candidate(513)).0, MAX_KEYS);
        let candidates = (0..MAX_KEYS).map(candidate).collect::<Vec<_>>();
        let start = std::time::Instant::now();
        for _ in 0..100 {
            let mut ranks = candidates
                .iter()
                .map(|candidate| order.get(candidate).0)
                .collect::<Vec<_>>();
            ranks.sort_unstable();
            assert_eq!(ranks[0], 0);
        }
        eprintln!(
            "100x512frozenranklookups+sort {:?}; no key copies or IO",
            start.elapsed()
        );
        let order = Order::snapshot((0..MAX_KEYS).map(|index| {
            let mut item = candidate(index);
            item.path = Some(PathBuf::from(format!("/{}-{index}", "x".repeat(4000))));
            (item, None)
        }));
        let bytes: usize = order.saved.keys().map(|path| path.capacity()).sum();
        assert!(bytes <= MAX_TOTAL_BYTES);
        assert!(order.saved.len() < MAX_KEYS);
        let order = Order::snapshot((0..MAX_KEYS).map(|_| (candidate(1), None)));
        assert_eq!(order.saved.len(), 1);
        assert_eq!(order.get(&candidate(1)).0, 0);
    }
}
