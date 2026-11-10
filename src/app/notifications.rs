//! Completion alerts belong to the Pi owner, never a mirrored history snapshot.
use super::*;

#[derive(Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub(super) struct NotificationSettings {
    pub sound: bool,
    pub desktop: bool,
}
impl Default for NotificationSettings {
    fn default() -> Self {
        Self {
            sound: true,
            desktop: false,
        }
    }
}
#[derive(Default)]
pub(super) struct CompletionRun {
    armed: bool,
    cancelled: bool,
    failed: bool,
}
impl CompletionRun {
    fn observe(&mut self, record: &Value) -> bool {
        match protocol::string(record, "type") {
            "agent_start" => {
                self.armed = true;
                self.cancelled = false;
                self.failed = false;
            }
            "message_end" if record["message"]["role"] == "assistant" => {
                self.failed = matches!(
                    protocol::string(&record["message"], "stopReason"),
                    "error" | "aborted"
                ) || record["message"]["errorMessage"].is_string();
            }
            "agent_settled" => {
                let complete =
                    self.armed && !self.cancelled && !self.failed && record["aborted"] != true;
                *self = Self::default();
                return complete;
            }
            _ => {}
        }
        false
    }
    pub fn cancel(&mut self) {
        self.cancelled = true;
    }
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub(super) struct ChatTarget {
    path: PathBuf,
    cwd: PathBuf,
}
#[derive(Default)]
pub(super) struct Notifications {
    #[cfg(all(target_os = "macos", not(test)))]
    native: Option<macos::Native>,
    #[cfg(test)]
    pub sent: Vec<NotificationSettings>,
    #[cfg(test)]
    targets: Vec<Option<ChatTarget>>,
    #[cfg(test)]
    clicks: Vec<Option<ChatTarget>>,
    pub status: Option<String>,
}
impl Notifications {
    pub fn supported() -> bool {
        cfg!(target_os = "macos")
    }
    pub fn initialize(&mut self, ctx: &egui::Context) {
        #[cfg(all(target_os = "macos", not(test)))]
        {
            if self.native.is_none() {
                match macos::Native::new(ctx) {
                    Ok(native) => self.native = Some(native),
                    Err(error) => self.status = Some(error),
                }
            }
        }
        #[cfg(any(test, not(target_os = "macos")))]
        {
            let _ = ctx;
        }
    }
    pub fn configure(&mut self, ctx: &egui::Context) {
        self.initialize(ctx);
        #[cfg(all(target_os = "macos", not(test)))]
        if let Some(native) = &self.native {
            native.request(None);
        }
    }
    pub fn poll(&mut self) -> Vec<Option<ChatTarget>> {
        #[allow(unused_mut)]
        let mut clicks = Vec::new();
        #[cfg(all(target_os = "macos", not(test)))]
        if let Some(native) = &self.native {
            for event in native.results.try_iter() {
                match event {
                    macos::Event::Status(status) => self.status = Some(status),
                    macos::Event::Open(target) => clicks.push(target),
                }
            }
        }
        #[cfg(test)]
        clicks.append(&mut self.clicks);
        clicks
    }
    pub fn complete(
        &mut self,
        ctx: &egui::Context,
        settings: NotificationSettings,
        name: &str,
        target: Option<ChatTarget>,
    ) {
        if !settings.sound && !settings.desktop {
            return;
        }
        #[cfg(test)]
        {
            let _ = (ctx, name);
            self.sent.push(settings);
            self.targets.push(target.clone());
        }
        #[cfg(all(target_os = "macos", not(test)))]
        {
            if settings.sound {
                macos::sound();
            }
            if settings.desktop {
                if self.native.is_none() {
                    self.configure(ctx);
                }
                if let Some(native) = &self.native {
                    native.request(Some((
                        name.chars().filter(|c| !c.is_control()).take(100).collect(),
                        target,
                    )));
                }
            }
        }
        #[cfg(not(target_os = "macos"))]
        {
            let _ = (ctx, name, target);
            self.status =
                Some("Completion notifications are currently supported on macOS only.".into());
        }
    }
}
impl Pui {
    pub(super) fn poll_notifications(&mut self) {
        for target in self.notifications.poll() {
            self.ctx
                .send_viewport_cmd(egui::ViewportCommand::Visible(true));
            self.ctx
                .send_viewport_cmd(egui::ViewportCommand::Minimized(false));
            self.ctx.send_viewport_cmd(egui::ViewportCommand::Focus);
            let Some(target) = target else {
                continue;
            };
            if self.deletion.in_progress() || self.deletion.deleting_chat(&target.path) {
                continue;
            }
            self.settings_open = false;
            self.kanban.open = false;
            self.automation.open = false;
            self.tasks.open = false;
            self.files.close();
            self.inspector_panel = false;
            self.terminals.close(&self.ctx);
            self.open_chat(Session {
                path: target.path,
                project: target.cwd.clone(),
                cwd: target.cwd,
                title: "Opening chat…".into(),
                created: None,
                modified: std::time::SystemTime::UNIX_EPOCH,
            });
            self.focus_composer = true;
        }
    }
    pub(super) fn propagate_notifications(
        &mut self,
        ctx: &egui::Context,
        before: NotificationSettings,
    ) {
        if before != self.prefs.notifications && !self.demo {
            let remote = self.remote_action(sync::Action::Notifications(self.prefs.notifications));
            if !remote && !before.desktop && self.prefs.notifications.desktop {
                self.notifications.configure(ctx);
            }
        }
    }
    pub(super) fn completion_event(&mut self, record: &Value) {
        let completed = self.completion_run.observe(record);
        if !completed || self.demo || self.is_mirror() || self.client.is_none() {
            return;
        }
        if self.state.session_file.as_ref().is_some_and(|p| {
            self.waiting
                .get(&PathBuf::from(p))
                .is_some_and(|q| !q.is_empty())
        }) {
            return;
        }
        self.notifications.complete(
            &self.ctx,
            self.prefs.notifications,
            &self.state.session_name,
            self.state.session_file.as_ref().map(|path| ChatTarget {
                path: PathBuf::from(path),
                cwd: self.prefs.cwd.clone(),
            }),
        );
    }
}

#[cfg(all(target_os = "macos", not(test)))]
mod macos {
    use super::*;
    use block2::{DynBlock, RcBlock};
    use objc2::{
        AnyThread, DefinedClass, define_class, msg_send,
        rc::Retained,
        runtime::{Bool, ProtocolObject},
    };
    use objc2_foundation::{NSBundle, NSDictionary, NSError, NSObject, NSObjectProtocol, NSString};
    use objc2_user_notifications::{
        UNAuthorizationOptions, UNMutableNotificationContent, UNNotification,
        UNNotificationDefaultActionIdentifier, UNNotificationPresentationOptions,
        UNNotificationRequest, UNNotificationResponse, UNUserNotificationCenter,
        UNUserNotificationCenterDelegate,
    };

