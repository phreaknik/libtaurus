use clap::{arg, command, ArgMatches};
use libtaurus::sequencer;
pub use libtaurus::{
    consensus::{self, GenesisConfig, Vertex, VertexHash},
    hash::Hash,
    p2p, params,
};
use tracing_subscriber::{fmt::time::UtcTime, EnvFilter, FmtSubscriber};

#[tokio::main]
async fn main() {
    // Parse CLI args
    let args = parse_cli_args();

    // Set up a subscriber to capture logs
    setup_logger(&args);

    // Build config from args
    let cfg = build_cfg(&args);

    // Start the sequencer process
    sequencer::start(cfg);
}

/// Parse CLI args
fn parse_cli_args() -> ArgMatches {
    command!() // initialize CLI with details from cargo.toml
        .about("Start taurus sequencer service")
        .arg(
            arg!(-L --log_level <LEVEL> "Set log level (error, warn, info, debug, trace)")
                .required(false)
                .default_value("info"),
        )
        .get_matches()
}

/// Set up logger
fn setup_logger<'a>(args: &'a ArgMatches) {
    let subscriber = FmtSubscriber::builder()
        .with_env_filter(EnvFilter::new(args.get_one::<String>("log_level").unwrap()))
        .with_timer(UtcTime::rfc_3339())
        .finish();
    tracing::subscriber::set_global_default(subscriber).expect("failed to start logger");
}

/// Build application config from parsed CLI args
fn build_cfg(_args: &ArgMatches) -> sequencer::Config {
    sequencer::Config {}
}
