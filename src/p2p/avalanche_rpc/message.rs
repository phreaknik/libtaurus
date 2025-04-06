use crate::consensus::{Block, SerdeHash};
use serde_derive::{Deserialize, Serialize};

/// Message type defining the peer RPC request messages
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum Request {
    GetBlock(u64, SerdeHash),
}

/// Message type defining the peer RPC response messages
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum Response {
    NotFound,
    Block(Block),
}
