use std::io;
use std::ops::Add;
use std::ops::AddAssign;
use std::ops::Sub;
use std::ops::SubAssign;
use std::time::Duration;
use std::time::SystemTime;
use std::time::UNIX_EPOCH;

use serde::Deserialize;
use serde::Serialize;
use serde::de::value::U64Deserializer;

use super::SerdeInstant;
use crate::Instant;
use crate::engine::testing::UTConfig;
use crate::type_config::TypeConfigExt;
use crate::type_config::alias::SerdeInstantOf;

const DAY: u64 = 86_400;

/// An instant of a clock that counts a `Duration` from its start, and whose `now()` is always
/// `NOW_SECS` seconds. It cannot represent a time before its start or past `Duration::MAX`, and its
/// operators panic there, as those of a platform clock do.
#[derive(Debug, Clone, Copy)]
#[derive(PartialEq, Eq, PartialOrd, Ord)]
struct TestInstant<const NOW_SECS: u64>(Duration);

/// A clock that started a day ago.
type StartedDayAgo = TestInstant<DAY>;

/// A clock that cannot represent a time more than a day from now.
type EndsInDay = TestInstant<{ u64::MAX - DAY }>;

/// A clock that can represent any time within 292 billion years of now, so its range covers every
/// `u64` count of nanoseconds since the Unix epoch.
type WideRange = TestInstant<{ u64::MAX / 2 }>;

impl<const NOW_SECS: u64> Add<Duration> for TestInstant<NOW_SECS> {
    type Output = Self;

    fn add(self, rhs: Duration) -> Self {
        Self(self.0 + rhs)
    }
}

impl<const NOW_SECS: u64> AddAssign<Duration> for TestInstant<NOW_SECS> {
    fn add_assign(&mut self, rhs: Duration) {
        self.0 += rhs;
    }
}

impl<const NOW_SECS: u64> Sub<Duration> for TestInstant<NOW_SECS> {
    type Output = Self;

    fn sub(self, rhs: Duration) -> Self {
        Self(self.0 - rhs)
    }
}

impl<const NOW_SECS: u64> Sub for TestInstant<NOW_SECS> {
    type Output = Duration;

    fn sub(self, rhs: Self) -> Duration {
        self.0.saturating_sub(rhs.0)
    }
}

impl<const NOW_SECS: u64> SubAssign<Duration> for TestInstant<NOW_SECS> {
    fn sub_assign(&mut self, rhs: Duration) {
        self.0 -= rhs;
    }
}

impl<const NOW_SECS: u64> Instant for TestInstant<NOW_SECS> {
    fn now() -> Self {
        Self(Duration::from_secs(NOW_SECS))
    }

    fn checked_add(&self, duration: Duration) -> Option<Self> {
        self.0.checked_add(duration).map(Self)
    }

    fn checked_sub(&self, duration: Duration) -> Option<Self> {
        self.0.checked_sub(duration).map(Self)
    }
}

/// A JSON formatter that writes `u64:` before an integer that the serializer receives as a `u64`.
/// Plain JSON writes such an integer as it writes a nonnegative `i64`.
struct MarkU64;

impl serde_json::ser::Formatter for MarkU64 {
    fn write_u64<W>(&mut self, writer: &mut W, value: u64) -> io::Result<()>
    where W: ?Sized + io::Write {
        write!(writer, "u64:{value}")
    }
}

/// A round trip through the runtime's clock moves an instant an hour before now, now, or an hour
/// after now by at most the time that the round trip takes.
///
/// Encoding and decoding each read the system clock and the runtime's clock. The round trip moves
/// the instant later by the time the runtime's clock advanced between its two reads, at most
/// `elapsed`, and earlier by the time the system clock advanced between its two reads, at most
/// `sys_elapsed`.
#[test]
fn test_serde_instant() {
    let hour = Duration::from_secs(3600);
    let now = UTConfig::<()>::now();

    for instant in [now - hour, now, now + hour] {
        let start = UTConfig::<()>::now();
        let sys_start = SystemTime::now();

        let json = serde_json::to_string(&SerdeInstant::new(instant)).unwrap();
        let decoded: SerdeInstantOf<UTConfig> = serde_json::from_str(&json).unwrap();

        let sys_elapsed = sys_start.elapsed().unwrap();
        let elapsed = start.elapsed();

        assert!(
            (instant - sys_elapsed..=instant + elapsed).contains(&*decoded),
            "{instant:?} decoded as {decoded:?}, more than {sys_elapsed:?} earlier or {elapsed:?} later"
        );
    }
}

