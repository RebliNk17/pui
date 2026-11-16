//! Save reviewed explicit destination as local upstream, never network intent.
use super::*;
#[derive(Default)]
pub(super) struct Upstream {
    pub(super) confirm: bool,
    pub(super) pending: Option<workspace::read_only::Request<Result<(), String>>>,
}
impl Changes {
    pub(super) fn upstream_poll(&mut self) {
        if let Some(result) = self
            .push
            .upstream
            .pending
            .as_ref()
            .and_then(|r| match r.try_recv() {
                Ok(r) => Some(r),
                Err(mpsc::TryRecvError::Empty) => None,
                Err(_) => Some(Err(
                    "Upstream creation ended; inspect config before retry.".into()
                )),
            })
        {
            self.push.upstream.pending = None;
            self.push.snapshot = None;
            self.push.notice=Some(match result{Ok(())=>"Upstream saved locally. Inspect default destination to review it. No fetch or push.".into(),Err(e)=>e});
        }
    }
    pub(super) fn upstream_dialog(&mut self, ctx: &egui::Context) {
        let own = egui::LayerId::new(
            egui::Order::Foreground,
            egui::Id::new("save-upstream-confirm"),
        );
        if !self.index_writable
            || egui::Popup::is_any_open(ctx)
            || prompt_ui::modal_owns_current_pass(ctx)
            || ctx.memory(|m| m.top_modal_layer().is_some_and(|l| l != own))
        {
            self.push.upstream.confirm = false;
            return;
        }
        let Some(snapshot) = &self.push.snapshot else {
            self.push.upstream.confirm = false;
            return;
        };
        let choice=confirmations::Dialog::new("save-upstream-confirm","Save this branch’s upstream?","Adds this remote and destination as the local branch’s default upstream. Existing upstream settings are never replaced. This affects other Git tools too. No network, fetch or push.","Cancel","Save upstream").destructive(false).show(ctx,|ui|{ui.add(egui::Label::new(theme::small(ui,format!("{}\n{} → {}",snapshot.branch,snapshot.remote,snapshot.destination))).wrap());});
        if let Some(choice) = choice {
            self.push.upstream.confirm = false;
            if choice == confirmations::Choice::Confirm {
                let snapshot = snapshot.clone();
                let Some(permit) = branches::permit(ctx) else {
                    self.push.notice = Some("Git busy; retry explicitly.".into());
                    return;
                };
                let (tx, rx) = mpsc::channel();
                let cancel = Arc::new(AtomicBool::new(false));
                self.push.upstream.pending =
                    Some(workspace::read_only::Request::new(rx, cancel.clone()));
                let ctx = ctx.clone();
                if std::thread::Builder::new()
                    .name("pui-save-upstream".into())
                    .spawn(move || {
                        let result = workspace::push::remote::set_upstream(&snapshot, &cancel);
                        drop(permit);
                        let _ = tx.send(result);
                        ctx.request_repaint();
                    })
                    .is_err()
                {
                    self.push.upstream.pending = None;
                    self.push.notice = Some("Cannot start upstream configuration worker.".into());
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
