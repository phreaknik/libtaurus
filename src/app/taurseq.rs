mod util;

use clap::{arg, command, ArgMatches};
use libtaurus::sequencer;
pub use libtaurus::{
    consensus::{self, GenesisConfig, Vertex, VertexHash},
    hash::Hash,
    p2p,
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
        .arg(arg!(-u --rpchost <URL> "Host address to reach consensus RPC").required(true))
        .arg(
            arg!(--delay <LEVEL> "Delay (in milliseconds) before producing each new vertex")
                .required(false)
                .default_value("1000")
                .value_parser(clap::value_parser!(u64)),
        )
        .arg(
            arg!(--loglevel <LEVEL> "Set log level")
                .required(false)
                .default_value("info")
                .value_parser(["info", "debug", "trace"]),
        )
        .get_matches()
}

/// Build application config from parsed CLI args
fn build_cfg(args: &ArgMatches) -> sequencer::Config {
    sequencer::Config {
        rpc_url: args.get_one::<String>("rpchost").unwrap().clone(),
        delay_millis: *args.get_one("delay").unwrap(),
    }
}
