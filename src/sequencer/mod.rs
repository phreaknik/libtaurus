use crate::{consensus, Vertex, VertexHash, WireFormat};
use jsonrpsee::{core::client::ClientT, rpc_params, ws_client::WsClientBuilder};
use std::result;
use tokio::{
    runtime::Runtime,
    select,
    time::{interval, Duration},
};
use tracing::{error, info};

#[derive(thiserror::Error, Debug)]
pub enum Error {
    #[error(transparent)]
    JsonrpseeClient(#[from] jsonrpsee::core::client::Error),
}
type Result<T> = result::Result<T, Error>;

/// Configuration details for the sequencer process.
#[derive(Debug, Clone)]
pub struct Config {
    pub rpc_url: String,
    pub delay_millis: u64,
}

/// Run the consensus process, spawning the task as a new thread. Returns an [`broadcast::Sender`],
/// which can be subscribed to, to receive consensus events from the task.
pub fn start(config: Config) {
    // Build a tokio runtime
    let rt = Runtime::new().unwrap();
    let hdl = rt.handle();

    // Spawn a task to execute the runtime
    let sequencer = Sequencer::new(config).expect("Failed to start sequencer runtime");
    match hdl.block_on(sequencer.run()) {
        Ok(_) => {}
        Err(e) => error!("exited with error: {e}"),
    }
}

/// Runtime state for the consensus process
pub struct Sequencer {
    config: Config,
}

impl Sequencer {
    fn new(config: Config) -> Result<Sequencer> {
        info!("Starting sequencer");

        // Instantiate the runtime
        Ok(Sequencer { config })
    }

    // Run the consensus processing loop
    async fn run(self) -> Result<()> {
        let ws_client = WsClientBuilder::default()
            .build(&self.config.rpc_url)
            .await?;

        let mut timer = interval(Duration::from_millis(self.config.delay_millis));
        loop {
            select! {
                // Handle timer event
                _ = timer.tick() => {
                    // Get the latest frontier
                    let frontier_meta: Vec<consensus::api::VertexMeta> = ws_client.request("get_frontier_meta", rpc_params![]).await?;

                    // Build a new vertex on the frontier
                    let mut proposal = Vertex::empty();
                    proposal.height = 1 + frontier_meta.iter().map(|meta| meta.height).max().unwrap();
                    proposal.parents = frontier_meta.iter().map(|meta| meta.hash).collect();

                    // Submit the vertex
                    let vhash = proposal.hash();
                    let _resp: Vec<VertexHash> = ws_client.request("submit_vertex", rpc_params![proposal]).await?;
                    info!("Submitted a new vertex {}", vhash.to_hex());
                },
            }
        }
    }
}
