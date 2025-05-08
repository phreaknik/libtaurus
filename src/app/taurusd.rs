mod util;

use clap::{arg, command, ArgMatches};
use etcetera::{base_strategy::choose_native_strategy, BaseStrategy};
use libp2p::{identity::Keypair, multiaddr::Protocol, Multiaddr};
use libtaurus::{consensus::dag, rpc};
pub use libtaurus::{
    consensus::{self, GenesisConfig, Vertex, VertexHash},
    hash::Hash,
    p2p, params,
};
use rand::{distributions::Alphanumeric, Rng};
use std::{char, env, fs, net::Ipv4Addr, path::PathBuf};
use tokio::select;
use tracing::error;

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
        .arg(
            arg!(--bootpeer <MULTIADDR> "Specify a boot peer to connect to")
                .required(false)
                .value_parser(clap::value_parser!(Multiaddr)),
        )
        .arg(
            arg!(-d --datadir <PATH> "Specify data directory")
                .required(false)
                .conflicts_with("tmpdir")
                .value_parser(clap::value_parser!(PathBuf)),
        )
        .arg(
            arg!(--tmpdir "Use a temporary data directory. Useful for testing.")
                .conflicts_with("datadir")
                .required(false),
        )
        .arg(
            arg!(--bind <ADDR> "Address to bind for P2P connections")
                .required(false)
                .default_value("0.0.0.0")
                .value_parser(clap::value_parser!(Ipv4Addr)),
        )
        .arg(
            arg!(--port <PORT> "Port number to accept P2P connections")
                .required(false)
                .default_value("9047")
                .value_parser(clap::value_parser!(u16)),
        )
        .arg(
            arg!(--rpcbind <ADDR> "Address to bind for JSON RPC")
                .required(false)
                .default_value("127.0.0.1")
                .value_parser(clap::value_parser!(Ipv4Addr)),
        )
        .arg(
            arg!(--rpcport <PORT> "Port number to accept JSON RPC http/ws connections")
                .required(false)
                .default_value("9048")
                .value_parser(clap::value_parser!(u16)),
        )
        .arg(
            arg!(--rpcportsearch "Increment RPC port number until an available port is found")
                .required(false),
        )
        .arg(
            arg!(--loglevel <LEVEL> "Set log level")
                .required(false)
                .default_value("info")
                .value_parser(["info", "debug", "trace"]),
        )
        .get_matches()
}

/// Build app data directory
fn app_data_dir(args: &ArgMatches) -> PathBuf {
    args.get_one("datadir").cloned().unwrap_or_else(|| {
        choose_native_strategy()
            .map(|strat| {
                if args.get_flag("tmpdir") {
                    env::temp_dir().join("taurus/").join(
                        rand::thread_rng()
                            .sample_iter(&Alphanumeric)
                            .take(8)
                            .map(char::from)
                            .collect::<String>(),
                    )
                } else {
                    strat.data_dir().join("taurus/")
                }
            })
            .unwrap()
    })
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
    let datadir = app_data_dir(args).join("p2p/");
    let identity_key = get_peer_identity_key(&datadir);
    p2p::Config {
        datadir,
        identity_key,
        boot_peers: args
            .get_many("bootpeer")
            .map(|p| p.cloned().collect())
            .unwrap_or(Vec::new()),
        listen_addr: Multiaddr::empty()
            .with(Protocol::Ip4(*args.get_one("bind").unwrap()))
            .with(Protocol::Udp(*args.get_one("port").unwrap()))
            .with(Protocol::QuicV1),
    }
}

/// Build consensus [`consensus::Config`] from parsed CLI args
fn build_consensus_cfg(args: &ArgMatches) -> consensus::Config {
    consensus::Config {
        datadir: app_data_dir(args).join("consensus/"),
        genesis: GenesisConfig {},
        dag: dag::Config::default(),
    }
}

/// Build RPC [`rpc::Config`] from parsed CLI args
fn build_rpc_cfg(args: &ArgMatches) -> rpc::Config {
    rpc::Config {
        bind_addr: *args.get_one("rpcbind").unwrap(),
        bind_port: *args.get_one("rpcport").unwrap(),
        search_port: args.get_flag("rpcportsearch"),
    }
}

fn get_peer_identity_key(datadir: &PathBuf) -> Keypair {
    let keypath = datadir.join("identity_key");
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
