//! Configuration for EzRaft
//!
//! EzRaft provides sensible defaults for all Raft timing parameters.
//! Users can optionally customize a few key parameters.

use openraft::Config;

/// Configuration for EzRaft
///
/// Provides sensible defaults for all Raft timing parameters.
/// Most users can use `EzConfig::default()` directly.
#[derive(Clone, Debug)]
pub struct EzConfig {
    /// Cluster name (used for logging and metrics)
    pub cluster_name: String,

    /// Heartbeat interval in milliseconds
    ///
    /// Leader sends heartbeats to followers at this interval.
    /// Default: 500ms
    pub heartbeat_interval_ms: u64,

    /// Minimum election timeout in milliseconds
    ///
    /// Followers wait this long before starting an election.
    /// Default: 1500ms
    pub election_timeout_min_ms: u64,

    /// Maximum election timeout in milliseconds
    ///
    /// Followers wait up to this long before starting an election (randomized).
    /// Default: 3000ms
    pub election_timeout_max_ms: u64,
}

impl Default for EzConfig {
    fn default() -> Self {
        Self {
            cluster_name: "ezraft-cluster".to_string(),
            heartbeat_interval_ms: 500,
            election_timeout_min_ms: 1500,
            election_timeout_max_ms: 3000,
        }
    }
}

impl EzConfig {
    /// Convert to openraft Config
    ///
    /// Validates and creates the internal Raft configuration.
    pub(crate) fn to_raft_config(&self) -> Result<Config, openraft::ConfigError> {
        let config = Config {
            heartbeat_interval: self.heartbeat_interval_ms,
            election_timeout_min: self.election_timeout_min_ms,
            election_timeout_max: self.election_timeout_max_ms,
            ..Default::default()
        };

        config.validate()
    }
}
