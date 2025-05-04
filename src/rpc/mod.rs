use crate::consensus;
use std::result;
use tokio::{
    select,
    sync::{broadcast, mpsc::UnboundedSender},
};
use tracing::info;

#[derive(thiserror::Error, Debug)]
pub enum Error {}
type Result<T> = result::Result<T, Error>;

/// Configuration details for the RPC process.
#[derive(Debug, Clone)]
pub struct Config {
    pub port: u16,
}

/// Setup a new RPC server and run the process
pub fn start(
    config: Config,
    consensus_action_ch: UnboundedSender<consensus::Action>,
    consensus_event_ch: broadcast::Receiver<consensus::Event>,
) {
    // Spawn a task to execute the runtime
    let runtime = Runtime::new(config, consensus_action_ch, consensus_event_ch)
        .expect("Failed to start RPC server");
    tokio::spawn(runtime.run());
}

/// Runtime state for the RPC server
pub struct Runtime {
    _config: Config,
    _consensus_action_ch: UnboundedSender<consensus::Action>,
    consensus_event_ch: broadcast::Receiver<consensus::Event>,
}

impl Runtime {
    fn new(
        config: Config,
        consensus_action_ch: UnboundedSender<consensus::Action>,
        consensus_event_ch: broadcast::Receiver<consensus::Event>,
    ) -> Result<Runtime> {
        info!("Starting RPC server on port {}", config.port);

        // Instantiate the runtime
        Ok(Runtime {
            _config: config.clone(),
            _consensus_action_ch: consensus_action_ch,
            consensus_event_ch,
        })
    }

    // Run the RPC processing loop
    async fn run(mut self) {
        loop {
            select! {
                // Handle consensus events
                _event = self.consensus_event_ch.recv() => {
                    todo!()
                },
            }
        }
    }
}
