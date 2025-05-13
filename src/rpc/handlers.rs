use super::task::RpcError;
use crate::{rpc, Vertex};
use jsonrpsee::RpcModule;
use std::sync::Arc;

/// Register RPC method handlers
pub(crate) fn register_consensus_api(module: &mut RpcModule<rpc::task::Task>) {
    module
        .register_async_method("get_frontier", |_params, ctx, _| async move {
            Ok::<_, RpcError>(ctx.consensus_api.get_frontier().await?)
        })
        .unwrap();
    module
        .register_async_method("get_frontier_meta", |_params, ctx, _| async move {
            Ok::<_, RpcError>(ctx.consensus_api.get_frontier_meta().await?)
        })
        .unwrap();
    module
        .register_async_method("submit_vertex", |params, ctx, _| async move {
            let vertex = Arc::new(params.one::<Vertex>().map_err(|_| RpcError::BadArg)?);
            ctx.p2p_api
                .submit_vertex(&vertex)
                .map_err(|_| RpcError::Unknown)?;
            Ok::<_, RpcError>(ctx.consensus_api.submit_vertex(&vertex).await?)
        })
        .unwrap();
}
