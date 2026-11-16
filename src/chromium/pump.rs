//! One replaceable deadline; unrelated frames and stale native wakes never pump CEF.
use std::{
    sync::Mutex,
    time::{Duration, Instant},
};

// CEF's reference external pump uses 30 Hz as a safety drain. Live tests
// reproduced missing callbacks during load AND after delayed JavaScript.
// Keep this native drain while a CEF browser exists, never repaint egui for it.
// No-page state has no timer. A slower settled cadence needs separate validation.
const DRAIN_INTERVAL: Duration = Duration::from_millis(33);
#[derive(Default)]
struct Deadlines {
    cef: Option<Instant>,
    next_drain: Option<Instant>,
    browsers: std::collections::BTreeSet<u64>,
}
impl Deadlines {
    fn next(&self) -> Option<Instant> {
        self.cef.into_iter().chain(self.next_drain).min()
    }
}
#[derive(Default)]
pub(super) struct Schedule(Mutex<Deadlines>);
impl Schedule {
    pub fn schedule(&self, now: Instant, delay: Duration) {
        self.0.lock().unwrap().cef = now.checked_add(delay);
    }
    pub fn browser(&self, now: Instant, id: u64, open: bool) {
        let mut deadlines = self.0.lock().unwrap();
        if open {
            deadlines.browsers.insert(id);
            if deadlines.next_drain.is_none() {
                deadlines.next_drain = Some(now);
            }
        } else if deadlines.browsers.remove(&id) && deadlines.browsers.is_empty() {
            deadlines.next_drain = None;
        }
    }
    pub fn due(&self, now: Instant) -> bool {
        let mut deadlines = self.0.lock().unwrap();
        if deadlines.browsers.is_empty() {
            deadlines.next_drain = None;
        }
        let due = deadlines.next().is_some_and(|at| at <= now);
        if due {
            if deadlines.cef.is_some_and(|at| at <= now) {
                deadlines.cef = None;
            }
            if !deadlines.browsers.is_empty() {
                deadlines.next_drain = Some(now + DRAIN_INTERVAL);
            }
        }
        due
    }
    pub fn remaining(&self, now: Instant) -> Option<Duration> {
        self.0
            .lock()
            .unwrap()
            .next()
            .map(|at| at.saturating_duration_since(now))
    }
    pub fn clear(&self) {
        *self.0.lock().unwrap() = Deadlines::default();
    }
}

#[cfg(target_os = "macos")]
pub(super) mod native {
    use super::*;
    use dispatch2::{
        DispatchObject, DispatchQueue, DispatchRetained, DispatchSource, DispatchTime,
    };
    use std::sync::Arc;

    pub struct Timer {
        source: DispatchRetained<DispatchSource>,
        schedule: Arc<Schedule>,
        arm: Mutex<()>,
    }
    impl Timer {
        pub fn new(schedule: Arc<Schedule>, fire: impl Fn() + Send + 'static) -> Self {
            // SAFETY: TIMER has no handle/mask; its handler runs only on the main queue.
            let source = unsafe {
                DispatchSource::new(
                    std::ptr::addr_of!(dispatch2::_dispatch_source_type_timer).cast_mut(),
                    0,
                    0,
                    Some(DispatchQueue::main()),
                )
            };
            let block = block2::RcBlock::new(fire);
            // SAFETY: libdispatch copies the block. Captures are owned and Send.
            unsafe {
                source.set_event_handler_with_block(block2::RcBlock::as_ptr(&block));
            }
            source.set_timer(DispatchTime::FOREVER, u64::MAX, 0);
            source.activate();
            Self {
                source,
                schedule,
                arm: Mutex::new(()),
            }
        }
        pub fn schedule(&self, delay: Duration) {
            let _arm = self.arm.lock().unwrap();
            self.schedule.schedule(Instant::now(), delay);
            self.arm_next();
        }
        pub fn rearm(&self) {
            let _arm = self.arm.lock().unwrap();
            self.arm_next();
        }
        fn arm_next(&self) {
            let when = self
                .schedule
                .remaining(Instant::now())
                .and_then(|delay| DispatchTime::try_from(delay).ok())
                .unwrap_or(DispatchTime::FOREVER);
            // One source, including CEF's native safety drain; no egui repaint.
            self.source.set_timer(when, u64::MAX, 0);
        }
        pub fn cancel(&self) {
            self.source.cancel();
        }
    }
    impl Drop for Timer {
        fn drop(&mut self) {
            self.cancel();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn native_drain_stops_after_last_browser_closes() {
        let schedule = Schedule::default();
        let now = Instant::now();
        schedule.browser(now, 999, false);
        assert_eq!(schedule.remaining(now), None);
        schedule.browser(now, 1, true);
        assert!(schedule.due(now));
        assert!(!schedule.due(now + Duration::from_millis(1)));
        assert!(schedule.due(now + DRAIN_INTERVAL));
        schedule.browser(now + DRAIN_INTERVAL, 2, true);
        schedule.browser(now + DRAIN_INTERVAL, 1, false);
        assert!(schedule.remaining(now + DRAIN_INTERVAL).is_some());
        schedule.browser(now + DRAIN_INTERVAL, 2, false);
        assert!(!schedule.due(now + DRAIN_INTERVAL * 2));
        assert_eq!(schedule.remaining(now + DRAIN_INTERVAL * 2), None);
    }
    #[test]
    fn unrelated_frames_and_stale_wakes_do_not_pump() {
        let schedule = Schedule::default();
        let now = Instant::now();
        assert!(!schedule.due(now));
        schedule.schedule(now, Duration::from_millis(20));
        for _ in 0..100 {
            assert!(!schedule.due(now));
        }
        assert!(schedule.due(now + Duration::from_millis(20)));
        assert!(!schedule.due(now + Duration::from_secs(1)));
    }
    #[test]
    fn earlier_and_later_replacements_have_exactly_one_deadline() {
        let schedule = Schedule::default();
        let now = Instant::now();
        schedule.schedule(now, Duration::from_millis(20));
        schedule.schedule(now, Duration::from_secs(1));
        assert!(!schedule.due(now + Duration::from_millis(20)));
        assert_eq!(schedule.remaining(now), Some(Duration::from_secs(1)));
        schedule.schedule(now, Duration::from_millis(5));
        assert!(schedule.due(now + Duration::from_millis(5)));
        assert_eq!(schedule.remaining(now), None);
    }
    #[test]
    fn clear_prevents_pending_work_and_callbacks_can_reschedule_after_consumption() {
        let schedule = Schedule::default();
        let now = Instant::now();
        schedule.schedule(now, Duration::ZERO);
        assert!(schedule.due(now));
        schedule.schedule(now, Duration::ZERO);
        assert!(schedule.due(now));
        schedule.schedule(now, Duration::ZERO);
        schedule.clear();
        assert!(!schedule.due(now));
    }
}
