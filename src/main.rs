use clap::{arg, command, ArgMatches};
use etcetera::{base_strategy::choose_native_strategy, BaseStrategy};
use std::path::PathBuf;
use tokio;
use tracing::{info, span, Level};
use tracing_subscriber::EnvFilter;

/// Application configuration details
struct Config<'a> {
    /// P2P client configuration
    p2p_cfg: cordelia_p2p::Config<'a>,
}

#[tokio::main]
async fn main() {
    // Parse CLI arguments
    let args = parse_args();

    // Set up a subscriber to capture logs
    setup_logger(&args);

    // Build config from args
    let cfg = build_cfg(&args);

    // Start the P2P client
    info!("Starting p2p...");
    cordelia_p2p::run(&cfg.p2p_cfg).await.unwrap();
}

/// Parse CLI args
fn parse_args() -> ArgMatches {
    command!() // initialize CLI with details from cargo.toml
        .arg(arg!(-v --verbosity ... "Increase verbosity level").required(false))
        .arg(arg!(-d --data_dir <PATH> "Specify data directory").required(false))
        .arg(arg!(--static_peer <MULTIADDR> "Specify static peer to connect to").required(false))
        .get_matches()
}

/// Set up logger
fn setup_logger<'a>(args: &'a ArgMatches) {
    let log_filter = match args.get_count("verbosity") {
        1 => EnvFilter::from_default_env().add_directive("cordelia=debug".parse().unwrap()),
        2 => EnvFilter::from_default_env().add_directive("cordelia=trace".parse().unwrap()),
        _ => EnvFilter::from_default_env().add_directive("cordelia=info".parse().unwrap()),
    };
    tracing_subscriber::fmt().with_env_filter(log_filter).init();
}

/// Build application config from parsed CLI args
fn build_cfg<'a>(args: &'a ArgMatches) -> Config<'a> {
    let app_dirs = choose_native_strategy().expect("failed to build application directories");
    let data_dir = args
        .get_one::<String>("data_dir")
        .map(|s| PathBuf::from(s))
        .unwrap_or(app_dirs.data_dir().join("cordelia/"));
    Config {
        p2p_cfg: build_p2p_cfg(data_dir.join("p2p/"), args),
    }
}

/// Build ['cordelia-p2p'] config from parsed CLI args
fn build_p2p_cfg<'a>(p2p_data_dir: PathBuf, args: &'a ArgMatches) -> cordelia_p2p::Config<'a> {
    cordelia_p2p::Config {
        static_peer: args.get_one::<String>("static_peer").map(move |s| &s[..]),
        data_dir: p2p_data_dir,
    }
}
