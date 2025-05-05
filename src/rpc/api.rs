use super::{RpcContext, RpcError};
use crate::{consensus, Vertex, VertexHash, WireFormat};
use futures::channel::oneshot;
use jsonrpsee::RpcModule;
use serde::{Deserialize, Serialize};
use std::{sync::Arc, time::Duration};
use tokio::time::timeout;

#[derive(Clone, Serialize, Deserialize)]
pub struct VertexMeta {
    pub hash: VertexHash,
    pub height: u64,
}

#[derive(Clone, Serialize, Deserialize)]
pub struct FrontierResponse {
    pub frontier_meta: Vec<VertexMeta>,
}

/// Register RPC method handlers
pub(crate) fn register_handlers(module: &mut RpcModule<RpcContext>) {
    module
        .register_async_method("get_frontier", |_params, ctx, _| async move {
            let (sender, resp) = oneshot::channel();
            ctx.consensus_action_ch
                .send(consensus::Action::GetAcceptedFrontier { result_ch: sender })
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

    module
        .register_async_method("submit_vertex", |params, ctx, _| async move {
            let vertex = Arc::new(params.one::<Vertex>().map_err(|_| RpcError::BadArg)?);
            let (sender, resp) = oneshot::channel();
            ctx.consensus_action_ch
                .send(consensus::Action::SubmitVertex {
                    vertex,
                    result_ch: sender,
                })
                .map_err(|_| RpcError::Unknown)?;
            let result = match timeout(Duration::from_secs(60), resp)
                .await
                .map_err(|_| RpcError::Busy)?
                .map_err(|_| RpcError::Unknown)?
            {
                Ok(_) => Ok(()),
                _ => Err(RpcError::Unknown),
            }?;
            Ok::<_, RpcError>(result)
        })
        .unwrap();
}
