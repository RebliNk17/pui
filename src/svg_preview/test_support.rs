//! Test-only, thread-scoped helper injection. Never an environment/production
//! executable override. Loader jobs snapshot it so tests exercise real delivery,
//! ticket cancellation, budgeting and supervision without changing other jobs.
use std::cell::RefCell;
thread_local! { static SCRIPT: RefCell<Option<String>> = const { RefCell::new(None) }; }
pub(crate) fn current() -> Option<String> {
    SCRIPT.with_borrow(Clone::clone)
}
pub(crate) fn with<T>(script: Option<String>, run: impl FnOnce() -> T) -> T {
    struct Restore(Option<String>);
    impl Drop for Restore {
        fn drop(&mut self) {
            SCRIPT.with_borrow_mut(|script| *script = self.0.take());
        }
    }
    let prior = SCRIPT.with_borrow_mut(|slot| std::mem::replace(slot, script));
    let _restore = Restore(prior);
    run()
}
