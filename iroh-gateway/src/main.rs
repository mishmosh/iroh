use std::io::Write;
use std::path::PathBuf;
use std::sync::Arc;
use std::{collections::HashMap, fs::File};

use axum::{routing::get, Router};
use std::net::SocketAddr;

use anyhow::{anyhow, Result};
use clap::Parser;
use iroh_gateway::{
    bad_bits::{self, BadBits},
    config::{Config, CONFIG_FILE_NAME, ENV_PREFIX},
    core::Core,
    metrics,
};
use iroh_metrics::gateway::Metrics;
use iroh_util::{iroh_home_path, make_config};
use pprof::protos::Message;
use prometheus_client::registry::Registry;
use tokio::sync::RwLock;

#[derive(Parser, Debug, Clone)]
#[clap(author, version, about, long_about = None)]
struct Args {
    #[clap(short, long)]
    port: Option<u16>,
    #[clap(short, long)]
    writeable: Option<bool>,
    #[clap(short, long)]
    fetch: Option<bool>,
    #[clap(short, long)]
    cache: Option<bool>,
    #[clap(long = "metrics")]
    metrics: bool,
    #[clap(long = "tracing")]
    tracing: bool,
    #[clap(long)]
    cfg: Option<PathBuf>,
    #[clap(long)]
    denylist: Option<bool>,
}

impl Args {
    fn make_overrides_map(&self) -> HashMap<&str, String> {
        let mut map: HashMap<&str, String> = HashMap::new();
        if let Some(port) = self.port {
            map.insert("port", port.to_string());
        }
        if let Some(writable) = self.writeable {
            map.insert("writable", writable.to_string());
        }
        if let Some(fetch) = self.fetch {
            map.insert("fetch", fetch.to_string());
        }
        if let Some(cache) = self.cache {
            map.insert("cache", cache.to_string());
        }
        if let Some(denylist) = self.denylist {
            map.insert("denylist", denylist.to_string());
        }
        map.insert("metrics.collect", self.metrics.to_string());
        map.insert("metrics.tracing", self.tracing.to_string());
        map
    }
}

#[tokio::main(flavor = "multi_thread", worker_threads = 8)]
async fn main() -> Result<()> {
    // let guard = pprof::ProfilerGuardBuilder::default().frequency(100).blocklist(&["libc", "libgcc", "pthread", "vdso"]).build().unwrap();
    // let guard = pprof::ProfilerGuardBuilder::default()
    //     .frequency(1000)
    //     .build()
    //     .unwrap();
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
    let use_denylist = config.denylist;
    println!("{:#?}", config);

    let metrics_config = config.metrics.clone();
    let mut prom_registry = Registry::default();
    let gw_metrics = Metrics::new(&mut prom_registry);
    let bad_bits = Arc::new(RwLock::new(BadBits::new()));
    let rpc_addr = config
        .server_rpc_addr()?
        .ok_or_else(|| anyhow!("missing gateway rpc addr"))?;
    let handler = Core::new(
        config,
        rpc_addr,
        gw_metrics,
        &mut prom_registry,
        Arc::clone(&bad_bits),
    )
    .await?;

    // let bad_bits_handle = match use_denylist {
    //     true => Some(bad_bits::bad_bits_update_handler(bad_bits)),
    //     false => None,
    // };
    // let metrics_handle =
    //     iroh_metrics::MetricsHandle::from_registry_with_tracer(metrics_config, prom_registry)
    //         .await
    //         .expect("failed to initialize metrics");
    let server = handler.server();
    println!("listening on {}", server.local_addr());
    let core_task = tokio::spawn(async move {
        server.await.unwrap();
    });

    iroh_util::block_until_sigint().await;
    core_task.abort();

    // metrics_handle.shutdown();
    // if let Some(handle) = bad_bits_handle {
    //     handle.abort();
    // }

    // match guard.report().build() {
    //     Ok(report) => {
    //         let mut file = File::create("profile.pb").unwrap();
    //         let profile = report.pprof().unwrap();

    //         let mut content = Vec::new();
    //         profile.encode(&mut content).unwrap();
    //         file.write_all(&content).unwrap();

    //         // println!("report: {:?}", &report);
    //     }
    //     Err(_) => {
    //         println!("failed to generate profile");
    //     }
    // };
    Ok(())
}
