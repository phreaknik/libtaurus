use crate::consensus::SerdeHash;
use serde_derive::{Deserialize, Serialize};

/// Top level message type for RPC messages
#[derive(Debug, Clone)]
pub enum Message {
    Request(Request),
    Response(Response),
}

/// Message type defining the peer RPC request messages
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum Request {
    GetBlock(u64, SerdeHash),
}

/// Message type defining the peer RPC response messages
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum Response {}
