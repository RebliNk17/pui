use super::*;

fn fixture() -> PathBuf {
    let directory =
        std::env::temp_dir().join(format!("pui-dock-presence-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&directory).unwrap();
    directory
}

#[test]
fn dock_eligibility_requires_successful_scan_and_counts_live_empty_or_malformed_peers() {
    let directory = fixture();
    let sync = WindowSync::new(&egui::Context::default(), true);
    assert!(!sync.dock_single_window());
    assert!(registry_with_presence(&directory, "own").1);
    let owner = private_open(&directory.join("peer.owner")).unwrap();
    owner.try_lock_exclusive().unwrap();
    assert!(
        !registry_with_presence(&directory, "own").1,
        "A starting peer lease without metadata is still live"
    );
    let peer = Endpoint {
        id: "peer".into(),
        port: 1,
        token: "fixture-only".into(),
        automation_home: None,
        paths: vec![],
    };
    write_endpoint(&directory, &peer).unwrap();
    let (endpoints, single) = registry_with_presence(&directory, "own");
    assert!(
        !single,
        "A pathless window still suppresses a local Dock total"
    );
    assert_eq!(endpoints.len(), 1);
    assert!(endpoints[0].paths.is_empty());
    std::fs::write(directory.join("peer.json"), b"invalid fixture metadata").unwrap();
    let (endpoints, single) = registry_with_presence(&directory, "own");
    assert!(endpoints.is_empty());
    assert!(!single, "Malformed live peer must fail closed");
    drop(owner);
    assert!(
        registry_with_presence(&directory, "own").1,
        "Unlocked stale peers are not live"
    );
    assert!(
        !registry_with_presence(&directory.join("missing"), "own").1,
        "Scan errors are not known-single"
    );
    std::fs::write(directory.join("unavailable.json"), b"{}").unwrap();
    assert!(
        !registry_with_presence(&directory, "own").1,
        "Missing owner cannot prove absence"
    );
}

fn wait_single(sync: &mut WindowSync, expected: bool) {
    let deadline = std::time::Instant::now() + Duration::from_secs(5);
    loop {
        let remaining = deadline.saturating_duration_since(std::time::Instant::now());
        match sync
            .rx
            .recv_timeout(remaining)
            .expect("Presence watcher did not report expected transition")
        {
            ResultEvent::Registry(_, single, epoch)
                if single == expected && epoch == sync.presence_epoch.load(Ordering::Acquire) =>
            {
                sync.dock_single_window = single;
                sync.dock_epoch_accepted = Some(epoch);
                return;
            }
            _ => {}
        }
    }
}

#[test]
fn empty_windows_publish_presence_and_event_driven_scan_recovers_on_close() {
    let directory = fixture();
    let ctx = egui::Context::default();
    let mut first = WindowSync::new_at(&ctx, false, Some(directory.clone()));
    assert!(first.error.is_none());
    assert!(
        !first.dock_single_window(),
        "Initialization has not observed scan results"
    );
    assert!(directory.join(format!("{}.json", first.id)).exists());
    first._watcher.as_ref().unwrap().wait_ready().unwrap();
    first.dock_watch_ready = true;
    first.tx.send(Work::Scan).unwrap();
    wait_single(&mut first, true);
    assert!(first.dock_single_window());
    let second = WindowSync::new_at(&ctx, false, Some(directory.clone()));
    assert!(second.error.is_none());
    assert!(
        registry_with_presence(&directory, &first.id)
            .0
            .iter()
            .any(|peer| peer.id == second.id && peer.paths.is_empty())
    );
    assert!(!registry_with_presence(&directory, &first.id).1);
    wait_single(&mut first, false);
    assert!(!first.dock_single_window());
    drop(second);
    assert!(registry_with_presence(&directory, &first.id).1);
    wait_single(&mut first, true);
    assert!(first.dock_single_window());
    first.dock_watch_ready = false;
    assert!(
        !first.dock_single_window(),
        "Scan alone is not a ready watcher"
    );
    first.dock_watch_ready = true;
    first.error = Some("fixture watcher failure".into());
    assert!(!first.dock_single_window());
}
