use super::*;
#[test]
fn observed_presence_epoch_invalidates_immediately_and_old_scan_cannot_authorize() {
    let ctx = egui::Context::default();
    let mut sync = WindowSync::new(&ctx, true);
    sync.dock_single_window = true;
    sync.dock_watch_ready = true;
    sync.dock_epoch_accepted = Some(1);
    assert!(sync.dock_single_window());
    sync.presence_epoch.fetch_add(1, Ordering::AcqRel);
    assert!(
        !sync.dock_single_window(),
        "no waiting for a queued scan or peer action"
    );
    sync.dock_epoch_accepted = Some(1);
    assert!(
        !sync.dock_single_window(),
        "stale successful scan cannot reauthorize"
    );
    sync.dock_epoch_accepted = Some(2);
    assert!(sync.dock_single_window());
    sync.dock_watch_ready = false;
    assert!(!sync.dock_single_window());
}
#[test]
fn dropped_event_rescan_is_presence_relevant_without_paths_but_own_metadata_is_not() {
    let own = Path::new("/fixture/own.json");
    let rescan = notify::Event::new(notify::EventKind::Other).set_flag(notify::event::Flag::Rescan);
    assert!(registry_changed(&rescan, own));
    let own_event = notify::Event::new(notify::EventKind::Modify(notify::event::ModifyKind::Any))
        .add_path(own.into());
    assert!(!registry_changed(&own_event, own));
    let peer = notify::Event::new(notify::EventKind::Create(notify::event::CreateKind::Any))
        .add_path("/fixture/peer.owner".into());
    assert!(registry_changed(&peer, own));
}