/// Encoding passes the time to the serializer as a `u64`. This checks only the type;
/// `test_serialize_past_and_future` checks the value.
#[test]
fn test_serialize_as_u64() {
    let mut out = Vec::new();
    let mut serializer = serde_json::Serializer::with_formatter(&mut out, MarkU64);
    SerdeInstant::new(UTConfig::<()>::now()).serialize(&mut serializer).unwrap();

    let out = String::from_utf8(out).unwrap();
    let nanos = out.strip_prefix("u64:").map(str::parse::<u64>);
    assert!(matches!(nanos, Some(Ok(_))), "expected u64:<nanos>, got {out}");
}

/// Encoding an instant an hour after or before now writes the nanoseconds since the Unix epoch of
/// the system time an hour after or before now.
///
/// `StartedDayAgo`'s clock always reads the same `now`, so only the system clock moves the result.
/// Encoding reads it between `before` and `after`.
#[test]
fn test_serialize_past_and_future() {
    let hour = Duration::from_secs(3600);

    tracing::info!("an instant an hour after now encodes as the system time an hour after now");
    {
        let before = SystemTime::now();
        let json = serde_json::to_string(&SerdeInstant::new(StartedDayAgo::now() + hour)).unwrap();
        let after = SystemTime::now();

        let encoded = UNIX_EPOCH + Duration::from_nanos(json.parse().unwrap());
        assert!(
            (before + hour..=after + hour).contains(&encoded),
            "{encoded:?} is not an hour after {before:?}..={after:?}"
        );
    }

    tracing::info!("an instant an hour before now encodes as the system time an hour before now");
    {
        let before = SystemTime::now();
        let json = serde_json::to_string(&SerdeInstant::new(StartedDayAgo::now() - hour)).unwrap();
        let after = SystemTime::now();

        let encoded = UNIX_EPOCH + Duration::from_nanos(json.parse().unwrap());
        assert!(
            (before - hour..=after - hour).contains(&encoded),
            "{encoded:?} is not an hour before {before:?}..={after:?}"
        );
    }
}

/// Decoding the nanoseconds since the Unix epoch of the system time an hour after or before now
/// gives the instant an hour after or before now.
///
/// `StartedDayAgo`'s clock always reads the same `now`, so only the system clock moves the result.
/// Decoding reads it at most `elapsed` after `before`, so the decoded instant lies at most
/// `elapsed` before the expected one.
#[test]
fn test_deserialize_past_and_future() {
    let hour = Duration::from_secs(3600);

    tracing::info!("the system time an hour after now decodes as the instant an hour after now");
    {
        let before = SystemTime::now();
        let nanos = (before + hour).duration_since(UNIX_EPOCH).unwrap().as_nanos();
        let decoded: SerdeInstant<StartedDayAgo> = serde_json::from_str(&nanos.to_string()).unwrap();
        let elapsed = before.elapsed().unwrap();

        let expected = StartedDayAgo::now() + hour;
        assert!(
            (expected - elapsed..=expected).contains(&*decoded),
            "{decoded:?} is not at most {elapsed:?} before {expected:?}"
        );
    }

    tracing::info!("the system time an hour before now decodes as the instant an hour before now");
    {
        let before = SystemTime::now();
        let nanos = (before - hour).duration_since(UNIX_EPOCH).unwrap().as_nanos();
        let decoded: SerdeInstant<StartedDayAgo> = serde_json::from_str(&nanos.to_string()).unwrap();
        let elapsed = before.elapsed().unwrap();

        let expected = StartedDayAgo::now() - hour;
        assert!(
            (expected - elapsed..=expected).contains(&*decoded),
            "{decoded:?} is not at most {elapsed:?} before {expected:?}"
        );
    }
}

