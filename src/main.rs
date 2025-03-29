use std::path::PathBuf;

use clap::{arg, command, ArgMatches};
use etcetera::{base_strategy::choose_native_strategy, BaseStrategy};
use tokio;

/// Application configuration details
struct Config<'a> {
    /// P2P client configuration
    p2p_cfg: cordelia_p2p::Config<'a>,
}

#[tokio::main]
async fn main() {
    // Parse CLI arguments
    let args = parse_args();

    // Build config from args
    let cfg = build_cfg(&args);

    // Start the P2P client
    cordelia_p2p::run(&cfg.p2p_cfg).await.unwrap();
}

/// Parse CLI args
fn parse_args() -> ArgMatches {
    command!() // initialize CLI with details from cargo.toml
        .arg(arg!(--data_dir <PATH>).required(false))
        .arg(arg!(--static_peer <MULTIADDR>).required(false))
        .get_matches()
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
