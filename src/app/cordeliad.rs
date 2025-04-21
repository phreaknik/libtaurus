use chrono::DateTime;
use clap::{arg, command, ArgMatches, Command};
pub use cordelia::{
    consensus::{self, avalanche, Block, BlockHash, GenesisConfig, Vertex, VertexHash},
    hash::Hash,
    http, miner,
    p2p::{self, PeerDatabase},
    params,
};
use etcetera::{base_strategy::choose_native_strategy, BaseStrategy};
use libp2p::identity::Keypair;
use std::{fs, path::PathBuf};
use time::macros::format_description;
use tokio::{self, select};
use tracing::{error, trace};
use tracing_subscriber::{fmt::time::UtcTime, EnvFilter};

/// File name of the stored identity_key
const IDENTITY_KEY_FILE: &str = "identity_key";

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
                .arg(
                    arg!(--mining_threads <NUMBER> "Number of threads for mining")
                        .required(false)
                        .default_value("0"),
                )
                .arg(arg!(-d --data_dir <PATH> "Specify data directory").required(false))
                .arg(arg!(--nohttp "Disable HTTP server").required(false))
                .arg(
                    arg!(--waitlist_cap <CAP> "Set the capacity of the DAG waitlist")
                        .required(false)
                        .default_value("10"),
                )
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
    let debug_verbosity = args.get_count("verbosity") > 0;
    let logger = tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::from_default_env().add_directive(
            match args.get_count("verbosity") {
                1 => "cordelia=debug".parse().unwrap(),
                2 => "cordelia=trace".parse().unwrap(),
                _ => "cordelia=info".parse().unwrap(),
            },
        ))
        .with_target(debug_verbosity);
    if debug_verbosity {
        logger.with_timer(UtcTime::rfc_3339()).init();
    } else {
        logger
            .with_timer(UtcTime::new(format_description!(
                "[hour]:[minute]:[second]"
            )))
            .init();
    }
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

/// Build P2P [`p2p::Config`] from parsed CLI args
fn build_p2p_cfg(args: &ArgMatches) -> p2p::Config {
    // Read the peer identity key if it exists, or create a new one.
    let data_dir = parse_data_dir(args);
    let identity_key = get_peer_identity_key(&data_dir);
    let boot_nodes = match args.try_get_one::<String>("bootnode") {
        Ok(Some(v)) => vec![v.parse().expect("failed to parse bootnode address")],
        _ => Vec::new(),
    };
    p2p::Config {
        data_dir: data_dir.join("p2p/"),
        boot_nodes,
        identity_key,
    }
}

/// Build P2P [`p2p::Config`] from parsed CLI args
fn build_consensus_cfg(args: &ArgMatches) -> consensus::Config {
    let data_dir = parse_data_dir(args).join("consensus/");
    let genesis = GenesisConfig {
        difficulty: params::MIN_DIFFICULTY,
        time: DateTime::parse_from_rfc2822("Wed, 18 Feb 2015 23:16:09 GMT")
            .unwrap()
            .into(),
    };
    consensus::Config {
        data_dir: data_dir.clone(),
        avalanche: avalanche::Config {
            data_dir,
            genesis: genesis.to_vertex(),
            waitlist_cap: args
                .get_one::<String>("waitlist_cap")
                .map(|v| {
                    v.parse()
                        .expect("waitlist_cap must be non-zero positive integer")
                })
                .unwrap(),
        },
        genesis,
    }
}

/// Build miner [`miner::Config`] from parsed CLI args
fn build_miner_cfg(args: &ArgMatches) -> miner::Config {
    let data_dir = parse_data_dir(args);
    let identity_key = get_peer_identity_key(&data_dir);
    miner::Config {
        num_threads: args
            .get_one::<String>("mining_threads")
            .map(|v| {
                v.parse()
                    .expect("waitlist_cap must be non-zero positive integer")
            })
            .unwrap(),
        identity_key,
    }
}

fn get_peer_identity_key(data_dir: &PathBuf) -> Keypair {
    let keypath = data_dir.join(IDENTITY_KEY_FILE);
    let _ = fs::create_dir_all(&data_dir);
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
