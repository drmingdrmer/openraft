use std::fmt;

use base2histogram::AsciiChart;
use base2histogram::Histogram;
use base2histogram::PercentileStats;

/// Log lifecycle latency histograms in microseconds.
///
/// Each segment contributes one sample per log entry it covers. A segment
/// covering many entries contributes many identical samples, so percentiles
/// are weighted by entry count, not by segment or batch count.
///
/// Unobserved earlier stages are backfilled with the first observed timestamp
/// for those entries. For example, a follower that first observes entries at
/// log storage submission uses the submission time for both proposal and
/// receipt. Its proposal-to-receipt and receipt-to-submission samples are zero
/// because those intervals were not observed, not because they were measured
/// to take zero time.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LogStageHistograms {
    /// From `client_write()` to `RaftCore` dequeuing the request: API channel queueing.
    pub proposed_to_received: Histogram,
    /// From `RaftCore` dequeuing the request to submitting the entry to log storage.
    pub received_to_submitted: Histogram,
    /// From submitting the entry to log storage until storage persists it: storage I/O latency.
    pub submitted_to_persisted: Histogram,
    /// From persistence to the local commit of the entry.
    pub persisted_to_committed: Histogram,
    /// From the local commit to apply completion.
    pub committed_to_applied: Histogram,
    /// Per-entry latency from proposal to apply completion.
    ///
    /// When a follower first observes an entry at log storage submission,
    /// this measures from local submission to apply completion, not from the
    /// client's proposal.
    pub proposed_to_applied: Histogram,
}

impl LogStageHistograms {
    pub(crate) fn new() -> Self {
        Self {
            proposed_to_received: Histogram::new(),
            received_to_submitted: Histogram::new(),
            submitted_to_persisted: Histogram::new(),
            persisted_to_committed: Histogram::new(),
            committed_to_applied: Histogram::new(),
            proposed_to_applied: Histogram::new(),
        }
    }
}

impl LogStageHistograms {
    pub(crate) fn percentile_stats_array(&self) -> [(&'static str, PercentileStats); 6] {
        [
            ("1:proposed→received", self.proposed_to_received.percentile_stats()),
            ("2:received→submitted", self.received_to_submitted.percentile_stats()),
            ("3:submitted→persisted", self.submitted_to_persisted.percentile_stats()),
            ("4:persisted→committed", self.persisted_to_committed.percentile_stats()),
            ("5:committed→applied", self.committed_to_applied.percentile_stats()),
            ("1~5:proposed→applied", self.proposed_to_applied.percentile_stats()),
        ]
    }

    /// Build a stacked ASCII chart of all stage-to-stage latency histograms.
    pub(crate) fn ascii_chart(&self) -> AsciiChart {
        AsciiChart::new()
            .add("1:proposed→received", self.proposed_to_received.clone())
            .add("2:received→submitted", self.received_to_submitted.clone())
            .add("3:submitted→persisted", self.submitted_to_persisted.clone())
            .add("4:persisted→committed", self.persisted_to_committed.clone())
            .add("5:committed→applied", self.committed_to_applied.clone())
    }
}

impl fmt::Display for LogStageHistograms {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let stages = [
            ("proposed→received", &self.proposed_to_received),
            ("received→submitted", &self.received_to_submitted),
            ("submitted→persisted", &self.submitted_to_persisted),
            ("persisted→committed", &self.persisted_to_committed),
            ("committed→applied", &self.committed_to_applied),
            ("proposed→applied", &self.proposed_to_applied),
        ];

        for (name, hist) in &stages {
            let stats = hist.percentile_stats();
            writeln!(
                f,
                "{:>22}: n={} p50={}us p90={}us p99={}us p99.9={}us",
                name, stats.samples, stats.p50, stats.p90, stats.p99, stats.p99_9,
            )?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_histograms_display() {
        let h = LogStageHistograms::new();
        let s = format!("{}", h);
        assert!(s.contains("proposed→received"));
        assert!(s.contains("proposed→applied"));
    }
}
