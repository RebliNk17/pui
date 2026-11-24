//! Reviewed browser handoffs freeze the existing chat draft only on explicit activation.
#[cfg(test)]
mod tests;
use super::*;
use std::sync::Arc;
const SNAPSHOT_ID: &str = "browser-draft-owner";
#[derive(Clone)]
struct Snapshot {
    root: PathBuf,
    session: String,
    draft: String,
    images: u64,
    token: u64,
    pass: u64,
}
#[derive(Clone)]
struct Control {
    id: egui::Id,
    pass: u64,
    root: PathBuf,
    session: String,
}
#[derive(Default, Clone)]
struct State {
    snapshot: Option<Arc<Snapshot>>,
    notice: Option<String>,
}
fn id() -> egui::Id {
    egui::Id::new(SNAPSHOT_ID)
}
fn take(ctx: &egui::Context) -> State {
    ctx.data_mut(|d| d.remove_temp::<State>(id()).unwrap_or_default())
}
fn put(ctx: &egui::Context, s: State) {
    ctx.data_mut(|d| d.insert_temp(id(), s));
}
/// Freeze the exact current draft for an explicit browser action. No per-frame hash/copy.
pub(in crate::app) fn freeze(ctx: &egui::Context) -> Option<u64> {
    let mut s = take(ctx);
    let live = ctx.data(|d| d.get_temp::<Arc<Snapshot>>(id().with("live")));
    let Some(live) = live
        .filter(|v| v.pass == ctx.cumulative_pass_nr() && !prompt_ui::modal_owns_current_pass(ctx))
    else {
        put(ctx, s);
        return None;
    };
    static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);
    let token = NEXT
        .fetch_update(
            std::sync::atomic::Ordering::Relaxed,
            std::sync::atomic::Ordering::Relaxed,
            |n| n.checked_add(1),
        )
        .ok()?;
    let mut owned = (*live).clone();
    owned.token = token;
    s.snapshot = Some(Arc::new(owned));
    s.notice = None;
    put(ctx, s);
    Some(token)
}
/// Opening a browser's explicit handoff must happen before later UI can mutate
/// its draft. This event-bound live snapshot is offered only for previous-live
/// enabled registered handoff controls, never on every animation frame.
pub(super) fn prepare(app: &mut Pui, ctx: &egui::Context) {
    ctx.data_mut(|d| d.remove::<Arc<Snapshot>>(id().with("live")));
    let Some(controls) = ctx.data(|d| d.get_temp::<Vec<Control>>(id().with("controls"))) else {
        return;
    };
    let pass = ctx.cumulative_pass_nr();
    let pointer = ctx.input(|i| {
        i.pointer
            .primary_released()
            .then(|| i.pointer.interact_pos())
            .flatten()
    });
    let keyboard=ctx.input(|i|i.events.iter().any(|e|matches!(e,egui::Event::Key{key:egui::Key::Enter|egui::Key::Space,pressed:true,modifiers,..}if *modifiers==egui::Modifiers::NONE)));
    let owner = app.review_comment_owner();
    let active = controls.iter().any(|c| {
        c.root == app.prefs.cwd
            && c.session == owner
            && app.files.is_open()
            && c.pass.checked_add(1) == Some(pass)
            && ctx.read_response(c.id).is_some_and(|r| {
                r.enabled()
                    && (r.has_focus() && keyboard
                        || pointer.is_some_and(|p| {
                            r.interact_rect.contains(p) && ctx.layer_id_at(p) == Some(r.layer_id)
                        }))
            })
    });
    if !active
        || app.draft.len() > 256 * 1024
        || ctx.memory(|m| m.top_modal_layer().is_some())
        || prompt_ui::modal_owns_current_pass(ctx)
    {
        return;
    }
    let snapshot = Snapshot {
        root: app.prefs.cwd.clone(),
        session: app.review_comment_owner(),
        draft: app.draft.clone(),
        images: app.draft_images.revision(),
        token: 0,
        pass,
    };
    ctx.data_mut(|d| d.insert_temp(id().with("live"), Arc::new(snapshot)));
}
pub(in crate::app) fn register(response: &egui::Response) {
    let pass = response.ctx.cumulative_pass_nr();
    let Some((root, session, bound)) = response
        .ctx
        .data(|d| d.get_temp::<(PathBuf, String, u64)>(egui::Id::new("pui-file-capture-owner")))
    else {
        return;
    };
    if bound != pass {
        return;
    }
    response.ctx.data_mut(|d| {
        let mut list = d
            .get_temp::<Vec<Control>>(id().with("controls"))
            .unwrap_or_default();
        list.retain(|c| c.pass == pass);
        if list.len() < 8 && !list.iter().any(|c| c.id == response.id) {
            list.push(Control {
                id: response.id,
                pass,
                root,
                session,
            });
        }
        d.insert_temp(id().with("controls"), list);
    });
}
fn owner_matches(app: &mut Pui, s: &Snapshot) -> bool {
    s.root == app.prefs.cwd
        && s.session == app.review_comment_owner()
        && s.draft == app.draft
        && s.images == app.draft_images.revision()
}
/// Detect draft changes only while a review exists; no draft clone/hash here.
pub(in crate::app) fn invalidate_changed(app: &mut Pui) {
    let ctx = app.ctx.clone();
    let mut s = take(&ctx);
    if s.snapshot.as_ref().is_some_and(|v| !owner_matches(app, v)) {
        s.snapshot = None;
        s.notice = Some("Browser review cancelled: chat, draft or attachments changed.".into());
    }
    put(&ctx, s);
}
/// Release only this review; dropping an older utility must not cancel its successor.
pub(in crate::app) fn cancel(ctx: &egui::Context, token: u64) {
    let mut s = take(ctx);
    if s.snapshot.as_ref().is_some_and(|v| v.token == token) {
        s.snapshot = None;
    }
    put(ctx, s);
}
pub(in crate::app) fn token(ctx: &egui::Context) -> Option<u64> {
    ctx.data(|d| {
        d.get_temp::<State>(id())
            .and_then(|s| s.snapshot.map(|s| s.token))
    })
}
pub(in crate::app) fn failed(ctx: &egui::Context, token: u64, error: String) {
    cancel(ctx, token);
    let mut s = take(ctx);
    s.notice = Some(error);
    put(ctx, s);
}
pub(in crate::app) fn notice(ctx: &egui::Context) -> Option<String> {
    ctx.data(|d| d.get_temp::<State>(id()).and_then(|s| s.notice))
}
pub(in crate::app) fn clear(ctx: &egui::Context) {
    ctx.data_mut(|d| {
        d.remove::<State>(id());
        d.remove::<Arc<Snapshot>>(id().with("live"));
        d.remove::<Vec<Control>>(id().with("controls"));
        d.remove::<Handoff>(id().with("result"));
    });
}
#[derive(Clone)]
pub(in crate::app) enum Handoff {
    Text {
        token: u64,
        page: String,
        epoch: u64,
        label: String,
        body: String,
    },
    Image {
        token: u64,
        page: String,
        epoch: u64,
        image: crate::media::Attachment,
    },
}
pub(in crate::app) fn queue(ctx: &egui::Context, event: Handoff) {
    ctx.data_mut(|d| d.insert_temp(id().with("result"), event));
}
pub(super) fn collect(app: &mut Pui) {
    let ctx = app.ctx.clone();
    let Some(event) = ctx.data_mut(|d| {
        let v = d.get_temp::<Handoff>(id().with("result"));
        d.remove::<Handoff>(id().with("result"));
        v
    }) else {
        return;
    };
    let mut s = take(&ctx);
    let (token, page, epoch) = match &event {
        Handoff::Text {
            token, page, epoch, ..
        }
        | Handoff::Image {
            token, page, epoch, ..
        } => (*token, page.as_str(), *epoch),
    };
    if s.snapshot.as_ref().is_some_and(|snap| snap.token != token) {
        put(&ctx, s);
        return;
    }
    let snapshot = s.snapshot.take();
    let valid = snapshot
        .as_ref()
        .is_some_and(|snap| snap.token == token && owner_matches(app, snap))
        && app.files.browser_capture_matches(page, epoch);
    if !valid {
        s.notice = Some(
            "Browser context not added: chat, draft or attachments changed. Review again.".into(),
        );
        put(&ctx, s);
        return;
    }
    let result = match event {
        Handoff::Text { label, body, .. } => {
            if body.is_empty() || body.len() > MAX_BYTES || label.len() > 512 {
                Err("Browser context exceeds the whole-excerpt bound.".into())
            } else {
                app.context_chips
                    .append(
                        &mut app.draft,
                        Snippet {
                            label,
                            text: body,
                            ask: None,
                        },
                    )
                    .map_err(str::to_owned)
            }
        }
        Handoff::Image { page, image, .. } => app
            .draft_images
            .append_reviewed(format!("Browser screenshot · {page}"), image),
    };
    match result {
        Ok(()) => {
            s.notice = Some("Added to draft only. Review before sending.".into());
            app.focus_composer = true;
        }
        Err(e) => s.notice = Some(e),
    }
    put(&ctx, s);
    ctx.request_repaint();
}
