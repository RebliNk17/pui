//! Retire only settled RPC processes. Conversation and explicit Stop are separate lifecycles.
use super::*;
use std::{collections::BTreeSet, path::Path, time::Instant};

const IDLE_TIMEOUT: Duration = Duration::from_secs(10 * 60);

#[derive(Default)]
pub(in crate::app) struct Idle {
    pub(super) since: BTreeMap<String, Instant>,
    pub(in crate::app) dormant: BTreeSet<String>,
    pub(in crate::app) retiring: BTreeMap<String, (PathBuf, Receiver<()>)>,
    pub(super) wake: BTreeSet<String>,
    // Records are sent once, only after reconnect + adapter readiness. The matching
    // agents.mail entry retains completion-report correlation until the real ack.
    pub(super) mail: BTreeMap<String, Value>,
    pub(super) next_mail: u64,
}

impl Pui {
    fn idle_agent_eligible(&self, id: &str) -> bool {
        let Some(node) = self.agents.nodes.get(id) else {
            return false;
        };
        let Some(path) = node.path.as_ref() else {
            return false;
        };
        if !node.settled
            || node.stopped
            || node.failure.is_some()
            || node.setup_pending()
            || (node.needs_report && !node.reported)
            || self.agents.idle.dormant.contains(id)
            || self.agents.permission_changes.contains_key(id)
            || self.deletion.deleting_agent(id)
            || self.deletion.deleting_chat(path)
            || self
                .waiting
                .get(path)
                .is_some_and(|queue| !queue.is_empty())
            || self
                .agents
                .mail
                .values()
                .any(|(target, reports)| target == id || reports.iter().any(|r| r == id))
            || self.agents.pending_side_reports.contains_key(id)
            || self
                .agents
                .pending_side_reports
                .values()
                .any(|reports| reports.iter().any(|r| r.agent_id == id))
            || self
                .agents
                .waits
                .iter()
                .any(|wait| wait.source == id || wait.ids.iter().any(|target| target == id))
            || self.has_active_automation(id, path)
            || self.tasks.records.iter().any(|task| {
                (task.phase.active() || task.phase == crate::tasks::Phase::AwaitingApproval)
                    && (self
                        .tasks
                        .parents
                        .get(&task.id)
                        .is_some_and(|parent| parent == id)
                        || task.parent_session.as_ref() == Some(path)
                        || task
                            .runs
                            .iter()
                            .any(|run| run.session.as_ref() == Some(path)))
            })
            || self.agents.nodes.values().any(|other| {
                other.id != id
                    && !other.stopped
                    && other.failure.is_none()
                    && !other.settled
                    && (self.descendants(id, &other.id) || other.depends.iter().any(|d| d == id))
            })
        {
            return false;
        }
        let (state, connected, loading, client, blocked) = if id == self.agents.active {
            (
                &self.state,
                self.connected,
                self.loading_chat,
                self.client.is_some(),
                self.pending_session.is_some()
                    || self.preview_rx.is_some()
                    || self.history_pending.is_some()
                    || self.resource_reload.pending()
                    || self.resource_reload.connector_pending()
                    || self.resource_reload.confirming()
                    || self.interrupt.is_some()
                    || !self.deliveries.is_empty()
                    || self.pending_dialog.is_some()
                    || self.dictation.active()
                    || self.restoring_code(),
            )
        } else if let Some(chat) = self.agents.parked.get(id) {
            (
                &chat.state,
                chat.connected,
                chat.loading,
                chat.client.is_some(),
                chat.pending.is_some()
                    || chat.preview.is_some()
                    || chat.pending_dialog.is_some()
                    || chat.deletion_busy(),
            )
        } else {
            return false;
        };
        client
            && connected
            && !loading
            && !blocked
            && state.session_file.as_deref().map(Path::new) == Some(path.as_path())
            && (!self.sync.enabled() || self.sync.owns(path))
            && state.ready()
            && state.error.is_none()
            && !state.run_observed
            && !state.retrying
            && state.stream.is_none()
            && state.dialogs.is_empty()
            && state.pending_messages.iter().all(|receipt| {
                matches!(
                    receipt.status,
                    protocol::DeliveryStatus::Handled | protocol::DeliveryStatus::Cancelled
                )
            })
            && state
                .messages
                .last()
                .is_none_or(|message| message.tools.iter().all(|tool| tool.finished))
    }

