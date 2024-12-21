use super::handlers;
use crate::{
    consensus::{self, api::ConsensusApi},
    p2p::P2pApi,
};
use jsonrpsee::{
    server::{RpcModule, Server},
    types::ErrorObjectOwned,
    IntoResponse, ResponsePayload,
};
use serde::Serialize;
use std::net::Ipv4Addr;
use tokio::select;
use tracing::{error, info};

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

/// Configuration details for the RPC task.
#[derive(Debug, Clone)]
pub struct Config {
    pub bind_addr: Ipv4Addr,
    pub bind_port: u16,
    pub search_port: bool,
}

/// Setup a new RPC server and run the task
pub fn start(config: Config, consensus_api: ConsensusApi, p2p_api: P2pApi) {
    // Spawn a task to run the task
    let task = Task::new(config, consensus_api, p2p_api);
    tokio::spawn(task.task_fn());
}

/// Runtime state for the RPC server task
#[derive(Clone)]
pub struct Task {
    pub config: Config,
    pub bind_addr: Ipv4Addr,
    pub bind_port: u16,
    pub consensus_api: ConsensusApi,
    pub p2p_api: P2pApi,
}

impl Task {
    fn new(config: Config, consensus_api: ConsensusApi, p2p_api: P2pApi) -> Task {
        // Instantiate the task
        Task {
            bind_addr: config.bind_addr,
            bind_port: config.bind_port,
            config,
            consensus_api,
            p2p_api,
        }
    }

    /// Return the RPC server address
    fn address(&self) -> String {
        format!("{}:{}", self.bind_addr, self.bind_port)
    }

    /// Run the RPC processing loop
    async fn task_fn(mut self) {
        // Build the RPC server, and optionally scan for an open port to bind
        let server = loop {
            match Server::builder().build(self.address()).await {
                Ok(server) => break server,
                Err(e) => {
                    if self.config.search_port && self.bind_port < u16::MAX {
                        self.bind_port += 1;
                    } else {
                        error!("Failed to start RPC: {e}");
                        return;
                    }
                }
            }
        };
        let mut module = RpcModule::new(self.clone());
        handlers::register_consensus_api(&mut module);
        let addr = server.local_addr().unwrap();
        info!("JSON RPC listening at {}", addr);

        // Run the server indefinitely
        let _hdl = server.start(module);

        let mut consensus_events = self.consensus_api.subscribe_events();
        loop {
            select! {
                _event = consensus_events.recv() => {},
            }
        }
    }
}
