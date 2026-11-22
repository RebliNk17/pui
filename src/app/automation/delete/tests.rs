use super::*;
use crate::app::automation::tests::{app, create};

#[test]
fn orphan_delete_is_durable_cancels_only_its_check_and_preserves_other_jobs_and_chats() {
    let ctx = egui::Context::default();
    let mut app = app(&ctx);
    let id = create(&mut app, "monitor");
    let keep = create(&mut app, "cron");
    let home = std::env::temp_dir().join(format!("pui-orphan-delete-{}", uuid::Uuid::new_v4()));
    app.automation.lock = Some(store::lock(&home).unwrap());
    app.automation.home = Some(home.clone());
    app.automation.persist().unwrap();
    app.agents.nodes.clear();
    let canceled = Arc::new(AtomicBool::new(false));
    let (_, rx) = mpsc::channel();
    app.automation.running.insert(
        id.clone(),
        Running {
            cancel: canceled.clone(),
            rx,
            provenance: None,
        },
    );
    app.automation
        .mail
        .insert("delivery".into(), (id.clone(), std::time::Instant::now()));
    let messages = app.state.messages.len();
    app.draft = "preserve draft".into();
    app.request_scheduled_job_delete(&id).unwrap();
    assert!(canceled.load(Ordering::Acquire));
    assert!(app.automation.mail.is_empty());
    assert_eq!(app.automation.jobs.len(), 1);
    assert_eq!(store::load(&home).unwrap()[0].id, keep);
    assert_eq!(app.state.messages.len(), messages);
    assert_eq!(app.draft, "preserve draft");
    assert!(app.agents.nodes.is_empty());
    app.delete_scheduled_job(&id).unwrap(); // already deleted is an idempotent success
    assert!(app.delete_scheduled_job("../jobs").is_err());
}

#[test]
fn delete_write_failure_keeps_live_and_durable_job_visible() {
    let ctx = egui::Context::default();
    let mut app = app(&ctx);
    let id = create(&mut app, "monitor");
    let home = std::env::temp_dir().join(format!("pui-delete-failure-{}", uuid::Uuid::new_v4()));
    app.automation.lock = Some(store::lock(&home).unwrap());
    app.automation.home = Some(home.clone());
    app.automation.persist().unwrap();
    std::fs::create_dir(home.join("jobs.tmp")).unwrap();
    assert!(app.request_scheduled_job_delete(&id).is_err());
    assert_eq!(app.automation.jobs[0].id, id);
    assert_eq!(store::load(&home).unwrap()[0].id, id);
}

#[test]
fn non_owner_must_route_and_demo_failure_and_wrong_store_cannot_delete() {
    let ctx = egui::Context::default();
    let mut app = app(&ctx);
    let id = create(&mut app, "cron");
    let home = std::env::temp_dir().join(format!("pui-delete-guard-{}", uuid::Uuid::new_v4()));
    let lock = store::lock(&home).unwrap();
    app.automation.home = Some(home.clone());
    assert!(app.request_scheduled_job_delete(&id).is_err());
    assert!(app.delete_remote_scheduled_job(&home, &id).is_err());
    app.automation.lock = Some(lock);
    assert!(
        app.delete_remote_scheduled_job(&home.join("wrong"), &id)
            .is_err()
    );
    app.demo = true;
    assert!(app.request_scheduled_job_delete(&id).is_err());
    assert!(app.delete_remote_scheduled_job(&home, &id).is_err());
    app.demo = false;
    app.automation.failed = true;
    assert!(app.request_scheduled_job_delete(&id).is_err());
    assert_eq!(app.automation.jobs.len(), 1);
}

#[test]
fn cross_window_orphan_delete_uses_real_transport_without_creator_and_handles_owner_exit() {
    let ctx = egui::Context::default();
    let mut owner = app(&ctx);
    let id = create(&mut owner, "cron");
    let keep = create(&mut owner, "cron");
    let home = std::env::temp_dir().join(format!("pui-cross-delete-{}", uuid::Uuid::new_v4()));
    owner.automation.lock = Some(store::lock(&home).unwrap());
    owner.automation.home = Some(home.clone());
    owner.automation.persist().unwrap();
    owner.agents.nodes.clear();
    owner.state.session_file = None;
    owner.client = None;
    owner.sync = sync::WindowSync::new_at(&ctx, false, Some(home.join("registry")));
    owner.poll_automation(); // advertise scheduler even with zero live chats
    let mut observer = app(&ctx);
    observer.agents.nodes.clear();
    observer.state.session_file = None;
    observer.client = None;
    observer.automation.home = Some(home.clone());
    observer.automation.refresh();
    assert!(observer.automation.lock.is_none());
    observer.sync = sync::WindowSync::new_at(&ctx, false, Some(home.join("registry")));
    let wait = |owner: &mut Pui, observer: &mut Pui, done: &dyn Fn(&Pui, &Pui) -> bool| {
        let deadline = std::time::Instant::now() + Duration::from_secs(8);
        while !done(owner, observer) {
            assert!(
                std::time::Instant::now() < deadline,
                "cross-window delete timeout: {:?}",
                observer.automation.error
            );
            owner.poll_window_sync();
            observer.poll_window_sync();
            std::thread::sleep(Duration::from_millis(5));
        }
    };
    // Wait for real registry publication before requesting; no scheduler lock transfer.
    let deadline = std::time::Instant::now() + Duration::from_secs(3);
    loop {
        let advertised = std::fs::read_dir(home.join("registry"))
            .unwrap()
            .flatten()
            .any(|entry| {
                entry.path().extension().is_some_and(|ext| ext == "json")
                    && std::fs::read(entry.path())
                        .ok()
                        .and_then(|bytes| serde_json::from_slice::<Value>(&bytes).ok())
                        .is_some_and(|data| data["automation_home"] == json!(home))
            });
        if advertised {
            break;
        }
        assert!(std::time::Instant::now() < deadline);
        std::thread::sleep(Duration::from_millis(5));
    }
    observer.request_scheduled_job_delete(&id).unwrap();
    assert!(observer.automation.deleting.is_some());
    assert!(observer.request_scheduled_job_delete(&keep).is_err());
    wait(&mut owner, &mut observer, &|a, b| {
        a.automation.jobs.len() == 1 && b.automation.deleting.is_none()
    });
    assert_eq!(owner.automation.jobs[0].id, keep);
    assert_eq!(observer.automation.jobs[0].id, keep);
    assert!(observer.automation.lock.is_none());
    assert!(observer.automation.error.is_none());
    assert!(owner.agents.nodes.is_empty() && observer.agents.nodes.is_empty());
    assert!(owner.state.session_file.is_none() && observer.state.session_file.is_none());
    // Unpublished/older owner is a surfaced failure, never direct non-owner writes.
    owner.sync = sync::WindowSync::new(&ctx, true);
    observer.request_scheduled_job_delete(&keep).unwrap();
    wait(&mut owner, &mut observer, &|_, b| {
        b.automation.deleting.is_none()
    });
    assert!(
        observer
            .automation
            .error
            .as_deref()
            .unwrap()
            .contains("not acknowledged")
    );
    assert_eq!(store::load(&home).unwrap().len(), 1);
    // Closing the owner releases the OS lock; ordinary takeover permits local deletion.
    owner.automation.lock = None;
    observer.automation.refresh();
    assert!(observer.automation.writable());
    observer.request_scheduled_job_delete(&keep).unwrap();
    assert!(store::load(&home).unwrap().is_empty());
}
