pub mod types;

use crate::{consensus, VertexHash, WireFormat};
use futures::channel::oneshot;
use jsonrpsee::{
    server::{RpcModule, Server},
    types::ErrorObjectOwned,
    IntoResponse, ResponsePayload,
};
use serde::Serialize;
use std::{result, time::Duration};
use tokio::{
    select,
    sync::{broadcast, mpsc},
    time::timeout,
};
use tracing::info;
use types::{FrontierResponse, VertexMeta};

#[derive(thiserror::Error, Debug)]
pub enum Error {}
type Result<T> = result::Result<T, Error>;

#[derive(Debug, Clone, Serialize)]
pub enum RpcError {
    Unknown,
    Busy,
}

impl Into<ErrorObjectOwned> for RpcError {
    fn into(self) -> ErrorObjectOwned {
        let code = match &self {
            Self::Unknown => -32000,
            Self::Busy => -32001,
        };
        let message = match &self {
            Self::Unknown => "unknown error",
            Self::Busy => "server is busy",
        };
        let data: Option<serde_json::Value> = match &self {
            _ => None,
        };
        let data = data.map(|val| serde_json::value::to_raw_value(&val).unwrap());

        jsonrpsee::types::ErrorObjectOwned::owned(code, message, data)
    }
}

impl IntoResponse for RpcError {
    type Output = Self;

    fn into_response(self) -> ResponsePayload<'static, Self::Output> {
        ResponsePayload::error(self)
    }
}

/// Configuration details for the RPC process.
#[derive(Debug, Clone)]
pub struct Config {
    pub bind_addr: String,
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
    consensus_action_ch: mpsc::UnboundedSender<consensus::Action>,
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
            consensus_action_ch,
            consensus_event_ch,
        })
    }

    // Run the RPC processing loop
    async fn run(mut self) {
        // Persistent context for the JSON RPC
        struct RpcContext {
            consensus_action_ch: mpsc::UnboundedSender<consensus::Action>,
        }
        let ctx = RpcContext {
            consensus_action_ch: self.consensus_action_ch.clone(),
        };

        // Build the RPC server
        let server = Server::builder()
            .build(self.config.bind_addr)
            .await
            .unwrap();
        let mut module = RpcModule::new(ctx);
        module
            .register_async_method("get_frontier", |_params, ctx, _| async move {
                let (sender, resp) = oneshot::channel();
                ctx.consensus_action_ch
                    .send(consensus::Action::GetAcceptedFrontier(sender))
                    .map_err(|_| RpcError::Unknown)?;
                let frontier = timeout(Duration::from_secs(60), resp)
                    .await
                    .map_err(|_| RpcError::Busy)?
                    .map_err(|_| RpcError::Unknown)?;
                Ok::<_, RpcError>(FrontierResponse {
                    frontier_meta: frontier
                        .iter()
                        .map(|vx| VertexMeta {
                            hash: vx.hash(),
                            height: vx.height,
                        })
                        .collect::<Vec<_>>(),
                })
            })
            .unwrap();
        let addr = server.local_addr().unwrap();
        info!("JSON RPC listening at {}", addr);

        // Start the server
        let _handle = server.start(module);

        // Handle async events
        loop {
            select! {
                // Handle consensus events
                event = self.consensus_event_ch.recv() => {
                    match event {
                        _ => {},
                    }
                },
            }
        }
    }
}
