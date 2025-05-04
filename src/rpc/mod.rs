use crate::consensus;
use jsonrpsee::server::{RpcModule, Server};
use std::result;
use tokio::{
    select,
    sync::{broadcast, mpsc},
};
use tracing::{info, trace};

#[derive(thiserror::Error, Debug)]
pub enum Error {}
type Result<T> = result::Result<T, Error>;

/// Configuration details for the RPC process.
#[derive(Debug, Clone)]
pub struct Config {
    pub http_addr: String,
}

/// Setup a new RPC server and run the process
pub fn start(
    config: Config,
    consensus_action_ch: mpsc::UnboundedSender<consensus::Action>,
    consensus_event_ch: broadcast::Receiver<consensus::Event>,
) {
    // Spawn a task to execute the runtime
    let runtime = Runtime::new(config, consensus_action_ch, consensus_event_ch)
        .expect("Failed to start RPC server");
    tokio::spawn(runtime.run());
}

/// Runtime state for the RPC server
pub struct Runtime {
    config: Config,
    _consensus_action_ch: mpsc::UnboundedSender<consensus::Action>,
    consensus_event_ch: broadcast::Receiver<consensus::Event>,
}

impl Runtime {
    fn new(
        config: Config,
        consensus_action_ch: mpsc::UnboundedSender<consensus::Action>,
        consensus_event_ch: broadcast::Receiver<consensus::Event>,
    ) -> Result<Runtime> {
        // Instantiate the runtime
        Ok(Runtime {
            config,
            _consensus_action_ch: consensus_action_ch,
            consensus_event_ch,
        })
    }

    // Run the RPC processing loop
    async fn run(mut self) {
        let server = Server::builder()
            .build(self.config.http_addr)
            .await
            .unwrap();
        let mut module = RpcModule::new(());
        module
            .register_method("say_hello", |params, _, _| {
                format!("Hello, {}", params.one::<String>().unwrap())
            })
            .unwrap();
        let addr = server.local_addr().unwrap();
        info!("HTTP RPC server listening on {}", addr);

        let handle = server.start(module);

        // In this example we don't care about doing shutdown so let's it run forever.
        // You may use the `ServerHandle` to shut it down or manage it yourself.
        tokio::spawn(handle.stopped());

        loop {
            select! {
                // Handle consensus events
                event = self.consensus_event_ch.recv() => {
                    trace!("received consensus event: {event:?}");
                },
            }
        }
    }
}
