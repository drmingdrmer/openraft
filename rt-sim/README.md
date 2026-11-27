# openraft-rt-sim

A deterministic, simulated [`AsyncRuntime`][rt_link] for testing Openraft.

- One thread, FIFO scheduling: tasks are polled in the order they became runnable.
- Virtual time: when nothing is runnable, the clock jumps to the earliest pending timer.
  Timers with equal deadlines fire in creation order.
- Seeded `thread_rng()`: every call draws from a stream derived from the runtime seed.
- If nothing is runnable and no timer is pending, `block_on` panics instead of hanging.

With the `sim-log` feature and `OPENRAFT_SIM_LOG=<file>`, `block_on` writes the tracing events of
its thread to that file, stamped with virtual time. All runtimes in a process share the file, so
concurrent tests can interleave events differently even with the same seed. For byte-for-byte
comparisons, keep the seed and log settings fixed and run tests serially (`--test-threads=1`). To
run them in parallel, run each test in its own process with a distinct `OPENRAFT_SIM_LOG` path.

For example, CI compares two serial runs of the membership tests with the same seed and network
delay. From the repository root:

```sh
log_dir=$(mktemp -d)
export OPENRAFT_RT_SIM_SEED=9 OPENRAFT_NETWORK_SEND_DELAY=30
for run in a b; do
    OPENRAFT_SIM_LOG="$log_dir/$run.log" cargo test --manifest-path tests/Cargo.toml --features rt-sim \
        --test membership -- --test-threads=1 t24_append_membership::
done
test -s "$log_dir/a.log" && diff "$log_dir/a.log" "$log_dir/b.log"
```

See [openraft#206](https://github.com/databendlabs/openraft/issues/206).

[rt_link]: https://docs.rs/openraft/latest/openraft/async_runtime/trait.AsyncRuntime.html
