#![feature(iterator_try_collect)]

mod consensus;
mod http;
mod miner;
mod p2p;
mod params;
mod randomx;
mod util;

use clap::{arg, command, ArgMatches, Command};
use consensus::GenesisConfig;
pub use consensus::{Block, Header};
use etcetera::{base_strategy::choose_native_strategy, BaseStrategy};
use p2p::PeerDatabase;
use std::path::PathBuf;
use tokio::{self, select};
use tracing::{error, trace};
use tracing_subscriber::EnvFilter;

/// Application configuration details
struct Config {
    /// P2P client configuration
    pub p2p: p2p::Config,
    /// Consensus configuration
    pub consensus: consensus::Config,
    /// Http server configuration
    pub http: http::Config,
    /// Miner configuration
    pub miner: miner::Config,
}

/// Main cordelia CLI application
#[tokio::main]
async fn main() {
    // Parse CLI arguments
    match parse_cli_args().subcommand() {
        Some(("run", sub_args)) => cmd_run(sub_args).await,
        Some(("list-peers", sub_args)) => cmd_list_peers(sub_args),
        _ => unreachable!("Exausted list of subcommands and subcommand_requred prevents 'None'"),
    }
}

/// Error type for cordelia-p2p errors
#[derive(thiserror::Error, Debug)]
enum Error {
    #[error(transparent)]
    P2pError(#[from] crate::p2p::Error),
    #[error(transparent)]
    ConsensusError(#[from] crate::consensus::Error),
    #[error(transparent)]
    HttpError(#[from] crate::http::Error),
}

/// Command to start node and connect to the network
async fn cmd_run(args: &ArgMatches) {
    // Set up a subscriber to capture logs
    setup_logger(&args);

    // Build config from args
    let cfg = build_cfg(&args);

    // Build a list of futures to be executed
    let (p2p_action_ch, p2p_event_sender) = p2p::start(cfg.p2p);
    let (consensus_action_ch, consensus_event_sender) = consensus::start(
        cfg.consensus,
        p2p_action_ch.clone(),
        p2p_event_sender.subscribe(),
    );
    if !args.get_flag("nohttp") {
        http::start(cfg.http, p2p_action_ch, consensus_action_ch.clone());
    }
    miner::start(
        cfg.miner,
        consensus_action_ch,
        consensus_event_sender.subscribe(),
    );
    let mut p2p_event_ch = p2p_event_sender.subscribe();
    let mut consensus_event_ch = consensus_event_sender.subscribe();
    loop {
        select! {
            event = p2p_event_ch.recv() => {
                trace!("p2p event: {event:?}");
            }
            event = consensus_event_ch.recv() => {
                trace!("consensus event: {event:?}");
            }
        }
    }
}

/// Command to list peers
fn cmd_list_peers(args: &ArgMatches) {
    // Set up a subscriber to capture logs
    setup_logger(&args);

    // Build config from args
    let cfg = build_cfg(&args);

    // Open the peer database
    let peer_db = match PeerDatabase::open(&cfg.p2p.data_dir.join(p2p::DATABASE_DIR), false) {
        Ok(db) => db,
        Err(e) => {
            error!("Unable to open peer database: {e}");
            return;
        }
    };

    // Iterate the entries and print them out
    let rtxn = peer_db.env.read_txn().unwrap();
    for entry in peer_db.db.iter(&rtxn).unwrap() {
        match entry {
            Ok((peer, info)) => println!("{}:{info}", peer.as_peerid()),
            Err(e) => error!("error getting peer info: {e}"),
        }
    }
}

/// Parse CLI args
fn parse_cli_args() -> ArgMatches {
    command!() // initialize CLI with details from cargo.toml
        .subcommand(
            Command::new("run")
                .about("Connect to the p2p network and join consensus")
                .arg(arg!(--bootnode <MULTIADDR> "Specify boot node to connect to").required(false))
                .arg(arg!(--mining_threads <NUMBER> "Number of threads for mining").required(false))
                .arg(arg!(-d --data_dir <PATH> "Specify data directory").required(false))
                .arg(arg!(--nohttp "Disable HTTP server").required(false))
                .arg(arg!(-v --verbosity ... "Increase verbosity level").required(false)),
        )
        .subcommand(
            Command::new("list-peers")
                .about("Lists all peers saved in the peer database")
                .arg(arg!(-v --verbosity ... "Increase verbosity level").required(false))
                .arg(arg!(-d --data_dir <PATH> "Specify data directory").required(false))
                .arg(arg!(-n --max <COUNT> "Max number of peers to list").required(false)),
        )
        .subcommand_required(true)
        .get_matches()
}

/// Set up logger
fn setup_logger<'a>(args: &'a ArgMatches) {
    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::from_default_env().add_directive(
            match args.get_count("verbosity") {
                1 => "cordelia=debug".parse().unwrap(),
                2 => "cordelia=trace".parse().unwrap(),
                _ => "cordelia=info".parse().unwrap(),
            },
        ))
        .init();
}

/// Determine system directories for the application to use
fn parse_data_dir(args: &ArgMatches) -> PathBuf {
    let app_dirs = choose_native_strategy().expect("failed to build application directories");
    args.get_one::<String>("data_dir")
        .map(|s| PathBuf::from(s))
        .unwrap_or(app_dirs.data_dir().join("cordelia/"))
}

/// Build application config from parsed CLI args
fn build_cfg(args: &ArgMatches) -> Config {
    Config {
        p2p: build_p2p_cfg(args),
        consensus: build_consensus_cfg(args),
        http: http::Config {},
        miner: build_miner_cfg(args),
    }
}

/// Build P2P ['p2p::Config'] from parsed CLI args
fn build_p2p_cfg(args: &ArgMatches) -> p2p::Config {
    let data_dir = parse_data_dir(args).join("p2p/");
    let boot_nodes = match args.try_get_one::<String>("bootnode") {
        Ok(Some(v)) => vec![v.parse().expect("failed to parse bootnode address")],
        _ => Vec::new(),
    };
    p2p::Config {
        data_dir,
        boot_nodes,
    }
}

/// Build P2P ['p2p::Config'] from parsed CLI args
fn build_consensus_cfg(args: &ArgMatches) -> consensus::Config {
    let data_dir = parse_data_dir(args).join("consensus/");
    consensus::Config {
        data_dir,
        genesis: GenesisConfig {
            difficulty: params::GENESIS_DIFFICULTY,
            time: params::GENESIS_TIMESTAMP,
        },
    }
}

/// Build miner ['miner::Config'] from parsed CLI args
fn build_miner_cfg(args: &ArgMatches) -> miner::Config {
    miner::Config {
        num_threads: args
            .try_get_one::<String>("mining_threads")
            .unwrap_or(Some(&"0".to_string()))
            .map(|v| v.parse().expect("failed to parse miner num-threads")),
    }
}
