mod util;

use clap::{arg, command, ArgMatches};
use libtaurus::sequencer;
pub use libtaurus::{
    consensus::{self, GenesisConfig, Vertex, VertexHash},
    hash::Hash,
    p2p, params,
};

fn main() {
    // Parse CLI args
    let args = parse_cli_args();

    // Set up a subscriber to capture logs
    util::setup_logger(&args);

    // Build config from args
    let cfg = build_cfg(&args);

    // Start the sequencer process
    sequencer::start(cfg);
}

/// Parse CLI args
fn parse_cli_args() -> ArgMatches {
    command!() // initialize CLI with details from cargo.toml
        .about("Start taurus sequencer service")
        .arg(arg!(-u --node_url <URL> "URL to reach consensus node").required(true))
        .arg(
            arg!(--loglevel <LEVEL> "Set log level (error, warn, info, debug, trace)")
                .required(false)
                .default_value("info"),
        )
        .get_matches()
}

/// Build application config from parsed CLI args
fn build_cfg(args: &ArgMatches) -> sequencer::Config {
    sequencer::Config {
        rpc_url: args.get_one::<String>("node_url").unwrap().clone(),
    }
}