    pub(in crate::app) fn poll_idle_agents(&mut self, now: Instant) {
        self.finish_idle_retirements();
        self.deliver_idle_mail();
        if self.demo || self.agents.exiting || !self.sync.can_evict_owned() {
            self.agents.idle.since.clear();
            return;
        }
        // Interaction with the selected chat resets its timer, not every worker's.
        if self.ctx.input(|input| !input.events.is_empty()) {
            self.agents.idle.since.remove(&self.agents.active);
        }
        let ids: Vec<_> = self.agents.nodes.keys().cloned().collect();
        for id in ids {
            if !self.idle_agent_eligible(&id) {
                self.agents.idle.since.remove(&id);
                continue;
            }
            let since = *self.agents.idle.since.entry(id.clone()).or_insert(now);
            let remaining = IDLE_TIMEOUT.saturating_sub(now.saturating_duration_since(since));
            if remaining.is_zero() {
                self.retire_idle_agent(&id);
            } else {
                // One deadline wake; no recurring idle poll or model call.
                self.ctx.request_repaint_after(remaining);
            }
        }
    }

    fn retire_idle_agent(&mut self, id: &str) {
        let path = self.agents.nodes[id].path.clone().unwrap();
        if !path.is_file() {
            self.agents.idle.since.remove(id);
            return;
        }
        // Drain newly arrived questions/work before the final eligibility check.
        let activity = self
            .with_live_chat(&path, |app| app.poll_rpc())
            .unwrap_or(true);
        if !self.idle_agent_eligible(id) {
            self.agents.idle.since.remove(id);
            return;
        }
        if activity {
            self.agents.idle.since.insert(id.into(), Instant::now());
            self.ctx.request_repaint_after(IDLE_TIMEOUT);
            return;
        }
        // No abort/clear_queue, Stop tree, workflow cancellation or history eviction.
        let client = if id == self.agents.active {
            self.connected = false;
            self.ultracode_ready = false;
            self.client.take()
        } else if let Some(chat) = self.agents.parked.get_mut(id) {
            chat.connected = false;
            chat.ready = false;
            chat.client.take()
        } else {
            None
        };
        let Some(client) = client else { return };
        self.agents.idle.since.remove(id);
        self.agents.idle.dormant.insert(id.into());
        self.agents.nodes.get_mut(id).unwrap().status = "Idle · process released".into();
        let (tx, rx) = mpsc::channel();
        self.agents.idle.retiring.insert(id.into(), (path, rx));
        let ctx = self.ctx.clone();
        std::thread::spawn(move || {
            drop(client); // Existing graceful stdin close + bounded termination/reaping.
            let _ = tx.send(());
            ctx.request_repaint();
        });
    }

    fn finish_idle_retirements(&mut self) {
        let done: Vec<_> = self
            .agents
            .idle
            .retiring
            .iter()
            .filter(|(_, (_, rx))| !matches!(rx.try_recv(), Err(mpsc::TryRecvError::Empty)))
            .map(|(id, _)| id.clone())
            .collect();
        for id in done {
            let (path, _) = self.agents.idle.retiring.remove(&id).unwrap();
            // Keep single-writer ownership through actual process exit.
            self.sync.release(&path);
        }
        let waking: Vec<_> = self.agents.idle.wake.iter().cloned().collect();
        for id in waking {
            if self.agents.idle.retiring.contains_key(&id) {
                continue;
            }
            self.agents.idle.wake.remove(&id);
            if !self.agents.idle.dormant.contains(&id)
                || self.agents.nodes.get(&id).is_none_or(|node| node.stopped)
            {
                continue;
            }
            if let Err(error) = self.reconnect_job_owner(&id) {
                self.fail_idle_mail(&id, &error);
                if let Some(path) = self
                    .agents
                    .nodes
                    .get(&id)
                    .and_then(|node| node.path.clone())
                {
                    self.with_live_chat(&path, |app| app.state.error = Some(error));
                }
            }
        }
    }

