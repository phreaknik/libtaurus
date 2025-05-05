pub mod api;

use crate::consensus::{self, api::ConsensusApi};
use jsonrpsee::{
    server::{RpcModule, Server},
    types::ErrorObjectOwned,
    IntoResponse, ResponsePayload,
};
use serde::Serialize;
use std::result;
use tokio::{
    select,
    sync::{broadcast, mpsc},
};
use tracing::info;

#[derive(Debug, Clone)]
pub enum Error {}
type Result<T> = result::Result<T, Error>;

#[derive(Debug, Clone, Serialize)]
pub enum RpcError {
    Unknown,
    Busy,
    BadArg,
}

impl From<consensus::api::Error> for RpcError {
    fn from(error: consensus::api::Error) -> Self {
        match error {
            consensus::api::Error::TimerElapsed(_) => RpcError::Busy,
            _ => RpcError::Unknown,
        }
    }
}

impl Into<ErrorObjectOwned> for RpcError {
    fn into(self) -> ErrorObjectOwned {
        let code = match &self {
            Self::Unknown => -32000,
            Self::Busy => -32001,
            Self::BadArg => -32002,
        };
        let message = match &self {
            Self::Unknown => "unknown error",
            Self::Busy => "server is busy",
            Self::BadArg => "one or more arguments is incorrect",
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

// Persistent context for the JSON RPC
pub struct RpcContext {
    consensus_action_ch: mpsc::UnboundedSender<consensus::Action>,
}

/// Setup a new RPC server and run the process
pub fn start(
    config: Config,
    consensus_api: ConsensusApi,
    consensus_action_ch: mpsc::UnboundedSender<consensus::Action>,
    consensus_event_ch: broadcast::Receiver<consensus::Event>,
) {
    // Spawn a task to execute the runtime
    let runtime = Runtime::new(
        config,
        consensus_api,
        consensus_action_ch,
        consensus_event_ch,
    )
    .expect("Failed to start RPC server");
    tokio::spawn(runtime.run());
}

/// Runtime state for the RPC server
pub struct Runtime {
    config: Config,
    consensus_api: ConsensusApi,
    consensus_action_ch: mpsc::UnboundedSender<consensus::Action>,
    consensus_event_ch: broadcast::Receiver<consensus::Event>,
}

impl Runtime {
    fn new(
        config: Config,
        consensus_api: ConsensusApi,
        consensus_action_ch: mpsc::UnboundedSender<consensus::Action>,
        consensus_event_ch: broadcast::Receiver<consensus::Event>,
    ) -> Result<Runtime> {
        // Instantiate the runtime
        Ok(Runtime {
            config,
            consensus_api,
            consensus_action_ch,
            consensus_event_ch,
        })
    }

    // Run the RPC processing loop
    async fn run(mut self) {
        // Build the RPC server
        let server = Server::builder()
            .build(self.config.bind_addr)
            .await
            .unwrap();
        let mut module = RpcModule::new(self.consensus_api);
        api::register_consensus_api(&mut module);
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
