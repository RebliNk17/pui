//! Separate confirmed local fast-forward; uses Fetch form's remote/branch draft.
use super::*;
impl Changes {
    pub(super) fn fast_forward_dialog(&mut self, ctx: &egui::Context) {
        let Some(review) = &self.fetch.update_review else {
            return;
        };
        let own = egui::LayerId::new(egui::Order::Foreground, egui::Id::new("git-ff-confirm"));
        if !self.index_writable
            || self.fetch.owner.as_ref() != Some(&self.identity)
            || egui::Popup::is_any_open(ctx)
            || prompt_ui::modal_owns_current_pass(ctx)
            || ctx.memory(|m| m.top_modal_layer().is_some_and(|l| l != own))
        {
            self.fetch.update_review = None;
            return;
        }
        let choice=confirmations::Dialog::new("git-ff-confirm","Fast-forward current branch?","Updates current branch, index and working files to this already-fetched commit. Requires clean state, including no ignored/untracked files, no active operations, no checkout filters or submodule trees. No network, merge commit, automatic stash, force or hooks. External changes can race; inspect checkout after any failure. Cancellation does not promise rollback.","Cancel","Update branch").destructive(false).show(ctx,|ui|{ui.add(egui::Label::new(theme::small(ui,format!("{}\n{}\n{} → {}\nSource: {}",review.root.display(),review.branch,review.head,review.target,review.tracking))).wrap());ui.label(theme::small(ui,format!("{} changed paths",review.files.len())));egui::ScrollArea::both().max_height(120.0).show_rows(ui,16.0,review.files.len(),|ui,range|{for f in &review.files[range]{ui.add(egui::Label::new(theme::small(ui,format!("{} {}",f.status,f.path.escape_debug()))).extend());}});});
        if let Some(choice) = choice {
            let review = self.fetch.update_review.take().unwrap();
            if choice == confirmations::Choice::Confirm {
                let Some(permit) = branches::permit(ctx) else {
                    self.fetch.notice = Some("Git busy; review again.".into());
                    return;
                };
                let remote = self.fetch.remote.clone();
                let name = self.fetch.branch.clone();
                let (tx, rx) = mpsc::channel();
                let cancel = Arc::new(AtomicBool::new(false));
                self.fetch.update_applying =
                    Some(workspace::read_only::Request::new(rx, cancel.clone()));
                let ctx = ctx.clone();
                if std::thread::Builder::new()
                    .name("pui-fast-forward".into())
                    .spawn(move || {
                        let result =
                            workspace::fast_forward::apply(review, &remote, &name, &cancel);
                        drop(permit);
                        let _ = tx.send(result);
                        ctx.request_repaint();
                    })
                    .is_err()
                {
                    self.fetch.update_applying = None;
                    self.fetch.notice = Some("Cannot start fast-forward.".into());
                }
            }
        }
        ctx.input_mut(|i| {
            i.events.retain(|e| {
                !matches!(
                    e,
                    egui::Event::Key { .. } | egui::Event::Text(_) | egui::Event::Paste(_)
                )
            })
        });
    }
}