    pub(super) enum Event {
        Status(String),
        Open(Option<ChatTarget>),
    }
    struct Ivars {
        tx: mpsc::Sender<Event>,
        ctx: egui::Context,
    }
    define_class!(
        #[unsafe(super(NSObject))]
        #[ivars = Ivars]
        struct CompletionDelegate;
        unsafe impl NSObjectProtocol for CompletionDelegate {}
        unsafe impl UNUserNotificationCenterDelegate for CompletionDelegate {
            #[unsafe(method(userNotificationCenter:willPresentNotification:withCompletionHandler:))]
            fn present(
                &self,
                _center: &UNUserNotificationCenter,
                _notification: &UNNotification,
                completion: &DynBlock<dyn Fn(UNNotificationPresentationOptions)>,
            ) {
                // Foreground banners too. Sound is controlled independently via NSSound.
                completion.call((UNNotificationPresentationOptions::Banner
                    | UNNotificationPresentationOptions::List,));
            }
            #[unsafe(method(userNotificationCenter:didReceiveNotificationResponse:withCompletionHandler:))]
            fn respond(
                &self,
                _center: &UNUserNotificationCenter,
                response: &UNNotificationResponse,
                completion: &DynBlock<dyn Fn()>,
            ) {
                // Dismissing a banner must not navigate or steal focus.
                if &*response.actionIdentifier() == unsafe { UNNotificationDefaultActionIdentifier }
                {
                    let target = response
                        .notification()
                        .request()
                        .content()
                        .userInfo()
                        .objectForKey(&NSString::from_str("pui.chat"))
                        .and_then(|value| {
                            value
                                .downcast_ref::<NSString>()
                                .and_then(|value| serde_json::from_str(&value.to_string()).ok())
                        });
                    let _ = self.ivars().tx.send(Event::Open(target));
                    self.ivars().ctx.request_repaint();
                }
                completion.call(());
            }
        }
    );
    pub(super) struct Native {
        center: Retained<UNUserNotificationCenter>,
        _delegate: Retained<CompletionDelegate>,
        tx: mpsc::Sender<Event>,
        pub results: Receiver<Event>,
        ctx: egui::Context,
    }
    impl Native {
        pub fn new(ctx: &egui::Context) -> Result<Self, String> {
            if NSBundle::mainBundle().bundleIdentifier().is_none() {
                return Err("OS notifications need the bundled Pui.app. Open dist/Pui.app, not the standalone binary.".into());
            }
            let center = UNUserNotificationCenter::currentNotificationCenter();
            let (tx, results) = mpsc::channel();
            let this = CompletionDelegate::alloc().set_ivars(Ivars {
                tx: tx.clone(),
                ctx: ctx.clone(),
            });
            // NSObject has no additional initialization requirements.
            let delegate: Retained<CompletionDelegate> = unsafe { msg_send![super(this), init] };
            center.setDelegate(Some(ProtocolObject::from_ref(&*delegate)));
            Ok(Self {
                center,
                _delegate: delegate,
                tx,
                results,
                ctx: ctx.clone(),
            })
        }
        pub fn request(&self, notification: Option<(String, Option<ChatTarget>)>) {
            let tx = self.tx.clone();
            let ctx = self.ctx.clone();
            let callback = RcBlock::new(move |granted: Bool, error: *mut NSError| {
                if !granted.as_bool() || !error.is_null() {
                    let _ = tx.send(Event::Status("OS notifications are blocked. Enable Pui in System Settings → Notifications. Focus modes may also silence banners.".into()));
                    ctx.request_repaint();
                    return;
                }
                let _ = tx.send(Event::Status("OS notifications allowed. Banner delivery follows macOS notification and Focus settings.".into()));
                ctx.request_repaint();
                if let Some((name, target)) = &notification {
                    let content = UNMutableNotificationContent::new();
                    content.setTitle(&NSString::from_str("Agent finished"));
                    content.setBody(&NSString::from_str(if name.is_empty() {
                        "Your Pui agent has finished its work."
                    } else {
                        name
                    }));
                    if let Some(target) = target {
                        let key = NSString::from_str("pui.chat");
                        let payload = NSString::from_str(&json!(target).to_string());
                        let user_info = NSDictionary::from_slices(&[&*key], &[&*payload]);
                        // Erasing the immutable dictionary's string types is safe;
                        // its values are all property-list-compatible strings.
                        unsafe { content.setUserInfo(user_info.cast_unchecked()) };
                    }
                    let request = UNNotificationRequest::requestWithIdentifier_content_trigger(
                        &NSString::from_str(&uuid::Uuid::new_v4().to_string()),
                        &content,
                        None,
                    );
                    let tx = tx.clone();
                    let ctx = ctx.clone();
                    let delivered = RcBlock::new(move |error: *mut NSError| {
                        if !error.is_null() {
                            let _ = tx.send(Event::Status("macOS could not deliver the notification. Check System Settings → Notifications → Pui.".into()));
                            ctx.request_repaint();
                        }
                    });
                    UNUserNotificationCenter::currentNotificationCenter()
                        .addNotificationRequest_withCompletionHandler(&request, Some(&delivered));
                }
            });
            self.center
                .requestAuthorizationWithOptions_completionHandler(
                    UNAuthorizationOptions::Alert,
                    &callback,
                );
        }
    }
    #[cfg(not(test))]
    pub fn sound() {
        if let Some(sound) = objc2_app_kit::NSSound::soundNamed(&NSString::from_str("Glass")) {
            let _ = sound.play();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn lifecycle_only_alerts_once_after_observed_run_fully_settles() {
        let mut run = CompletionRun::default();
        assert!(!run.observe(&json!({"type":"agent_settled"})));
        for event in [
            json!({"type":"agent_start"}),
            json!({"type":"agent_end"}),
            json!({"type":"compaction_start"}),
            json!({"type":"compaction_end"}),
            json!({"type":"agent_end"}),
        ] {
            assert!(!run.observe(&event));
        }
        assert!(run.observe(&json!({"type":"agent_settled","aborted":false})));
        assert!(!run.observe(&json!({"type":"agent_settled"})));
    }
    #[test]
    fn aborted_stopped_and_failed_runs_do_not_sound_like_success() {
        for kind in ["abort", "stop", "error"] {
            let mut run = CompletionRun::default();
            run.observe(&json!({"type":"agent_start"}));
            if kind == "stop" {
                run.cancel();
            }
            if kind == "error" {
                run.observe(&json!({"type":"message_end","message":{"role":"assistant","stopReason":"error"}}));
            }
            assert!(!run.observe(&json!({"type":"agent_settled","aborted":kind=="abort"})));
        }
    }
    fn app(ctx: &egui::Context) -> Pui {
        let mut app = Pui::new(
            &eframe::CreationContext::_new_kittest(ctx.clone()),
            Options {
                demo: true,
                ..Default::default()
            },
        );
        app.demo = false;
        app.state = State {
            session_file: Some("/fixture/alerts.jsonl".into()),
            session_name: "Completion fixture".into(),
            ..Default::default()
        };
        app.connected = true;
        app.client = Some(rpc::Client::stub());
        app
    }
    #[test]
    fn rpc_completion_notifies_once_and_history_or_process_exit_never_notifies() {
        let ctx = egui::Context::default();
        let mut app = app(&ctx);
        let (client, events) = rpc::Client::stub_with_events();
        app.client = Some(client);
        events.send(rpc::Event::History(vec![])).unwrap();
        events
            .send(rpc::Event::Record(json!({"type":"agent_start"})))
            .unwrap();
        events
            .send(rpc::Event::Record(json!({"type":"agent_end"})))
            .unwrap();
        app.poll_rpc();
        assert!(app.notifications.sent.is_empty());
        for _ in 0..2 {
            events
                .send(rpc::Event::Record(
                    json!({"type":"agent_settled","aborted":false}),
                ))
                .unwrap();
        }
        app.poll_rpc();
        assert_eq!(app.notifications.sent.len(), 1);
        events
            .send(rpc::Event::Record(json!({"type":"agent_start"})))
            .unwrap();
        events.send(rpc::Event::Exited).unwrap();
        app.poll_rpc();
        assert_eq!(app.notifications.sent.len(), 1);
    }
    #[test]
    fn background_chat_keeps_completion_tracking_when_another_chat_is_viewed() {
        let ctx = egui::Context::default();
        let mut app = app(&ctx);
        let (client, events) = rpc::Client::stub_with_events();
        app.client = Some(client);
        events
            .send(rpc::Event::Record(json!({"type":"agent_start"})))
            .unwrap();
        app.poll_rpc();
        let original = app.ensure_agent();
        app.park_tree();
        app.state = State {
            session_file: Some("/fixture/other.jsonl".into()),
            ..Default::default()
        };
        app.ensure_agent();
        events
            .send(rpc::Event::Record(
                json!({"type":"agent_settled","aborted":false}),
            ))
            .unwrap();
        app.poll_other_agents();
        assert_eq!(app.notifications.sent.len(), 1);
        assert_eq!(
            app.state.session_file.as_deref(),
            Some("/fixture/other.jsonl")
        );
        let target = app.notifications.targets[0].clone().unwrap();
        assert_eq!(target.path, PathBuf::from("/fixture/alerts.jsonl"));
        assert_eq!(target.cwd, app.agents.nodes[&original].cwd);
        app.settings_open = true;
        app.kanban.open = true;
        app.automation.open = true;
        app.tasks.open = true;
        app.inspector_panel = true;
        app.notifications.clicks.push(Some(target));
        app.poll_notifications();
        assert_eq!(app.agents.active, original);
        assert_eq!(
            app.state.session_file.as_deref(),
            Some("/fixture/alerts.jsonl")
        );
        assert!(app.client.is_some());
        assert!(app.focus_composer);
        assert!(!app.settings_open);
        assert!(!app.kanban.open);
        assert!(!app.automation.open);
        assert!(!app.tasks.open);
        assert!(!app.inspector_panel);
        assert_eq!(app.notifications.sent.len(), 1);
        app.shutdown_agents();
    }
    #[test]
    fn notification_target_round_trips_paths_with_spaces_and_unicode() {
        let target = ChatTarget {
            path: PathBuf::from("/fixture/שלום chat.jsonl"),
            cwd: PathBuf::from("/workspace/my project"),
        };
        let payload = serde_json::to_string(&target).unwrap();
        assert_eq!(
            serde_json::from_str::<ChatTarget>(&payload).unwrap(),
            target
        );
    }
    #[test]
    fn click_on_current_chat_reveals_it_and_focuses_window() {
        let ctx = egui::Context::default();
        let mut app = app(&ctx);
        app.settings_open = true;
        app.kanban.open = true;
        app.automation.open = true;
        app.tasks.open = true;
        app.inspector_panel = true;
        let _ = ctx.run(egui::RawInput::default(), |_| {});
        let output = ctx.run(egui::RawInput::default(), |_| {
            app.notifications.clicks.push(Some(ChatTarget {
                path: PathBuf::from("/fixture/alerts.jsonl"),
                cwd: app.prefs.cwd.clone(),
            }));
            app.poll_notifications();
        });
        assert_eq!(
            app.state.session_file.as_deref(),
            Some("/fixture/alerts.jsonl")
        );
        assert!(!app.settings_open);
        assert!(!app.kanban.open);
        assert!(!app.automation.open);
        assert!(!app.tasks.open);
        assert!(!app.inspector_panel);
        let commands = &output.viewport_output[&egui::ViewportId::ROOT].commands;
        assert!(
            commands
                .iter()
                .any(|c| matches!(c, egui::ViewportCommand::Visible(true)))
        );
        assert!(
            commands
                .iter()
                .any(|c| matches!(c, egui::ViewportCommand::Minimized(false)))
        );
        assert!(
            commands
                .iter()
                .any(|c| matches!(c, egui::ViewportCommand::Focus))
        );
        assert!(app.notifications.poll().is_empty());
        app.shutdown_agents();
    }
    #[test]
    fn legacy_or_test_alert_without_target_does_not_switch_chats() {
        let ctx = egui::Context::default();
        let mut app = app(&ctx);
        app.settings_open = true;
        app.notifications.clicks.push(None);
        app.poll_notifications();
        assert_eq!(
            app.state.session_file.as_deref(),
            Some("/fixture/alerts.jsonl")
        );
        assert!(app.settings_open);
        assert!(app.notifications.poll().is_empty());
        app.shutdown_agents();
    }
    #[test]
    fn notification_click_reopens_saved_chat_outside_discovery_list() {
        let ctx = egui::Context::default();
        let mut app = app(&ctx);
        let cwd = app.prefs.cwd.clone();
        app.notifications.clicks.push(Some(ChatTarget {
            path: PathBuf::from("/fixture/saved.jsonl"),
            cwd: cwd.clone(),
        }));
        app.poll_notifications();
        assert_eq!(
            app.state.session_file.as_deref(),
            Some("/fixture/saved.jsonl")
        );
        assert_eq!(app.prefs.cwd, cwd);
        assert!(app.client.is_some());
        app.shutdown_agents();
    }
    #[test]
    fn cancelled_queued_and_mirrored_runs_do_not_raise_success_alerts() {
        let ctx = egui::Context::default();
        let mut app = app(&ctx);
        app.completion_event(&json!({"type":"agent_start"}));
        app.send(protocol::command("abort"));
        app.completion_event(&json!({"type":"agent_settled","aborted":false}));
        assert!(app.notifications.sent.is_empty());
        app.waiting.insert(
            PathBuf::from("/fixture/alerts.jsonl"),
            vec![run_status::WaitingMessage {
                text: "next task".into(),
                ..Default::default()
            }],
        );
        app.completion_event(&json!({"type":"agent_start"}));
        app.completion_event(&json!({"type":"agent_settled"}));
        assert!(app.notifications.sent.is_empty());
        app.waiting.clear();
        app.client.take();
        app.completion_event(&json!({"type":"agent_start"}));
        app.completion_event(&json!({"type":"agent_settled"}));
        assert!(app.notifications.sent.is_empty());
    }
    #[test]
    fn successful_retry_can_still_complete_after_an_intermediate_error() {
        let mut run = CompletionRun::default();
        run.observe(&json!({"type":"agent_start"}));
        run.observe(
            &json!({"type":"message_end","message":{"role":"assistant","stopReason":"error"}}),
        );
        assert!(!run.observe(&json!({"type":"agent_end"})));
        run.observe(
            &json!({"type":"message_end","message":{"role":"assistant","stopReason":"stop"}}),
        );
        assert!(run.observe(&json!({"type":"agent_settled"})));
    }
    #[test]
    fn preferences_migrate_and_channels_are_independent() {
        let old: NotificationSettings = serde_json::from_str("{}").unwrap();
        assert!(old.sound);
        assert!(!old.desktop);
        let ctx = egui::Context::default();
        let mut alerts = Notifications::default();
        for (sound, desktop) in [(false, false), (true, false), (false, true), (true, true)] {
            alerts.complete(
                &ctx,
                NotificationSettings { sound, desktop },
                "fixture",
                None,
            );
        }
        assert_eq!(alerts.sent.len(), 3);
        assert!(alerts.sent[0].sound && !alerts.sent[0].desktop);
        assert!(!alerts.sent[1].sound && alerts.sent[1].desktop);
    }
}
