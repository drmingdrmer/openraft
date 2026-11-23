use std::time::Duration;

use super::QuorumLossProbe;
use crate::engine::testing::UTConfig;
use crate::type_config::TypeConfigExt;

#[test]
fn test_quiet_for() {
    let ms = Duration::from_millis;
    let t = UTConfig::<()>::now();
    let probe = QuorumLossProbe {
        quorum_acked_at: t,
        leader_lease: ms(300),
        interval: ms(600),
    };

    assert_eq!(None, probe.quiet_for(t));
    assert_eq!(None, probe.quiet_for(t + ms(299)));
    assert_eq!(Some(ms(600)), probe.quiet_for(t + ms(300)));
    assert_eq!(Some(ms(1)), probe.quiet_for(t + ms(899)));
    assert_eq!(None, probe.quiet_for(t + ms(900)));
    assert_eq!(None, probe.quiet_for(t + ms(1_499)));
    assert_eq!(Some(ms(400)), probe.quiet_for(t + ms(1_700)));
}

#[test]
fn test_phase() {
    let ms = Duration::from_millis;
    let t = UTConfig::<()>::now();
    let probe = QuorumLossProbe {
        quorum_acked_at: t,
        leader_lease: ms(300),
        interval: ms(600),
    };
    for (elapsed, expected) in [
        (0, (false, ms(300))),
        (300, (true, ms(600))),
        (900, (false, ms(600))),
        (1_499, (false, ms(1))),
    ] {
        let now = t + ms(elapsed);
        let actual = probe.phase(now);
        assert_eq!(expected, actual);
    }
}
