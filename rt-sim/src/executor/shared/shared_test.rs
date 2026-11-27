use std::future::pending;
use std::panic::catch_unwind;
use std::sync::Arc;
use std::sync::atomic::AtomicBool;
use std::sync::atomic::Ordering;
use std::time::Duration;

use openraft_rt::AsyncRuntime;

use crate::SimInstant;
use crate::SimRuntime;

#[test]
fn nested_runtime_teardown_uses_the_owning_clock_and_drains_new_tasks() {
    let mut owner = SimRuntime::with_seed(1);
    let owner_elapsed = Duration::from_secs(7);
    let cleanup_dropped = Arc::new(AtomicBool::new(false));

    #[cfg(feature = "sim-log")]
    tracing::info!(?owner_elapsed, "Leave a pending task that spawns cleanup on drop");
    {
        owner.block_on(async {
            SimRuntime::sleep(owner_elapsed).await;
            let cleanup_dropped = cleanup_dropped.clone();
            spawn_on_drop(move || {
                assert_eq!(SimInstant::try_now().unwrap().elapsed_since_start(), owner_elapsed);
                spawn_on_drop(move || {
                    assert_eq!(SimInstant::try_now().unwrap().elapsed_since_start(), owner_elapsed);
                    cleanup_dropped.store(true, Ordering::Relaxed);
                });
            });
        });
    }

    SimRuntime::with_seed(2).block_on(async {
        SimRuntime::sleep(Duration::from_secs(3)).await;
        let outer_now = SimInstant::try_now().unwrap();
        #[cfg(feature = "sim-log")]
        tracing::info!(?outer_now, "Drop the owner while the outer runtime is active");
        {
            drop(owner);
            assert!(cleanup_dropped.load(Ordering::Relaxed), "cleanup not drained");
            assert_eq!(SimInstant::try_now(), Some(outer_now));
        }
    });
}

#[test]
fn nested_runtime_teardown_restores_the_outer_clock_on_unwind() {
    let mut owner = SimRuntime::with_seed(1);
    owner.block_on(async {
        let owner_now = SimInstant::try_now().unwrap();
        spawn_on_drop(move || {
            assert_eq!(SimInstant::try_now(), Some(owner_now));
            panic!("task destructor panicked");
        });
    });

    SimRuntime::with_seed(2).block_on(async {
        SimRuntime::sleep(Duration::from_secs(3)).await;
        let outer_now = SimInstant::try_now().unwrap();
        #[cfg(feature = "sim-log")]
        tracing::info!(?outer_now, "Catch a teardown panic without losing the outer clock");
        {
            let err = catch_unwind(|| drop(owner)).unwrap_err();
            assert_eq!(err.downcast_ref::<&str>().copied(), Some("task destructor panicked"));
            assert_eq!(SimInstant::try_now(), Some(outer_now));
        }
    });
}

fn spawn_on_drop<F>(on_drop: F)
where F: FnOnce() + Send + 'static {
    struct OnDrop<F>(Option<F>)
    where F: FnOnce();

    impl<F> Drop for OnDrop<F>
    where F: FnOnce()
    {
        fn drop(&mut self) {
            self.0.take().unwrap()();
        }
    }

    let guard = OnDrop(Some(on_drop));
    let _handle = SimRuntime::spawn(async move {
        let _guard = guard;
        pending::<()>().await;
    });
}
