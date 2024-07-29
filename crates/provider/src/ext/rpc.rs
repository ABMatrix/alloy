//! This module extends the Ethereum JSON-RPC provider with the Rpc namespace's RPC methods.
use crate::Provider;
use alloy_network::Network;
use alloy_rpc_types::RpcModules;
use alloy_transport::{Transport, TransportResult};

/// The rpc API provides methods to get information about the RPC server itself, such as the enabled
/// namespaces.
#[cfg_attr(target_arch = "wasm32", async_trait::async_trait(?Send))]
#[cfg_attr(not(target_arch = "wasm32"), async_trait::async_trait)]
pub trait RpcApi<N, T>: Send + Sync {
    /// Lists the enabled RPC namespaces and the versions of each.
    async fn rpc_modules(&self) -> TransportResult<RpcModules>;
}

#[cfg_attr(target_arch = "wasm32", async_trait::async_trait(?Send))]
#[cfg_attr(not(target_arch = "wasm32"), async_trait::async_trait)]
impl<N, T, P> RpcApi<N, T> for P
where
    N: Network,
    T: Transport + Clone,
    P: Provider<T, N>,
{
    async fn rpc_modules(&self) -> TransportResult<RpcModules> {
        self.client().request("rpc_modules", ()).await
    }
}
