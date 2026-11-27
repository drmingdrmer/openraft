//! The guard that installs a runtime on the current thread.

use std::sync::Arc;

use crate::executor::CURRENT;
use crate::executor::Shared;

/// Installs a runtime on this thread and restores the previous slot on drop, even during a panic.
pub(super) struct Enter {
    previous: Option<Arc<Shared>>,
}

impl Enter {
    pub(super) fn new(shared: Arc<Shared>) -> Self {
        Self::try_new(shared).expect("rt-sim: nested block_on is not supported")
    }

    /// Installs `shared` unless a runtime is already installed on this thread.
    fn try_new(shared: Arc<Shared>) -> Option<Self> {
        CURRENT
            .try_with(|c| {
                let mut c = c.borrow_mut();
                if c.is_some() {
                    return None;
                }
                *c = Some(shared);
                Some(Enter { previous: None })
            })
            .ok()
            .flatten()
    }

    /// Temporarily installs the task owner for teardown, even inside another runtime.
    /// Returns `None` if the thread-local has already been destroyed.
    pub(super) fn for_teardown(shared: Arc<Shared>) -> Option<Self> {
        CURRENT
            .try_with(|c| Enter {
                previous: c.replace(Some(shared)),
            })
            .ok()
    }
}

impl Drop for Enter {
    fn drop(&mut self) {
        let _ = CURRENT.try_with(|c| c.replace(self.previous.take()));
    }
}

#[cfg(test)]
mod tests {
    use std::panic::AssertUnwindSafe;
    use std::panic::catch_unwind;

    use openraft_rt::AsyncRuntime;

    use crate::SimInstant;
    use crate::SimRuntime;

    #[test]
    fn dropping_another_runtime_preserves_the_outer_clock() {
        SimRuntime::with_seed(1).block_on(async {
            let now = SimInstant::try_now().unwrap();

            drop(SimRuntime::with_seed(2));

            assert_eq!(SimInstant::try_now(), Some(now));
        });
    }

    #[test]
    fn rejected_nested_block_on_preserves_the_outer_clock() {
        SimRuntime::with_seed(1).block_on(async {
            let now = SimInstant::try_now().unwrap();
            let mut inner = SimRuntime::with_seed(2);

            let result = catch_unwind(AssertUnwindSafe(|| inner.block_on(async {})));

            assert!(result.is_err(), "nested block_on must panic");
            assert_eq!(SimInstant::try_now(), Some(now));
        });
    }
}
