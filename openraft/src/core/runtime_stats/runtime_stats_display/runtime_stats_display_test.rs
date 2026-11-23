use base2histogram::Histogram;
use base2histogram::PercentileStats;

use super::*;
use crate::engine::testing::UTConfig;

type C = UTConfig<()>;

const EMPTY_STATS: PercentileStats = PercentileStats {
    samples: 0,
    p0_1: 0,
    p1: 0,
    p5: 0,
    p10: 0,
    p50: 0,
    p90: 0,
    p99: 0,
    p99_9: 0,
};

fn sample_stats(samples: u64, p50: u64, p99: u64) -> PercentileStats {
    PercentileStats {
        samples,
        p0_1: 1,
        p1: 2,
        p5: 5,
        p10: 10,
        p50,
        p90: p99 - 1,
        p99,
        p99_9: p99 + 1,
    }
}

fn make_display(
    apply_batch: PercentileStats,
    command_counts: Vec<u64>,
    log_stage: [(&'static str, PercentileStats); 6],
) -> RuntimeStatsDisplay<C> {
    RuntimeStatsDisplay {
        mode: DisplayMode::default(),
        core_busy_duration: Duration::ZERO,
        core_busy_histogram: EMPTY_STATS,
        apply_batch,
        append_batch: EMPTY_STATS,
        replicate_batch: EMPTY_STATS,
        raft_msg_per_run: EMPTY_STATS,
        write_batch: EMPTY_STATS,
        raft_msg_budget: EMPTY_STATS,
        notification_budget: EMPTY_STATS,
        raft_msg_usage_permille: EMPTY_STATS,
        notification_usage_permille: EMPTY_STATS,
        command_counts,
        raft_msg_counts: vec![0; RaftMsgName::COUNT],
        notification_counts: vec![0; NotificationName::COUNT],
        log_stage_percentiles: log_stage,
        log_stage_histograms: LogStageHistograms::new(),
        log_stages: LogStages::new(1, 0),
    }
}

fn empty_log_stages() -> [(&'static str, PercentileStats); 6] {
    [
        ("1:proposed→received", EMPTY_STATS),
        ("2:received→submitted", EMPTY_STATS),
        ("3:submitted→persisted", EMPTY_STATS),
        ("4:persisted→committed", EMPTY_STATS),
        ("5:committed→applied", EMPTY_STATS),
        ("1~5:proposed→applied", EMPTY_STATS),
    ]
}

#[test]
fn test_compact_empty() {
    let d = make_display(EMPTY_STATS, vec![0; CommandName::COUNT], empty_log_stages());
    let s = format!("{}", d.compact());

    assert!(s.starts_with("RuntimeStats {"));
    assert!(s.ends_with("} }"));
    assert!(s.contains("commands: {}"));
    assert!(s.contains("log_stages(us): {}"));
}

#[test]
fn test_compact_with_data() {
    let mut cmds = vec![0u64; CommandName::COUNT];
    cmds[0] = 42;

    let mut stages = empty_log_stages();
    stages[0].1 = sample_stats(10, 100, 500);

    let d = make_display(sample_stats(5, 50, 200), cmds, stages);
    let s = format!("{}", d.compact());
    println!("{s}");

    assert!(s.contains("apply_batch: [samples: 5"));
    assert!(s.contains("1:proposed→received: [samples: 10"));
}

#[test]
fn test_core_busy_histogram_distribution_display() {
    let mut histogram = Histogram::<()>::new();
    for micros in 1..=7 {
        histogram.record(micros);
    }
    let stats = histogram.percentile_stats();
    let expected = PercentileStats {
        samples: 7,
        p0_1: 1,
        p1: 1,
        p5: 1,
        p10: 1,
        p50: 4,
        p90: 7,
        p99: 7,
        p99_9: 7,
    };
    assert_eq!(expected, stats);

    let rendered = format!("{stats}");
    assert_eq!(
        "[samples: 7, P0.1: 1, P1: 1, P5: 1, P10: 1, P50: 4, P90: 7, P99: 7, P99.9: 7]",
        rendered,
    );

    let duration = Duration::from_micros(28);
    let duration_text = format!("{duration:?}");
    let duration_field = format!("core_busy_duration: {duration_text}");
    let histogram_field = format!("core_busy_histogram(us): {rendered}");

    let mut display = make_display(EMPTY_STATS, vec![0; CommandName::COUNT], empty_log_stages());
    display.core_busy_duration = duration;
    display.core_busy_histogram = stats;
    let compact = format!("{}", display.compact());
    let compact_head = format!("RuntimeStats {{ {duration_field}, {histogram_field}");
    assert!(compact.starts_with(&compact_head));

    let mut display = make_display(EMPTY_STATS, vec![0; CommandName::COUNT], empty_log_stages());
    display.core_busy_duration = duration;
    display.core_busy_histogram = stats;
    let multiline = format!("{}", display.multiline());
    let multiline_head = format!("RuntimeStats:\n  {duration_field}\n  {histogram_field}\n");
    assert!(multiline.starts_with(&multiline_head));

    #[cfg(feature = "runtime-stats")]
    {
        let mut display = make_display(EMPTY_STATS, vec![0; CommandName::COUNT], empty_log_stages());
        display.core_busy_duration = duration;
        display.core_busy_histogram = stats;
        let human = format!("{}", display.human_readable());
        let table = [
            "╭──────┬──────────┬──────┬────┬────┬─────┬─────┬─────┬─────┬───────┬─────────────────────────────────╮",
            "│      │ #Samples │ P0.1 │ P1 │ P5 │ P10 │ P50 │ P90 │ P99 │ P99.9 │                                 │",
            "├──────┼──────────┼──────┼────┼────┼─────┼─────┼─────┼─────┼───────┼─────────────────────────────────┤",
            "│ Busy │        7 │    1 │  1 │  1 │   1 │   4 │   7 │   7 │     7 │ Wall time of one core-loop turn │",
            "╰──────┴──────────┴──────┴────┴────┴─────┴─────┴─────┴─────┴───────┴─────────────────────────────────╯",
        ]
        .join("\n");
        let busy = format!("Core busy duration: {duration_text}\n\nCore loop busy (us):\n{table}\n");
        assert!(human.starts_with(&busy));
    }
}

#[test]
fn test_multiline_empty() {
    let d = make_display(EMPTY_STATS, vec![0; CommandName::COUNT], empty_log_stages());
    let s = format!("{}", d.multiline());

    assert!(s.contains("RuntimeStats:"));
    assert!(s.contains("apply_batch:"));
    assert!(s.contains("log_stages(us):"));
    // No log stage entries when all empty
    assert!(!s.contains("proposed→received"));
}

#[test]
fn test_multiline_with_log_stages() {
    let mut stages = empty_log_stages();
    stages[0].1 = sample_stats(100, 50, 200);
    stages[5].1 = sample_stats(100, 120, 800);

    let d = make_display(EMPTY_STATS, vec![0; CommandName::COUNT], stages);
    let s = format!("{}", d.multiline());

    println!("{s}");

    assert!(s.contains("1:proposed→received"));
    assert!(s.contains("n=100"));
    assert!(s.contains("p50=50"));
    assert!(s.contains("p99=200"));
    assert!(s.contains("1~5:proposed→applied"));
    assert!(s.contains("p50=120"));
    // Stages with 0 samples are omitted
    assert!(!s.contains("2:received→submitted"));
}

#[test]
fn test_multiline_with_log_stage_segments() {
    use std::time::Duration;

    use crate::type_config::TypeConfigExt;

    // Build 3 batches of log entries with realistic stage timestamps:
    //   batch 1: log [0,10), proposed at t=0ms
    //   batch 2: log [10,20), proposed at t=5ms
    //   batch 3: log [20,30), proposed at t=12ms
    let base = C::now();
    let t = |ms: u64| -> InstantOf<C> { base + Duration::from_millis(ms) };

    let mut log_stages = LogStages::<InstantOf<C>>::new(10, 0);

    // Batch 1: fast path — small queue delay, quick persist/commit/apply
    log_stages.proposed(10, t(0));
    log_stages.received(10, t(1));
    log_stages.submitted(10, t(1));
    log_stages.persisted(10, t(3));
    log_stages.committed(10, t(4));
    log_stages.applied(10, t(5));

    // Batch 2: moderate — slightly slower persist and replication
    log_stages.proposed(20, t(5));
    log_stages.received(20, t(6));
    log_stages.submitted(20, t(7));
    log_stages.persisted(20, t(10));
    log_stages.committed(20, t(12));
    log_stages.applied(20, t(13));

    // Batch 3: slow — storage stall causes higher latency
    log_stages.proposed(30, t(12));
    log_stages.received(30, t(13));
    log_stages.submitted(30, t(14));
    log_stages.persisted(30, t(22));
    log_stages.committed(30, t(25));
    log_stages.applied(30, t(28));

    let mut d = make_display(EMPTY_STATS, vec![0; CommandName::COUNT], empty_log_stages());
    d.log_stages = log_stages;

    let d = d.multiline();
    let s = format!("{}", d);
    println!("{s}");

    // Each proposal is shown relative to the first one, so the text depends on the recorded
    // instants alone.
    let want = [
        "  log_stage_segments (T: proposal of the first segment):",
        "    [0,10): T+0.00ns +0.00ns; proposed +0.00ns (0.00ns), received +1.00ms (1.00ms), submitted +0.00ns (1.00ms), persisted +2.00ms (3.00ms), committed +1.00ms (4.00ms), applied +1.00ms (5.00ms)",
        "    [10,20): T+5.00ms +5.00ms; proposed +0.00ns (0.00ns), received +1.00ms (1.00ms), submitted +1.00ms (2.00ms), persisted +3.00ms (5.00ms), committed +2.00ms (7.00ms), applied +1.00ms (8.00ms)",
        "    [20,30): T+12.00ms +7.00ms; proposed +0.00ns (0.00ns), received +1.00ms (1.00ms), submitted +1.00ms (2.00ms), persisted +8.00ms (10.00ms), committed +3.00ms (13.00ms), applied +3.00ms (16.00ms)",
    ];
    let start = s.find(want[0]).expect("the segments section");
    let got = s[start..].lines().collect::<Vec<_>>();
    assert_eq!(want.to_vec(), got);

    assert_eq!(s, format!("{}", d), "formatting again gives the same text");
}

#[test]
fn test_multiline_no_segments_when_empty() {
    let d = make_display(EMPTY_STATS, vec![0; CommandName::COUNT], empty_log_stages());
    let s = format!("{}", d.multiline());

    assert!(!s.contains("log_stage_segments"));
}

#[test]
fn test_multiline_with_counts() {
    let mut cmds = vec![0u64; CommandName::COUNT];
    cmds[0] = 7;

    let d = make_display(EMPTY_STATS, cmds, empty_log_stages());
    let s = format!("{}", d.multiline());

    assert!(s.contains("commands:"));
    assert!(s.contains(": 7"));
}

#[cfg(feature = "runtime-stats")]
#[test]
fn test_mode_builder() {
    let d = make_display(EMPTY_STATS, vec![0; CommandName::COUNT], empty_log_stages());
    let compact = format!("{}", d.compact());

    let d = make_display(EMPTY_STATS, vec![0; CommandName::COUNT], empty_log_stages());
    let multiline = format!("{}", d.multiline());

    let d = make_display(EMPTY_STATS, vec![0; CommandName::COUNT], empty_log_stages());
    let human = format!("{}", d.human_readable());

    assert!(compact.starts_with("RuntimeStats {"));
    assert!(multiline.starts_with("RuntimeStats:"));
    assert!(human.contains("Batch Sizes:"));
    assert_ne!(compact, multiline);
    assert_ne!(multiline, human);
}

#[cfg(feature = "runtime-stats")]
#[test]
fn test_human_readable_empty() {
    let d = make_display(EMPTY_STATS, vec![0; CommandName::COUNT], empty_log_stages());
    let s = format!("{}", d.human_readable());

    assert!(s.contains("Batch Sizes:"));
    assert!(s.contains("Budget & Utilization:"));
    // No command/message/notification/log-stage tables when all counts are zero
    assert!(!s.contains("Commands:"));
    assert!(!s.contains("Raft Messages:"));
    assert!(!s.contains("Notifications:"));
    assert!(!s.contains("Log Stage Latencies"));
}

#[cfg(feature = "runtime-stats")]
#[test]
fn test_human_readable_with_data() {
    let mut cmds = vec![0u64; CommandName::COUNT];
    cmds[0] = 1_234;

    let mut msgs = vec![0u64; RaftMsgName::COUNT];
    msgs[0] = 56;

    let mut stages = empty_log_stages();
    stages[0].1 = sample_stats(100, 50, 200);
    stages[5].1 = sample_stats(100, 120, 800);

    // Build histograms where each stage follows a log-normal distribution
    // (Gaussian on the log scale) with a different center.
    // 7 geometrically-spaced bucket values; each stage peaks at a
    // different position and tapers symmetrically on the log axis.
    //
    // values(us):            8   20   50  120  300  700 1500
    // 1:proposed→received   20   12    5    2    1    1    1  (peak@8)
    // 2:received→submitted   8   20   12    5    2    1    1  (peak@20)
    // 5:committed→applied    1    2    8   20   12    5    1  (peak@120)
    // 4:persisted→committed  1    1    2    8   20   12    5  (peak@300)
    // 3:submitted→persisted  1    1    1    2    8   20   12  (peak@700)
    let mut hist = LogStageHistograms::new();
    let values: [u64; 7] = [8, 20, 50, 120, 300, 700, 1500];
    let counts: [&[u64; 7]; 5] = [
        &[20, 12, 5, 2, 1, 1, 1],
        &[8, 20, 12, 5, 2, 1, 1],
        &[1, 1, 1, 2, 8, 20, 12],
        &[1, 1, 2, 8, 20, 12, 5],
        &[1, 2, 8, 20, 12, 5, 1],
    ];
    let fields = [
        &mut hist.proposed_to_received,
        &mut hist.received_to_submitted,
        &mut hist.submitted_to_persisted,
        &mut hist.persisted_to_committed,
        &mut hist.committed_to_applied,
    ];
    for (field, stage_counts) in fields.into_iter().zip(counts) {
        for (&v, &n) in values.iter().zip(stage_counts) {
            field.record_n(v, n);
        }
    }
    for (i, &v) in values.iter().enumerate() {
        let total: u64 = counts.iter().map(|c| c[i]).sum();
        hist.proposed_to_applied.record_n(v, total);
    }

    let mut d = make_display(sample_stats(10, 30, 150), cmds, stages);
    d.raft_msg_counts = msgs;
    d.log_stage_histograms = hist;
    let s = format!("{}", d.human_readable());
    println!("{s}");

    // Batch sizes table
    assert!(s.contains("Batch Sizes:"));
    assert!(s.contains("Apply"));

    // Counts tables
    assert!(s.contains("Commands:"));
    assert!(s.contains("1,234"));
    assert!(s.contains("Raft Messages:"));

    // Log stage percentile table
    assert!(s.contains("Log Stage Latencies (us):"));
    assert!(s.contains("1:proposed→received"));
    assert!(s.contains("1~5:proposed→applied"));
    // ASCII chart
    assert!(s.contains("Log Stage Latency Distribution (us):"));
    assert!(s.contains("1:proposed→received"));
    // Chart contains bar characters
    assert!(s.contains('█'));
}
