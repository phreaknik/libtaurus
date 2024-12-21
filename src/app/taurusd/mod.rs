mod cli;
mod handler;
mod util;

use cli::parse_cli_args;
use libtaurus::{consensus, p2p, rpc};
use tokio::select;
use tracing::{error, info};
use util::{build_consensus_cfg, build_handler_cfg, build_p2p_cfg, build_rpc_cfg};

#[derive(thiserror::Error, Debug)]
enum Error {
    #[error(transparent)]
    P2pError(#[from] crate::p2p::task::Error),
    #[error(transparent)]
    ConsensusError(#[from] crate::consensus::task::Error),
}

/// Main taurus daemon
#[tokio::main]
async fn main() {
    // Parse CLI args
    let args = parse_cli_args();

    // Set up a subscriber to capture logs
    libtaurus::app::util::setup_logger(&args);
    info!(
        "Using data directory {}",
        util::app_data_dir(&args).to_str().unwrap()
    );

    // Start the P2P process
    let p2p_api = p2p::start(build_p2p_cfg(&args)).unwrap();

    // Start the consensus process
    let consensus_api = consensus::start(build_consensus_cfg(&args), p2p_api.clone()).unwrap();
    let mut consensus_events = consensus_api.subscribe_events();

    // Start the RPC server
    rpc::start(build_rpc_cfg(&args), consensus_api.clone(), p2p_api.clone());

    // Start the event handler
    handler::Handler::new(build_handler_cfg(&args), p2p_api, consensus_api).start();

    // Handle events
    loop {
        select! {
            // Handle P2P events
            event = consensus_events.recv() => {
                match event {
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
