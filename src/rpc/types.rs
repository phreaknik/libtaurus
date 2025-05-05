use crate::VertexHash;
use serde::{Deserialize, Serialize};

#[derive(Clone, Serialize, Deserialize)]
pub struct VertexMeta {
    pub hash: VertexHash,
    pub height: u64,
}

#[derive(Clone, Serialize, Deserialize)]
pub struct FrontierResponse {
    pub frontier: Vec<VertexMeta>,
}
