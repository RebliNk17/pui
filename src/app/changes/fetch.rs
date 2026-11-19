//! Exact branch Fetch, explicit local review then separate network confirmation.
use super::*;
mod update;
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};
#[derive(Default)]
pub(super) struct Fetch {
    owner: Option<(PathBuf, String)>,
    remote: String,
    branch: String,
    pending: Option<workspace::read_only::Request<Result<workspace::fetch::Review, String>>>,
    review: Option<workspace::fetch::Review>,
    applying: Option<workspace::read_only::Request<Result<String, String>>>,
    update_review: Option<workspace::fast_forward::Review>,
    update_pending:
        Option<workspace::read_only::Request<Result<workspace::fast_forward::Review, String>>>,
    update_applying: Option<workspace::read_only::Request<Result<(), String>>>,
    timeout: u64,
    notice: Option<String>,
    controls: Vec<egui::Id>,
}
impl Changes {
    pub(super) fn fetch_ui(&mut self, ui: &mut egui::Ui, workspace: &Workspace) {
        if !workspace.is_git {
            return;
        }
        let sheet_open = self.repo_sheet == Some(RepoSheet::Fetch);
        let modal_ok = self.repo_sheet.is_some() || ui.memory(|m| m.top_modal_layer().is_none());
        let s = &mut self.fetch;
        s.controls.clear();
        egui::CollapsingHeader::new("Fetch remote branch…").default_open(sheet_open).show(ui,|ui|{
            let enabled=self.index_writable&&s.update_pending.is_none()&&s.update_applying.is_none()&&s.update_review.is_none()&&s.pending.is_none()&&s.applying.is_none()&&s.review.is_none()&&!egui::Popup::is_any_open(ui.ctx())&&modal_ok;
            ui.add_enabled_ui(enabled,|ui|{
                for(label,value)in [("Remote name",&mut s.remote),("Branch name",&mut s.branch)]{ui.label(theme::small(ui,label));let r=ui.add(egui::TextEdit::singleline(value).char_limit(1000).desired_width(ui.available_width()));s.controls.push(r.id);}
                let update=prompt_ui::button(ui,"Review local fast-forward…",prompt_ui::Button::Secondary,workspace::push::remote::valid_name(&s.remote)&&workspace::branches::create::valid_name(&s.branch));s.controls.push(update.id);
                if update.clicked(){let Some(permit)=branches::permit(ui.ctx())else{s.notice=Some("Git busy; retry.".into());return;};let root=workspace.root.clone();let remote=s.remote.clone();let name=s.branch.clone();let(tx,rx)=mpsc::channel();let cancel=Arc::new(AtomicBool::new(false));s.owner=Some(self.identity.clone());s.update_pending=Some(workspace::read_only::Request::new(rx,cancel.clone()));let ctx=ui.ctx().clone();if std::thread::Builder::new().name("pui-ff-review".into()).spawn(move||{let result=workspace::fast_forward::inspect(&root,&remote,&name,&cancel);drop(permit);if !cancel.load(Ordering::Acquire){let _=tx.send(result);ctx.request_repaint();}}).is_err(){s.update_pending=None;s.notice=Some("Cannot inspect fast-forward.".into());}return;}
                let r=prompt_ui::button(ui,"Review Fetch…",prompt_ui::Button::Secondary,workspace::push::remote::valid_name(&s.remote)&&workspace::branches::create::valid_name(&s.branch));s.controls.push(r.id);
                if r.clicked(){let Some(permit)=branches::permit(ui.ctx())else{s.notice=Some("Git busy; retry explicitly.".into());return;};let root=workspace.root.clone();let remote=s.remote.clone();let branch=s.branch.clone();let(tx,rx)=mpsc::channel();let cancel=Arc::new(AtomicBool::new(false));s.owner=Some(self.identity.clone());s.pending=Some(workspace::read_only::Request::new(rx,cancel.clone()));let ctx=ui.ctx().clone();if std::thread::Builder::new().name("pui-fetch-review".into()).spawn(move||{let result=workspace::fetch::inspect(&root,&remote,&branch,&cancel);drop(permit);if !cancel.load(Ordering::Acquire){let _=tx.send(result);ctx.request_repaint();}}).is_err(){s.pending=None;s.notice=Some("Cannot inspect Fetch.".into());}}
            });
            if s.update_applying.is_some(){ui.label(theme::small(ui,"Updating current branch… hide view to request cancellation; inspect checkout afterward."));}
            if s.applying.is_some(){ui.label(theme::small(ui,"Fetching reviewed branch…"));if prompt_ui::button(ui,"Cancel Fetch",prompt_ui::Button::Secondary,self.index_writable).clicked(){s.applying=None;s.notice=Some("Fetch cancellation requested; objects or tracking update may already exist. Inspect before retrying.".into());}}
            if let Some(notice)=&s.notice{ui.add(egui::Label::new(theme::small(ui,notice)).wrap());}
        });
    }
    pub(super) fn fetch_inputs(&mut self, ctx: &egui::Context, visible: bool) {
        let s = &mut self.fetch;
        if !visible || !self.index_writable || s.owner.as_ref().is_some_and(|o| o != &self.identity)
        {
            *s = Default::default();
            return;
        }
        if let Some(result) = s.pending.as_ref().and_then(|r| match r.try_recv() {
            Ok(r) => Some(r),
            Err(mpsc::TryRecvError::Empty) => None,
            Err(_) => Some(Err("Fetch review ended.".into())),
        }) {
            s.pending = None;
            match result {
                Ok(review) => {
                    s.review = Some(review);
                    s.timeout = 30;
                }
                Err(e) => s.notice = Some(e),
            }
        }
        if let Some(result) = s.applying.as_ref().and_then(|r| match r.try_recv() {
            Ok(r) => Some(r),
            Err(mpsc::TryRecvError::Empty) => None,
            Err(_) => Some(Err(
                "Fetch completion uncertain; inspect tracking ref before retry.".into(),
            )),
        }) {
            s.applying = None;
            s.notice = Some(match result {
                Ok(oid) => format!(
                    "Fetched {oid}. Only the reviewed tracking ref updated; no checkout, merge or Push."
                ),
                Err(e) => e,
            });
            self.refresh = true;
        }
        if let Some(result) = s.update_pending.as_ref().and_then(|r| match r.try_recv() {
            Ok(r) => Some(r),
            Err(mpsc::TryRecvError::Empty) => None,
            Err(_) => Some(Err("Update review ended".into())),
        }) {
            s.update_pending = None;
            match result {
                Ok(review) => s.update_review = Some(review),
                Err(e) => s.notice = Some(e),
            }
        }
        if let Some(result) = s.update_applying.as_ref().and_then(|r| match r.try_recv() {
            Ok(r) => Some(r),
            Err(mpsc::TryRecvError::Empty) => None,
            Err(_) => Some(Err("Update completion uncertain; inspect checkout.".into())),
        }) {
            s.update_applying = None;
            s.notice = Some(match result {
                Ok(()) => "Current branch fast-forwarded. No network or merge commit.".into(),
                Err(e) => e,
            });
            self.refresh = true;
        }
        let focus = ctx.memory(|m| m.focused());
        if s.review.is_some()
            || s.update_review.is_some()
            || focus.is_some_and(|id| s.controls.contains(&id))
        {
            ctx.input_mut(|i|i.events.retain(|e|!matches!(e,egui::Event::Key{key:egui::Key::Enter|egui::Key::Backspace,modifiers,..}if modifiers.command||modifiers.ctrl)));
        }
    }
    pub(super) fn fetch_dialog(&mut self, ctx: &egui::Context) {
        if self.fetch.update_review.is_some() {
            self.fast_forward_dialog(ctx);
            return;
        }
        let Some(review) = &self.fetch.review else {
            return;
        };
        let own = egui::LayerId::new(egui::Order::Foreground, egui::Id::new("git-fetch-confirm"));
        if !self.index_writable
            || self.fetch.owner.as_ref() != Some(&self.identity)
            || egui::Popup::is_any_open(ctx)
            || prompt_ui::modal_owns_current_pass(ctx)
            || ctx.memory(|m| m.top_modal_layer().is_some_and(|l| l != own))
        {
            self.fetch.review = None;
            return;
        }
        let choice=confirmations::Dialog::new("git-fetch-confirm","Fetch this remote branch?","Contacts the endpoint using existing credentials, downloads objects and fast-forwards only the reviewed local tracking ref. No checkout, merge, force, tags, pruning, hooks or FETCH_HEAD update. Rewound branches refuse. Helpers/config run with normal OS permissions. Cancellation may leave objects or a completed tracking update; no automatic retry.","Cancel","Fetch branch").destructive(false).show(ctx,|ui|{ui.add(egui::Label::new(theme::small(ui,format!("{}\nrefs/heads/{} → {}\nPrevious: {}",review.endpoint,review.branch,review.tracking,review.previous.as_deref().unwrap_or("absent")))).wrap());ui.label(theme::small(ui,"Timeout (seconds; cleanup may take longer)"));ui.add(egui::DragValue::new(&mut self.fetch.timeout).range(1..=300));});
        if let Some(choice) = choice {
            let review = self.fetch.review.take().unwrap();
            if choice == confirmations::Choice::Confirm {
                let Some(permit) = branches::permit(ctx) else {
                    self.fetch.notice = Some("Git busy; review again.".into());
                    return;
                };
                let seconds = self.fetch.timeout;
                let (tx, rx) = mpsc::channel();
                let cancel = Arc::new(AtomicBool::new(false));
                self.fetch.applying = Some(workspace::read_only::Request::new(rx, cancel.clone()));
                let ctx = ctx.clone();
                if std::thread::Builder::new()
                    .name("pui-fetch".into())
                    .spawn(move || {
                        let result = workspace::fetch::apply(review, seconds, &cancel);
                        drop(permit);
                        let _ = tx.send(result);
                        ctx.request_repaint();
                    })
                    .is_err()
                {
                    self.fetch.applying = None;
                    self.fetch.notice = Some("Cannot start Fetch.".into());
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
#[cfg(test)]
mod tests;
