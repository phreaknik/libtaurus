use clap::{arg, command, ArgMatches, Command};
use cordelia_p2p::peer_db::PeerDB;
use etcetera::{base_strategy::choose_native_strategy, BaseStrategy};
use futures::{future::select, pin_mut};
use std::path::PathBuf;
use tokio::{self, sync::mpsc};
use tracing::{error, info};
use tracing_subscriber::EnvFilter;

/// Application configuration details
struct Config {
    /// P2P client configuration
    p2p_cfg: cordelia_p2p::Config,
    /// Core configuration
    core_cfg: cordelia_core::Config,
}

/// Main cordelia CLI application
#[tokio::main]
async fn main() {
    // Parse CLI arguments
    match parse_cli_args().subcommand() {
        Some(("run", sub_args)) => run_cmd(sub_args).await,
        Some(("list-peers", sub_args)) => list_peers_cmd(sub_args),
        _ => unreachable!("Exausted list of subcommands and subcommand_requred prevents 'None'"),
    }
}

/// Command to start node and connect to the network
async fn run_cmd(args: &ArgMatches) {
    // Set up a subscriber to capture logs
    setup_logger(&args);

    // Build config from args
    let cfg = build_cfg(&args);

    // Set up communication channels
    let (send_to_core, recv_from_p2p) = mpsc::unbounded_channel();
    let (send_to_p2p, recv_from_core) = mpsc::unbounded_channel();

    // Start the P2P client
    info!("Starting p2p...");
    let p2p = cordelia_p2p::run(&cfg.p2p_cfg, recv_from_core, send_to_core);

    // Start core backend
    info!("Starting core...");
    let core = cordelia_core::run(&cfg.core_cfg, recv_from_p2p, send_to_p2p);

    // Run all processes
    pin_mut!(p2p, core);
    let _ = select(p2p, core).await;
}

/// Command to list peers
fn list_peers_cmd(args: &ArgMatches) {
    // Set up a subscriber to capture logs
    setup_logger(&args);

    // Open the peer database
    let peer_dir = parse_data_dir(args).join("p2p/peer_db/");
    match PeerDB::open(&peer_dir) {
        Ok(db) => {
            let _ = db.print_peers(args.get_one("max").map(|x| *x));
        }
        Err(e) => {
            error!("Failed to open peer database: {e}");
        }
    }
}

/// Parse CLI args
fn parse_cli_args() -> ArgMatches {
    command!() // initialize CLI with details from cargo.toml
        .subcommand(
            Command::new("run")
                .about("Connect to the p2p network and join consensus")
                .arg(arg!(-v --verbosity ... "Increase verbosity level").required(false))
                .arg(arg!(-d --data_dir <PATH> "Specify data directory").required(false))
                .arg(
                    arg!(--bootnode <MULTIADDR> "Specify boot node to connect to").required(false),
                ),
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
    let data_dir = parse_data_dir(args);
    Config {
        p2p_cfg: build_p2p_cfg(data_dir.join("p2p/"), args),
        core_cfg: cordelia_core::Config {},
    }
}

/// Build ['cordelia-p2p'] config from parsed CLI args
fn build_p2p_cfg(p2p_data_dir: PathBuf, args: &ArgMatches) -> cordelia_p2p::Config {
    let boot_nodes = match args.get_one::<String>("bootnode") {
        Some(v) => vec![v.parse().expect("failed to parse bootnode address")],
        _ => Vec::new(),
    };
    cordelia_p2p::Config {
        data_dir: p2p_data_dir,
        boot_nodes,
    }
}
