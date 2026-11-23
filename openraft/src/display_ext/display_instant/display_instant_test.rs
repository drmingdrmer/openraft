use std::time::Duration;
use std::time::UNIX_EPOCH;

use super::DisplayUtc;
use super::civil_from_days;
use super::unix_secs_micros;
use crate::display_ext::DisplayInstantExt;
use crate::engine::testing::UTConfig;
use crate::type_config::TypeConfigExt;

/// Check the result by a human.
#[test]
fn test_display_instant() -> anyhow::Result<()> {
    let now = UTConfig::<()>::now();
    println!("now: {}", now.display());
    println!("now: {}", now.display().full());
    println!("now: {}", now.display().full().simple());
    Ok(())
}

#[test]
fn test_display_utc() {
    let t = UNIX_EPOCH + Duration::new(1_721_829_051, 211_301_916);
    assert_eq!("2024-07-24T13:50:51.211301Z", DisplayUtc::<false>(t).to_string());
    assert_eq!("13:50:51.211301", DisplayUtc::<true>(t).to_string());

    assert_eq!(
        "1970-01-01T00:00:00.000000Z",
        DisplayUtc::<false>(UNIX_EPOCH).to_string()
    );

    let before_epoch = UNIX_EPOCH - Duration::from_micros(1);
    assert_eq!(
        "1969-12-31T23:59:59.999999Z",
        DisplayUtc::<false>(before_epoch).to_string()
    );
}

/// Every time stays in its own second and microsecond, on both sides of the epoch.
#[test]
fn test_unix_secs_micros() {
    let nanos = |n: u64| Duration::from_nanos(n);

    let after = [
        (1, (0, 0)),
        (999, (0, 0)),
        (1_000, (0, 1)),
        (1_001, (0, 1)),
        (999_999_999, (0, 999_999)),
        (1_000_000_000, (1, 0)),
        (1_000_000_001, (1, 0)),
    ];
    for (n, want) in after {
        assert_eq!(want, unix_secs_micros(UNIX_EPOCH + nanos(n)), "{n} ns after the epoch");
    }

    let before = [
        (1, (-1, 999_999)),
        (999, (-1, 999_999)),
        (1_000, (-1, 999_999)),
        (1_001, (-1, 999_998)),
        (999_999_999, (-1, 0)),
        (1_000_000_000, (-1, 0)),
        (1_000_000_001, (-2, 999_999)),
    ];
    for (n, want) in before {
        assert_eq!(want, unix_secs_micros(UNIX_EPOCH - nanos(n)), "{n} ns before the epoch");
    }
}

#[test]
fn test_display_utc_around_the_epoch() {
    let cases = [
        (UNIX_EPOCH - Duration::from_nanos(1), "1969-12-31T23:59:59.999999Z"),
        (UNIX_EPOCH - Duration::from_nanos(1_001), "1969-12-31T23:59:59.999998Z"),
        (
            UNIX_EPOCH - Duration::from_nanos(999_999_999),
            "1969-12-31T23:59:59.000000Z",
        ),
        (
            UNIX_EPOCH - Duration::from_nanos(1_000_000_001),
            "1969-12-31T23:59:58.999999Z",
        ),
        (UNIX_EPOCH + Duration::from_nanos(999), "1970-01-01T00:00:00.000000Z"),
        (
            UNIX_EPOCH + Duration::from_nanos(1_000_000_001),
            "1970-01-01T00:00:01.000000Z",
        ),
    ];
    for (t, full) in cases {
        assert_eq!(full, DisplayUtc::<false>(t).to_string());
        let simple = &full["yyyy-mm-ddT".len()..full.len() - 1];
        assert_eq!(simple, DisplayUtc::<true>(t).to_string());
    }
}

#[test]
fn test_civil_from_days() {
    assert_eq!((1970, 1, 1), civil_from_days(0));
    assert_eq!((1969, 12, 31), civil_from_days(-1));
    assert_eq!((2000, 2, 29), civil_from_days(11_016));
    assert_eq!((2000, 3, 1), civil_from_days(11_017));
    assert_eq!((2024, 1, 1), civil_from_days(19_723));
    assert_eq!((2024, 7, 24), civil_from_days(19_928));
}
