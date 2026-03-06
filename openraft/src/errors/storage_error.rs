use std::fmt;

use crate::RaftComposites;
use crate::RaftPrimitives;
use crate::errors::ErrorSource;
use crate::storage::SnapshotSignature;
use crate::type_config::alias::ErrorSourceOf;
use crate::type_config::alias::LogIdOf;

/// Convert error to StorageError::IO();
pub trait ToStorageResult<C, T>
where C: RaftComposites
{
    /// Convert `Result<T, E>` to `Result<T, StorageError>`
    ///
    /// `f` provides error context for building the StorageError.
    fn sto_res<F>(self, f: F) -> Result<T, StorageError<C>>
    where F: FnOnce() -> (ErrorSubject<C::Prim>, ErrorVerb);
}

impl<C, T> ToStorageResult<C, T> for Result<T, std::io::Error>
where C: RaftComposites
{
    fn sto_res<F>(self, f: F) -> Result<T, StorageError<C>>
    where F: FnOnce() -> (ErrorSubject<C::Prim>, ErrorVerb) {
        match self {
            Ok(x) => Ok(x),
            Err(e) => {
                let (subject, verb) = f();
                let io_err = StorageError::new(subject, verb, ErrorSourceOf::<C>::from_error(&e));
                Err(io_err)
            }
        }
    }
}

/// The subject of a storage error, indicating what operation or component failed.
#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Deserialize, serde::Serialize), serde(bound = ""))]
pub enum ErrorSubject<P>
where P: RaftPrimitives
{
    /// A general storage error
    Store,

    /// HardState related error.
    Vote,

    /// Error that happened when operating a series of log entries
    Logs,

    /// Error about a single log entry
    Log(LogIdOf<P>),

    /// Error about a single log entry without knowing the log term.
    LogIndex(u64),

    /// Error happened when applying a log entry
    Apply(LogIdOf<P>),

    /// Error that happened when operating state machine.
    StateMachine,

    /// Error that happened when operating snapshots.
    Snapshot(Option<SnapshotSignature<P>>),

    /// No specific subject for this error.
    None,
}

/// What it is doing when an error occurs.
#[derive(Debug)]
#[derive(Clone, Copy)]
#[derive(PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Deserialize, serde::Serialize))]
pub enum ErrorVerb {
    /// Reading data.
    Read,
    /// Writing data.
    Write,
    /// Seeking in data.
    Seek,
    /// Deleting data.
    Delete,
}

impl fmt::Display for ErrorVerb {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{:?}", self)
    }
}

/// Backward compatible with old application using `StorageIOError`
#[deprecated(note = "use StorageError instead", since = "0.10.0")]
pub type StorageIOError<C> = StorageError<C>;

impl<C> StorageError<C>
where C: RaftComposites
{
    /// Backward compatible with old form `StorageError::IO{ source: StorageError }`
    #[deprecated(note = "no need to call this method", since = "0.10.0")]
    pub fn into_io(self) -> Option<StorageError<C>> {
        Some(self)
    }

    /// Create a StorageError from a std::io::Error.
    pub fn from_io_error(subject: ErrorSubject<C::Prim>, verb: ErrorVerb, io_error: std::io::Error) -> Self {
        StorageError::new(subject, verb, ErrorSourceOf::<C>::from_error(&io_error))
    }
}

impl<C> From<StorageError<C>> for std::io::Error
where C: RaftComposites
{
    fn from(e: StorageError<C>) -> Self {
        std::io::Error::other(e.to_string())
    }
}

/// Error that occurs when operating the store.
///
/// It indicates a data crash.
/// An application returning this error will shut down the Openraft node immediately to prevent
/// further damage.
#[derive(Debug, Clone, thiserror::Error, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Deserialize, serde::Serialize), serde(bound = ""))]
pub struct StorageError<C>
where C: RaftComposites
{
    subject: ErrorSubject<C::Prim>,
    verb: ErrorVerb,
    source: C::ErrorSource,
}

impl<C> fmt::Display for StorageError<C>
where C: RaftComposites
{
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "when {:?} {:?}: {}", self.verb, self.subject, self.source)
    }
}

