//! Presentation only: root membership, numbered labels and selection. No PTY state,
//! output, command, environment, approval or working-directory override is saved.
use super::*;
use std::collections::BTreeSet;

const MAX_ROOTS: usize = tabs::MAX_TOTAL_TABS;
const MAX_LABEL: u64 = 9999;
mod decoder;

#[derive(Clone, Default, Serialize)]
pub(in crate::app) struct SavedTabs {
    projects: Vec<SavedProject>,
}
#[derive(Clone, Default, Serialize)]
struct SavedProject {
    root: PathBuf,
    labels: Vec<u64>,
    selected: Option<u64>,
}

fn valid_root(root: &Path) -> bool {
    root.is_absolute()
        && root
            .to_str()
            .is_some_and(|value| value.len() <= 4096 && !value.chars().any(char::is_control))
        && !root.components().any(|part| {
            matches!(
                part,
                std::path::Component::ParentDir | std::path::Component::CurDir
            )
        })
}
impl SavedTabs {
    fn bound(&mut self) {
        let mut roots = BTreeSet::new();
        let mut remaining = tabs::MAX_TOTAL_TABS;
        self.projects.truncate(MAX_ROOTS);
        self.projects.retain_mut(|project| {
            if !valid_root(&project.root) || !roots.insert(project.root.clone()) {
                return false;
            }
            project.labels.truncate(tabs::MAX_TABS.min(remaining));
            let mut used = BTreeSet::new();
            for label in &mut project.labels {
                if !(1..=MAX_LABEL).contains(label) || !used.insert(*label) {
                    *label = (1..=MAX_LABEL).find(|label| !used.contains(label)).unwrap();
                    used.insert(*label);
                }
            }
            remaining -= project.labels.len();
            project.selected = project
                .selected
                .filter(|label| project.labels.contains(label))
                .or_else(|| project.labels.first().copied());
            true
        });
    }
    pub(in crate::app) fn retain_unopened(&mut self, other: &Self) {
        // Live groups (even deliberately empty ones) always win over stale checkpoints.
        for project in &other.projects {
            if self.projects.len() >= MAX_ROOTS {
                break;
            }
            if !self.projects.iter().any(|saved| saved.root == project.root) {
                self.projects.push(project.clone());
            }
        }
        self.bound();
    }
}
impl Terminals {
    pub(in crate::app) fn saved_tabs(&self) -> SavedTabs {
        let mut saved = SavedTabs {
            projects: self
                .projects
                .iter()
                .map(|(root, group)| SavedProject {
                    root: root.clone(),
                    labels: group.tabs.iter().map(|tab| tab.label).collect(),
                    selected: group.active().map(|tab| tab.label),
                })
                .collect(),
        };
        // Legacy layout-only checkpoints also survive a save before activation.
        for root in &self.restored_roots {
            if !self.projects.contains_key(root) {
                saved.projects.push(SavedProject {
                    root: root.clone(),
                    labels: vec![1],
                    selected: Some(1),
                });
            }
        }
        saved.bound();
        saved
    }
    pub(in crate::app) fn restore_tabs(&mut self, saved: &SavedTabs) {
        // A bounded checkpoint can omit overflow roots. Even an unknown project
        // stays inert in a restored window rather than becoming an implicit replay.
        self.restore_guard = true;
        let mut saved = saved.clone();
        saved.bound();
        for project in saved.projects {
            if self.projects.contains_key(&project.root)
                || self.projects.len() >= MAX_ROOTS
                || (!self.restored_roots.contains(&project.root)
                    && self.projects.len() + self.restored_roots.len() >= MAX_ROOTS)
            {
                continue;
            }
            let reserved = usize::from(self.restored_roots.contains(&project.root));
            let remaining =
                tabs::MAX_TOTAL_TABS.saturating_sub(self.total_tabs().saturating_sub(reserved));
            let labels: Vec<_> = project.labels.into_iter().take(remaining).collect();
            let selected = labels
                .iter()
                .position(|label| Some(*label) == project.selected)
                .unwrap_or(0);
            let next_label = labels.iter().copied().max().unwrap_or(0);
            self.restored_roots.remove(&project.root);
            let tabs = labels
                .into_iter()
                .map(|label| Tab {
                    id: next_tab_id(),
                    tab_id: self.next_short_tab_id(),
                    label,
                    restored: true,
                    ..Default::default()
                })
                .collect();
            self.projects.insert(
                project.root,
                Project {
                    tabs,
                    selected,
                    next_label,
                },
            );
        }
    }
    pub(super) fn activate_restored(&mut self, ctx: &egui::Context, demo: bool) {
        let Some(tab) = self
            .projects
            .get_mut(&self.root)
            .and_then(Project::active_mut)
        else {
            return;
        };
        if !tab.restored {
            return;
        }
        tab.restored = false;
        launch(tab, ctx, &self.root, demo);
        self.focus = true;
        capture::clear(ctx);
    }
}

#[cfg(test)]
mod tests;
#[cfg(test)]
mod ui_tests;