    pub(in crate::app) fn wake_idle_agent(&mut self, id: &str) -> Result<(), String> {
        if self.agents.nodes.get(id).is_none_or(|node| node.stopped) {
            return Err("Agent was explicitly stopped; resume it in its chat.".into());
        }
        if self.agents.idle.dormant.contains(id) {
            let connected = if id == self.agents.active {
                self.client.is_some()
            } else {
                self.agents
                    .parked
                    .get(id)
                    .is_some_and(|chat| chat.client.is_some())
            };
            if connected {
                return Ok(());
            }
        }
        if self.agents.idle.retiring.contains_key(id) {
            self.agents.idle.wake.insert(id.into());
            return Ok(());
        }
        self.reconnect_job_owner(id)
    }

    fn fail_idle_mail(&mut self, id: &str, error: &str) {
        let keys: Vec<_> = self
            .agents
            .idle
            .mail
            .keys()
            .filter(|key| {
                self.agents
                    .mail
                    .get(*key)
                    .is_some_and(|(target, _)| target == id)
            })
            .cloned()
            .collect();
        for key in keys {
            self.agents.idle.mail.remove(&key);
            self.agent_mail_response(&json!({"id":key,"success":false,"error":error}));
        }
    }

    fn deliver_idle_mail(&mut self) {
        // A failed reconnect is not evidence that a queued prompt was accepted.
        // Surface the error and never automatically retry it.
        let failed: Vec<_> = self
            .agents
            .idle
            .dormant
            .iter()
            .filter_map(|id| {
                let chat = self.agents.parked.get(id);
                let state = if id == &self.agents.active {
                    Some(&self.state)
                } else {
                    chat.map(|c| &c.state)
                };
                state
                    .and_then(|s| s.error.as_ref())
                    .map(|error| (id.clone(), error.clone()))
            })
            .collect();
        for (id, error) in failed {
            self.fail_idle_mail(&id, &error);
        }
        let ids: Vec<_> = self.agents.idle.dormant.iter().cloned().collect();
        for id in ids {
            if self.agents.idle.retiring.contains_key(&id) {
                continue;
            }
            let has_client = if id == self.agents.active {
                self.client.is_some()
            } else {
                self.agents
                    .parked
                    .get(&id)
                    .is_some_and(|chat| chat.client.is_some())
            };
            if !has_client {
                continue;
            }
            let Some(path) = self
                .agents
                .nodes
                .get(&id)
                .and_then(|node| node.path.clone())
            else {
                continue;
            };
            let ready = self
                .with_live_chat(&path, |app| {
                    if app.client.is_none()
                        || app.loading_chat
                        || !app.connected
                        || !app.ultracode_ready
                    {
                        return false;
                    }
                    // Reconnecting is read-only. Leave questions, errors and uncertain
                    // deliveries untouched rather than blindly replaying a prompt.
                    app.state.ready()
                        && !app.state.run_observed
                        && app.state.dialogs.is_empty()
                        && app.state.error.is_none()
                        && app.state.pending_messages.iter().all(|receipt| {
                            matches!(
                                receipt.status,
                                protocol::DeliveryStatus::Handled
                                    | protocol::DeliveryStatus::Cancelled
                            )
                        })
                })
                .unwrap_or(false);
            if !ready {
                continue;
            }
            self.agents.idle.dormant.remove(&id);
            self.agents.idle.since.remove(&id);
            let keys: Vec<_> = self
                .agents
                .idle
                .mail
                .keys()
                .filter(|key| {
                    self.agents
                        .mail
                        .get(*key)
                        .is_some_and(|(target, _)| target == &id)
                })
                .cloned()
                .collect();
            for key in keys {
                let record = self.agents.idle.mail.remove(&key).unwrap();
                let result = self
                    .with_live_chat(&path, |app| {
                        let request = app
                            .client
                            .as_mut()
                            .ok_or("Agent disconnected before delivery")?
                            .enqueue(record)?;
                        app.state.pending = true;
                        Ok::<_, String>(format!("{id}:{request}"))
                    })
                    .unwrap_or_else(|| Err("Agent chat unavailable".into()));
                match result {
                    Ok(new_key) => {
                        if let Some(correlation) = self.agents.mail.remove(&key) {
                            self.agents.mail.insert(new_key, correlation);
                        }
                    }
                    Err(error) => {
                        self.agent_mail_response(&json!({"id":key,"success":false,"error":error}))
                    }
                }
            }
            self.with_live_chat(&path, |app| app.deliver_waiting());
        }
    }
}

#[cfg(test)]
mod tests;
