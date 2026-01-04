//! Configuration for EzRaft
//!
//! EzRaft provides sensible defaults for all Raft timing parameters.
//! Users can optionally customize a few key parameters.

use std::time::Duration;

use openraft::Config;

/// Configuration for EzRaft
///
/// Provides sensible defaults for all Raft timing parameters.
/// Most users can use `EzConfig::default()` directly.
///
/// Election timeout is automatically calculated as 3-6x the heartbeat interval.
#[derive(Clone, Debug)]
pub struct EzConfig {
    /// Heartbeat interval
    ///
    /// Leader sends heartbeats to followers at this interval.
    /// Election timeout is derived from this value (3x to 6x).
    /// Default: 500ms
    pub heartbeat_interval: Duration,
}

impl Default for EzConfig {
    fn default() -> Self {
        Self {
            heartbeat_interval: Duration::from_millis(500),
        }
    }
}

impl EzConfig {
    /// Convert to openraft Config
    ///
    /// Validates and creates the internal Raft configuration.
    /// Election timeout is calculated as 3x to 6x the heartbeat interval.
    pub(crate) fn to_raft_config(&self) -> Result<Config, openraft::ConfigError> {
        let heartbeat_ms = self.heartbeat_interval.as_millis() as u64;

        let config = Config {
            heartbeat_interval: heartbeat_ms,
            election_timeout_min: heartbeat_ms * 3,
            election_timeout_max: heartbeat_ms * 6,
            ..Default::default()
        };

        config.validate()
    }
}