impl<C> StorageError<C>
where C: RaftComposites
{
    /// Create a new StorageError.
    pub fn new(subject: ErrorSubject<C::Prim>, verb: ErrorVerb, source: C::ErrorSource) -> Self {
        Self { subject, verb, source }
    }

    /// Create an error for writing a log entry.
    pub fn write_log_entry(log_id: LogIdOf<C::Prim>, source: C::ErrorSource) -> Self {
        Self::new(ErrorSubject::Log(log_id), ErrorVerb::Write, source)
    }

    /// Create an error for reading a log entry at an index.
    pub fn read_log_at_index(log_index: u64, source: C::ErrorSource) -> Self {
        Self::new(ErrorSubject::LogIndex(log_index), ErrorVerb::Read, source)
    }

    /// Create an error for reading a log entry.
    pub fn read_log_entry(log_id: LogIdOf<C::Prim>, source: C::ErrorSource) -> Self {
        Self::new(ErrorSubject::Log(log_id), ErrorVerb::Read, source)
    }

    /// Create an error for writing multiple log entries.
    pub fn write_logs(source: C::ErrorSource) -> Self {
        Self::new(ErrorSubject::Logs, ErrorVerb::Write, source)
    }

    /// Create an error for reading multiple log entries.
    pub fn read_logs(source: C::ErrorSource) -> Self {
        Self::new(ErrorSubject::Logs, ErrorVerb::Read, source)
    }

    /// Create an error for writing vote state.
    pub fn write_vote(source: C::ErrorSource) -> Self {
        Self::new(ErrorSubject::Vote, ErrorVerb::Write, source)
    }

    /// Create an error for reading vote state.
    pub fn read_vote(source: C::ErrorSource) -> Self {
        Self::new(ErrorSubject::Vote, ErrorVerb::Read, source)
    }

    /// Create an error for applying a log entry to the state machine.
    pub fn apply(log_id: LogIdOf<C::Prim>, source: C::ErrorSource) -> Self {
        Self::new(ErrorSubject::Apply(log_id), ErrorVerb::Write, source)
    }

    /// Create an error for writing to the state machine.
    pub fn write_state_machine(source: C::ErrorSource) -> Self {
        Self::new(ErrorSubject::StateMachine, ErrorVerb::Write, source)
    }

    /// Create an error for reading from the state machine.
    pub fn read_state_machine(source: C::ErrorSource) -> Self {
        Self::new(ErrorSubject::StateMachine, ErrorVerb::Read, source)
    }

    /// Create an error for writing a snapshot.
    pub fn write_snapshot(signature: Option<SnapshotSignature<C::Prim>>, source: C::ErrorSource) -> Self {
        Self::new(ErrorSubject::Snapshot(signature), ErrorVerb::Write, source)
    }

    /// Create an error for reading a snapshot.
    pub fn read_snapshot(signature: Option<SnapshotSignature<C::Prim>>, source: C::ErrorSource) -> Self {
        Self::new(ErrorSubject::Snapshot(signature), ErrorVerb::Read, source)
    }

    /// General read error
    pub fn read(source: C::ErrorSource) -> Self {
        Self::new(ErrorSubject::Store, ErrorVerb::Read, source)
    }

    /// General write error
    pub fn write(source: C::ErrorSource) -> Self {
        Self::new(ErrorSubject::Store, ErrorVerb::Write, source)
    }
}

#[cfg(test)]
mod tests {
    use anyerror::AnyError;

    /// If `bt` feature is enabled, `backtrace` field is included in the serialized error.
    #[cfg(all(feature = "serde", not(feature = "bt")))]
    #[test]
    fn test_storage_error_serde() {
        use super::StorageError;
        use crate::engine::testing::UTConfig;
        use crate::engine::testing::log_id;

        let err = StorageError::write_log_entry(log_id(1, 2, 3), AnyError::error("test"));
        let s = serde_json::to_string(&err).unwrap();
        assert_eq!(
            s,
            r#"{"subject":{"Log":{"leader_id":{"term":1,"node_id":2},"index":3}},"verb":"Write","source":{"typ":null,"msg":"test","source":null,"context":[],"backtrace":null}}"#
        );
        let err2: StorageError<UTConfig> = serde_json::from_str(&s).unwrap();
        assert_eq!(err, err2);
    }

    /// Test backward compatibility: deserializing from old format that included `backtrace` field.
    ///
    /// Since 0.10.0, `backtrace` is removed, and it should be able to deserialize it.
    #[cfg(all(feature = "serde", not(feature = "bt")))]
    #[test]
    fn test_storage_error_deserialize_old_format_with_backtrace() {
        use super::StorageError;
        use crate::engine::testing::UTConfig;

        // Old serialized format with the redundant `backtrace` field
        let old_format = r#"{"subject":{"Log":{"leader_id":{"term":1,"node_id":2},"index":3}},"verb":"Write","source":{"typ":null,"msg":"test","source":null,"context":[],"backtrace":null},"backtrace":"some backtrace"}"#;

        // Should deserialize successfully, ignoring the `backtrace` field
        let err: StorageError<UTConfig> = serde_json::from_str(old_format).unwrap();
        assert_eq!(
            err.to_string(),
            "when Write Log(LogId { leader_id: LeaderId { term: 1, node_id: 2 }, index: 3 }): test"
        );
    }

    #[test]
    fn test_storage_error_to_io_error() {
        use super::StorageError;
        use crate::engine::testing::UTConfig;
        use crate::engine::testing::log_id;

        let storage_err: StorageError<UTConfig> =
            StorageError::write_log_entry(log_id(1, 2, 3), AnyError::error("disk full"));
        let io_err: std::io::Error = storage_err.into();

        assert_eq!(io_err.kind(), std::io::ErrorKind::Other);
        assert!(io_err.to_string().contains("Write"));
        assert!(io_err.to_string().contains("disk full"));

        let storage_err: StorageError<UTConfig> = StorageError::read_vote(AnyError::error("permission denied"));
        let io_err: std::io::Error = storage_err.into();

        assert_eq!(io_err.kind(), std::io::ErrorKind::Other);
        assert!(io_err.to_string().contains("Read"));
        assert!(io_err.to_string().contains("Vote"));
        assert!(io_err.to_string().contains("permission denied"));
    }
}
