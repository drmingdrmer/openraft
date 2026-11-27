//! Prints the scheduling trace of the conformance suite on one `SimRuntime`, then runs
//! `DetsimSuite`.
//!
//! From the repository root, run the example in two separate processes and compare both
//! traces. Use a fresh directory for each comparison because runtime-drop traces are appended.
//!
//! ```sh
//! trace_dir=$(mktemp -d) &&
//!     OPENRAFT_RT_SIM_TRACE="$trace_dir/a-detsim.log" \
//!         cargo run --manifest-path rt-sim/Cargo.toml --example trace_suite > "$trace_dir/a-suite.log" &&
//!     OPENRAFT_RT_SIM_TRACE="$trace_dir/b-detsim.log" \
//!         cargo run --manifest-path rt-sim/Cargo.toml --example trace_suite > "$trace_dir/b-suite.log" &&
//!     diff "$trace_dir/a-suite.log" "$trace_dir/b-suite.log" &&
//!     diff "$trace_dir/a-detsim.log" "$trace_dir/b-detsim.log"
//! ```
//!
//! The logs remain in `$trace_dir` for inspection.

use openraft_rt::testing::DetsimSuite;
use openraft_rt::testing::Suite;
use openraft_rt_sim::SimRuntime;

fn main() {
    let seed = std::env::args().nth(1).map_or(0, |arg| arg.parse().expect("seed must be a u64"));

    let mut rt = SimRuntime::with_seed(seed);
    rt.record_trace(true);
    Suite::<SimRuntime>::test_all_on(&mut rt);
    for event in rt.take_trace() {
        println!("{event}");
    }

    DetsimSuite::<SimRuntime>::test_all();
}
