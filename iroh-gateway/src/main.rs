use std::sync::Arc;

use anyhow::{anyhow, Result};
use clap::Parser;
use iroh_gateway::{
    bad_bits::{self, BadBits},
    cli::Args,
    config::{Config, CONFIG_FILE_NAME, ENV_PREFIX},
    core::Core,
    metrics,
};
use iroh_metrics::gateway::Metrics;
use iroh_util::{iroh_home_path, make_config};
use prometheus_client::registry::Registry;
use tokio::sync::RwLock;

#[tokio::main(flavor = "multi_thread")]
async fn main() -> Result<()> {
    let args = Args::parse();

    let sources = vec![iroh_home_path(CONFIG_FILE_NAME), args.cfg.clone()];
    let mut config = make_config(
        // default
        Config::default(),
        // potential config files
        sources,
        // env var prefix for this config
        ENV_PREFIX,
        // map of present command line arguments
        args.make_overrides_map(),
    )
    .unwrap();
    config.metrics = metrics::metrics_config_with_compile_time_info(config.metrics);
    println!("{:#?}", config);

    let metrics_config = config.metrics.clone();
    let mut prom_registry = Registry::default();
    let gw_metrics = Metrics::new(&mut prom_registry);
    let bad_bits = match config.denylist {
        true => Arc::new(Some(RwLock::new(BadBits::new()))),
        false => Arc::new(None),
    };
    let rpc_addr = config
        .server_rpc_addr()?
        .ok_or_else(|| anyhow!("missing gateway rpc addr"))?;
    let handler = Core::new(
        Arc::new(config),
        rpc_addr,
        gw_metrics,
        &mut prom_registry,
        Arc::clone(&bad_bits),
    )
    .await?;

    let bad_bits_handle = bad_bits::spawn_bad_bits_updater(Arc::clone(&bad_bits));

    let metrics_handle =
        iroh_metrics::MetricsHandle::from_registry_with_tracer(metrics_config, prom_registry)
            .await
            .expect("failed to initialize metrics");
    let server = handler.server();
    println!("listening on {}", server.local_addr());
    let core_task = tokio::spawn(async move {
        server.await.unwrap();
    });

    iroh_util::block_until_sigint().await;
    core_task.abort();

    metrics_handle.shutdown();
    if let Some(handle) = bad_bits_handle {
        handle.abort();
    }

    Ok(())
}
