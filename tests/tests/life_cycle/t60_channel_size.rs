use anyhow::Result;
use openraft::Config;
use openraft::errors::Fatal;
use openraft_memstore::TypeConfig;

use crate::fixtures::RaftRouter;
use crate::fixtures::ut_harness;

/// The largest buffer of a Tokio channel: `tokio::sync::Semaphore::MAX_PERMITS`.
const TOKIO_MAX_BUFFER: u64 = (usize::MAX >> 3) as u64;

/// Sets one channel size of a `Config`.
type SetSize = fn(&mut Config, u64);

/// A channel size that the runtime's channels do not accept makes `Raft::new` return
/// `Fatal::InvalidConfig` naming the field, instead of panicking while it builds the channel.
#[tracing::instrument]
#[test_harness::test(harness = ut_harness)]
async fn unsupported_channel_size_fails_startup() -> Result<()> {
    let fields: [(&str, SetSize); 3] = [
        ("api_channel_size", |config, size| config.api_channel_size = Some(size)),
        ("notification_channel_size", |config, size| {
            config.notification_channel_size = Some(size)
        }),
        ("state_machine_channel_size", |config, size| {
            config.state_machine_channel_size = Some(size)
        }),
    ];

    for (field, set_size) in fields {
        for size in [0, TOKIO_MAX_BUFFER + 1] {
            tracing::info!(field, size, "--- a size Tokio-backed channels reject fails Raft::new");
            {
                let mut config = Config {
                    enable_tick: false,
                    enable_heartbeat: false,
                    enable_elect: false,
                    ..Default::default()
                };
                set_size(&mut config, size);
                let mut router = RaftRouter::new(config)?;

                let started = router.new_raft_node(0).await;
                let error = started.unwrap_err();
                let fatal = error.downcast_ref::<Fatal<TypeConfig>>();
                let Some(Fatal::InvalidConfig { field: rejected, .. }) = fatal else {
                    panic!("expected Fatal::InvalidConfig for {field}={size}, got: {error:?}");
                };
                assert_eq!(field, rejected);
            }
        }

        tracing::info!(field, "--- a size of 1 starts and shuts down normally");
        {
            let mut config = Config {
                enable_tick: false,
                enable_heartbeat: false,
                enable_elect: false,
                ..Default::default()
            };
            set_size(&mut config, 1);
            let mut router = RaftRouter::new(config)?;

            router.new_raft_node(0).await?;
            router.stop_node(0).await?;
        }
    }

    Ok(())
}
