//! Bounded execution-time identity, never executable intent or authenticated proof.
use super::*;
use sha2::{Digest, Sha256};

#[derive(Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(in crate::app::automation) enum Origin {
    Scheduled,
    TestOnce,
}
#[derive(Clone, Serialize, Deserialize)]
pub(in crate::app::automation) struct Identity {
    sha256: String,
    excerpt: String,
    shortened: bool,
    canonical: bool,
}
impl Identity {
    fn path(path: &std::path::Path) -> Self {
        #[cfg(test)]
        COLLECTIONS.with(|counts| counts.set((counts.get().0 + 1, counts.get().1)));
        // A failed canonicalization must not masquerade as a resolved path.
        let resolved = path.canonicalize().ok();
        let canonical = resolved.is_some();
        let path = resolved.as_deref().unwrap_or(path);
        let mut excerpt = path.display().to_string();
        let shortened = excerpt.len() > 128;
        truncate(&mut excerpt, 128);
        Self {
            sha256: digest(path.as_os_str().as_encoded_bytes()),
            excerpt,
            shortened,
            canonical,
        }
    }
    fn valid(&self) -> bool {
        valid_digest(&self.sha256) && self.excerpt.len() <= 128
    }
    fn show(&self, ui: &mut egui::Ui, label: &str) {
        literal(
            ui,
            format!(
                "{label}: {}{}",
                self.excerpt,
                if self.shortened {
                    "… (path excerpt)"
                } else {
                    ""
                }
            ),
        );
        literal(
            ui,
            if self.canonical {
                "Resolved path at capture (not a filesystem-content snapshot)."
            } else {
                "Canonicalization unavailable: identity uses the supplied path, not a verified resolved location."
            },
        );
        literal(ui, format!("{label} identity SHA-256: {}", self.sha256));
    }
}
#[derive(Clone, Serialize, Deserialize)]
pub(in crate::app::automation) struct Provenance {
    origin: Origin,
    started: i64,
    job_id: String,
    creator_agent_id: String,
    command_sha256: String,
    timeout_seconds: u64,
    workspace: Identity,
    session: Identity,
    store: Option<Identity>,
}
/// One explicit inspection snapshot, held only in memory and never serialized.
/// Exact input comparisons invalidate it without filesystem IO or hashing.
#[derive(Clone)]
pub(in crate::app::automation) struct Comparison {
    job_id: String,
    creator_id: String,
    command: String,
    timeout: u64,
    workspace: PathBuf,
    session: PathBuf,
    store: Option<PathBuf>,
    owner: bool,
    requested: i64,
    captured: Provenance,
}
impl Comparison {
    pub(in crate::app::automation) fn collect(
        job: &Job,
        store: Option<&std::path::Path>,
        owner: bool,
    ) -> Option<Self> {
        let Trigger::Monitor {
            command,
            timeout_seconds,
            ..
        } = &job.trigger
        else {
            return None;
        };
        let requested = now();
        Some(Self {
            job_id: job.id.clone(),
            creator_id: job.creator.id.clone(),
            command: command.clone(),
            timeout: *timeout_seconds,
            workspace: job.creator.cwd.clone(),
            session: job.creator.path.clone(),
            store: store.map(ToOwned::to_owned),
            owner,
            requested,
            captured: Provenance::capture(job, store, Origin::Scheduled, requested)?,
        })
    }
    pub(in crate::app::automation) fn applies(
        &self,
        job: &Job,
        store: Option<&std::path::Path>,
        owner: bool,
    ) -> bool {
        matches!(&job.trigger, Trigger::Monitor { command, timeout_seconds, .. }
            if command == &self.command && timeout_seconds == &self.timeout)
            && self.job_id == job.id
            && self.creator_id == job.creator.id
            && self.workspace == job.creator.cwd
            && self.session == job.creator.path
            && self.store.as_deref() == store
            && self.owner == owner
    }
}
impl Provenance {
    pub(in crate::app::automation) fn capture(
        job: &Job,
        store: Option<&std::path::Path>,
        origin: Origin,
        started: i64,
    ) -> Option<Self> {
        let Trigger::Monitor {
            command,
            timeout_seconds,
            ..
        } = &job.trigger
        else {
            return None;
        };
        Some(Self {
            origin,
            started,
            job_id: job.id.clone(),
            creator_agent_id: job.creator.id.clone(),
            command_sha256: digest(command.as_bytes()),
            timeout_seconds: *timeout_seconds,
            workspace: Identity::path(&job.creator.cwd),
            session: Identity::path(&job.creator.path),
            store: store.map(Identity::path),
        })
    }
    pub(super) fn valid(&self, observed: i64) -> bool {
        // At most 4 KiB/check (16 KiB/job); the existing 4 MiB whole-store
        // cap remains authoritative and fails closed instead of evicting jobs.
        serde_json::to_vec(self).is_ok_and(|bytes| bytes.len() <= 4096)
            && self.started >= 0
            && observed >= 0
            && uuid::Uuid::parse_str(&self.job_id).is_ok()
            && self.creator_agent_id.len() <= 32
            && self
                .creator_agent_id
                .strip_prefix("agent-")
                .and_then(|id| id.parse::<u64>().ok())
                .is_some()
            && valid_digest(&self.command_sha256)
            && (1..=300).contains(&self.timeout_seconds)
            && self.workspace.valid()
            && self.session.valid()
            && self.store.as_ref().is_none_or(Identity::valid)
    }
    pub(super) fn origin_label(&self) -> &'static str {
        match self.origin {
            Origin::Scheduled => "Scheduled execution",
            Origin::TestOnce => "Test once · no follow-up delivery",
        }
    }
    pub(super) fn show(
        &self,
        ui: &mut egui::Ui,
        job: &Job,
        store: Option<&std::path::Path>,
        owner: bool,
        comparison: Option<&Comparison>,
    ) -> bool {
        literal(
            ui,
            format!(
                "Launch requested: {} · {} · timeout {}s",
                timestamp(self.started),
                self.origin_label(),
                self.timeout_seconds
            ),
        );
        literal(
            ui,
            format!(
                "Job at execution: {} · creator agent at execution: {}",
                self.job_id, self.creator_agent_id
            ),
        );
        literal(
            ui,
            "Saved local metadata, not authenticated proof. Resolved paths are recorded when available; SHA-256 identifies exact bytes, not workspace contents or task correctness.",
        );
        let comparison = comparison.filter(|comparison| comparison.applies(job, store, owner));
        let current = comparison.map(|comparison| &comparison.captured);
        if let Some(comparison) = comparison {
            literal(
                ui,
                format!(
                    "Last explicit comparison: {} · filesystem changes are not watched; refresh to collect again.",
                    timestamp(comparison.requested)
                ),
            );
        } else {
            literal(
                ui,
                "Current comparison unavailable or invalidated. Compare explicitly to collect path identities; viewing alone does not collect them.",
            );
        }
        let refresh = ui
            .add_enabled(
                prompt_ui::keyboard_available(ui),
                egui::Button::new("Compare current identities"),
            )
            .clicked();
        let matches = current.is_some_and(|current| {
            self.job_id == current.job_id
                && self.command_sha256 == current.command_sha256
                && self.workspace.sha256 == current.workspace.sha256
                && self.workspace.canonical == current.workspace.canonical
                && self.session.sha256 == current.session.sha256
                && self.session.canonical == current.session.canonical
                && self.timeout_seconds == current.timeout_seconds
        });
        literal(
            ui,
            if matches {
                "Command, timeout, job, workspace and creator session matched the last explicit comparison. Agent IDs may be remapped on restart."
            } else {
                "Current execution identity differs or is unavailable. Do not attribute this check to the current command/workspace/session."
            },
        );
        let same_store = self
            .store
            .as_ref()
            .zip(current.and_then(|p| p.store.as_ref()))
            .map(|(saved, current)| {
                saved.sha256 == current.sha256 && saved.canonical == current.canonical
            });
        literal(
            ui,
            match same_store {
                Some(true) => {
                    "Store identity matched the last explicit comparison; this does not establish scheduler ownership."
                }
                Some(false) => "Store identity differs from this view (copied or moved evidence).",
                None => "Store identity not recorded or unavailable in this view.",
            },
        );
        literal(
            ui,
            format!("Exact command SHA-256: {}", self.command_sha256),
        );
        if let Trigger::Monitor { command, .. } = &job.trigger
            && self.job_id == job.id
            && same_store == Some(true)
            && current.is_some_and(|current| current.command_sha256 == self.command_sha256)
        {
            literal(
                ui,
                "Matching current command (historical command text is not duplicated in evidence):",
            );
            literal(ui, command);
        } else {
            literal(
                ui,
                "Historical command text not retained; digest cannot reconstruct it.",
            );
        }
        self.workspace.show(ui, "Workspace at execution");
        self.session.show(ui, "Creator session at execution");
        if let Some(store) = &self.store {
            store.show(ui, "Scheduler store at execution");
        }
        refresh
    }
}
#[cfg(test)]
thread_local! {
    static COLLECTIONS: std::cell::Cell<(usize, usize)> = const { std::cell::Cell::new((0, 0)) };
}
fn digest(bytes: &[u8]) -> String {
    #[cfg(test)]
    COLLECTIONS.with(|counts| counts.set((counts.get().0, counts.get().1 + 1)));
    crate::hex::lower(Sha256::digest(bytes))
}
fn valid_digest(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}
fn timestamp(seconds: i64) -> String {
    chrono::DateTime::from_timestamp(seconds, 0)
        .map(|t| t.format("%Y-%m-%d %H:%M:%S UTC").to_string())
        .unwrap_or_else(|| "Unknown time".into())
}
pub(super) fn literal(ui: &mut egui::Ui, value: impl AsRef<str>) {
    let value: String = value
        .as_ref()
        .chars()
        .flat_map(|c| {
            if c.is_control() && !matches!(c, '\n' | '\r' | '\t') {
                c.escape_default().collect::<Vec<_>>()
            } else {
                vec![c]
            }
        })
        .collect();
    ui.add(
        egui::Label::new(theme::small(ui, value))
            .wrap()
            .selectable(false),
    );
}

#[cfg(test)]
mod cache_tests;
#[cfg(test)]
mod tests;
