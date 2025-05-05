use super::RpcError;
use crate::{consensus::api::ConsensusApi, Vertex};
use jsonrpsee::RpcModule;
use std::sync::Arc;

/// Register RPC method handlers
pub(crate) fn register_consensus_api(module: &mut RpcModule<ConsensusApi>) {
    module
        .register_async_method("get_frontier", |_params, ctx, _| async move {
            println!(":::: get_frontier");
            Ok::<_, RpcError>(ctx.get_frontier().await?)
        })
        .unwrap();
    module
        .register_async_method("get_frontier_meta", |_params, ctx, _| async move {
            Ok::<_, RpcError>(ctx.get_frontier_meta().await?)
        })
        .unwrap();
    module
        .register_async_method("submit_vertex", |params, ctx, _| async move {
            let vertex = Arc::new(params.one::<Vertex>().map_err(|_| RpcError::BadArg)?);
            Ok::<_, RpcError>(ctx.insert_vertex(&vertex).await?)
        })
        .unwrap();
}
