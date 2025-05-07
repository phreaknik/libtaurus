mod util;

use clap::{arg, command, ArgMatches};
use etcetera::{base_strategy::choose_native_strategy, BaseStrategy};
use libp2p::identity::Keypair;
use libtaurus::{consensus::dag, rpc};
pub use libtaurus::{
    consensus::{self, GenesisConfig, Vertex, VertexHash},
    hash::Hash,
    p2p, params,
};
use std::{fs, path::PathBuf};
use tokio::select;
use tracing::error;

/// File name of the stored identity_key
const IDENTITY_KEY_FILE: &str = "identity_key";

/// Application configuration details
struct Config {
    /// P2P client configuration
    pub p2p: p2p::Config,

    /// Consensus configuration
    pub consensus: consensus::Config,

    /// RPC server configuration
    pub rpc: rpc::Config,
}

#[derive(thiserror::Error, Debug)]
enum Error {
    #[error(transparent)]
    P2pError(#[from] crate::p2p::Error),
    #[error(transparent)]
    ConsensusError(#[from] crate::consensus::Error),
}

/// Main taurus daemon
#[tokio::main]
async fn main() {
    // Parse CLI args
    let args = parse_cli_args();

    // Set up a subscriber to capture logs
    util::setup_logger(&args);

    // Build config from args
    let cfg = build_cfg(&args);

    // Start the P2P process
    let p2p_api = p2p::start(cfg.p2p);

    // Start the consensus process
    let consensus_api = consensus::start(cfg.consensus, p2p_api);
    let mut consensus_events = consensus_api.subscribe_events();

    // Start the RPC server
    rpc::start(cfg.rpc, consensus_api);

    // Handle events
    loop {
        select! {
            // Handle P2P events
            event = consensus_events.recv() => {
                match event {
                    Ok(consensus::Event::Stopped) =>{
                        error!("Consensus stopped.");
                        error!("Shutting down.");
                        // TODO: clean shutdown of all processes
                        return
                    },
                    Err(e) =>{
                        error!("Consensus error: {e}");
                        error!("Shutting down.");
                        return
                    },
                    _ => {},
                }
            },
        }
    }
}

/// Parse CLI args
fn parse_cli_args() -> ArgMatches {
    command!() // initialize CLI with details from cargo.toml
        .about("Start taurusd network client")
        .arg(arg!(--bootpeer <MULTIADDR> "Specify a boot peer to connect to").required(false))
        .arg(arg!(-d --datadir <PATH> "Specify data directory").required(false))
        .arg(
            arg!(--rpcbind <ADDR> "IP address to serve JSON RPC")
                .required(false)
                .default_value("127.0.0.1"),
        )
        .arg(
            arg!(--rpcport <PORT> "Port number to serve JSON RPC")
                .required(false)
                .default_value("8545"),
        )
        .arg(
            arg!(--rpcportsearch "Increment RPC port number until an available port is found")
                .required(false),
        )
        .arg(
            arg!(-L --log_level <LEVEL> "Set log level (error, warn, info, debug, trace)")
                .required(false)
                .default_value("info"),
        )
        .get_matches()
}

/// Determine system directories for the application to use
fn parse_datadir(args: &ArgMatches) -> PathBuf {
    // TODO: look into etcetera AppStrategies. Instead of manually building data/config dirs, setup
    // an app strategy and use the provided API any time you want a file or subdir of the data,
    // config, etc dirs.
    let app_dirs = choose_native_strategy().expect("failed to build application directories");
    args.get_one::<String>("datadir")
        .map(|s| PathBuf::from(s))
        .unwrap_or(app_dirs.data_dir().join("taurus/"))
}

/// Build application config from parsed CLI args
fn build_cfg(args: &ArgMatches) -> Config {
    Config {
        p2p: build_p2p_cfg(args),
        consensus: build_consensus_cfg(args),
        rpc: build_rpc_cfg(args),
    }
}

/// Build P2P [`p2p::Config`] from parsed CLI args
fn build_p2p_cfg(args: &ArgMatches) -> p2p::Config {
    // Read the peer identity key if it exists, or create a new one.
    let datadir = parse_datadir(args);
    let identity_key = get_peer_identity_key(&datadir);
    let boot_peers = match args.try_get_one::<String>("bootpeer") {
        Ok(Some(v)) => vec![v.parse().expect("failed to parse boot peer address")],
        _ => Vec::new(),
    };
    p2p::Config {
        datadir: datadir.join("p2p/"),
        boot_peers,
        identity_key,
    }
}

/// Build consensus [`consensus::Config`] from parsed CLI args
fn build_consensus_cfg(args: &ArgMatches) -> consensus::Config {
    let datadir = parse_datadir(args).join("consensus/");
    let genesis = GenesisConfig {};
    let dag = dag::Config::default();
    consensus::Config {
        datadir: datadir.clone(),
        genesis,
        dag,
    }
}

/// Build RPC [`rpc::Config`] from parsed CLI args
fn build_rpc_cfg(args: &ArgMatches) -> rpc::Config {
    rpc::Config {
        bind_addr: args.get_one::<String>("rpcbind").unwrap().clone(),
        bind_port: args
            .get_one::<String>("rpcport")
            .unwrap()
            .parse::<u16>()
            .unwrap(),
        search_port: args.get_flag("rpcportsearch"),
    }
}

fn get_peer_identity_key(datadir: &PathBuf) -> Keypair {
    let keypath = datadir.join(IDENTITY_KEY_FILE);
    let _ = fs::create_dir_all(&datadir);
    match fs::read(&keypath) {
        Ok(keydata) => {
            Keypair::from_protobuf_encoding(&keydata).expect("Failed to decode keyfile!")
        }
        Err(_) => {
            // Generate a random new key
            let newkey = Keypair::generate_ed25519();
            // Save the key to the file
            fs::write(
                keypath,
                newkey
                    .to_protobuf_encoding()
                    .expect("Failed to encode key to save to keyfile!"),
            )
            .expect("Failed to write to keyfile!");
            newkey
        }
    }
}