/// Decoding returns an error for a time that the runtime's clock cannot represent, before or after
/// now.
#[test]
fn test_deserialize_out_of_clock_range() {
    let now_nanos = u64::try_from(SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos()).unwrap();
    let is_clock_range_error = |e: &serde_json::Error| e.to_string().contains("of the runtime's clock");

    tracing::info!(
        now_nanos,
        "a clock that started a day ago decodes now, but cannot represent the Unix epoch"
    );
    {
        let decoded = serde_json::from_str::<SerdeInstant<StartedDayAgo>>(&now_nanos.to_string());
        assert!(decoded.is_ok(), "{decoded:?}");

        let err = serde_json::from_str::<SerdeInstant<StartedDayAgo>>("0").unwrap_err();
        assert!(is_clock_range_error(&err), "{err}");
    }

    tracing::info!(
        now_nanos,
        "a clock that ends in a day decodes now, but cannot represent i64::MAX ns, in 2262"
    );
    {
        let decoded = serde_json::from_str::<SerdeInstant<EndsInDay>>(&now_nanos.to_string());
        assert!(decoded.is_ok(), "{decoded:?}");

        let err = serde_json::from_str::<SerdeInstant<EndsInDay>>(&i64::MAX.to_string()).unwrap_err();
        assert!(is_clock_range_error(&err), "{err}");
    }
}

/// Encoding returns an error for an instant whose wall-clock time `SystemTime` cannot represent,
/// before or after now.
#[test]
fn test_serialize_out_of_system_time_range() {
    tracing::info!("now encodes, but SystemTime cannot represent an instant about Duration::MAX after it");
    {
        let now = SerdeInstant::new(StartedDayAgo::now());
        assert!(serde_json::to_string(&now).is_ok());

        let far: SerdeInstant<StartedDayAgo> = SerdeInstant::new(TestInstant(Duration::MAX));
        let err = serde_json::to_string(&far).unwrap_err();
        assert_eq!(err.to_string(), "time out of range");
    }

    tracing::info!("now encodes, but SystemTime cannot represent an instant about Duration::MAX before it");
    {
        let now = SerdeInstant::new(EndsInDay::now());
        assert!(serde_json::to_string(&now).is_ok());

        let far: SerdeInstant<EndsInDay> = SerdeInstant::new(TestInstant(Duration::ZERO));
        let err = serde_json::to_string(&far).unwrap_err();
        assert_eq!(err.to_string(), "time out of range");
    }
}

/// Encoding and decoding share a range that ends at `i64::MAX` nanoseconds since the Unix epoch,
/// in year 2262. Decoding rejects a later time even when the runtime's clock can represent it.
#[test]
fn test_out_of_encoding_range() {
    let max = i64::MAX as u64;
    let decode = |nanos: u64| {
        let deserializer = U64Deserializer::<serde::de::value::Error>::new(nanos);
        SerdeInstant::<WideRange>::deserialize(deserializer)
    };

    tracing::info!(max, "decoding accepts the end of the range, and rejects any later time");
    {
        let decoded = decode(max);
        assert!(decoded.is_ok(), "{decoded:?}");

        for nanos in [max + 1, u64::MAX] {
            let err = decode(nanos).unwrap_err();
            assert_eq!(err.to_string(), "time out of range", "decoding {nanos}");
        }
    }

    tracing::info!(
        max,
        "encoding accepts a time a day before the end of the range, and rejects one a day after it; \
         the system clock moves far less between decoding and encoding"
    );
    {
        let day = Duration::from_secs(DAY);
        let end = decode(max).unwrap().into_inner();

        let encoded = serde_json::to_string(&SerdeInstant::new(end - day));
        assert!(encoded.is_ok(), "{encoded:?}");

        let err = serde_json::to_string(&SerdeInstant::new(end + day)).unwrap_err();
        assert_eq!(err.to_string(), "time out of range");
    }
}
