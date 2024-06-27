use alloy_json_rpc::{RpcParam, RpcReturn};
use alloy_rpc_client::{RpcCall, Waiter};
use alloy_transport::{Transport, TransportResult};
use futures::FutureExt;
use pin_project::pin_project;
use serde_json::value::RawValue;
use std::{
    future::Future,
    pin::Pin,
    task::{self, Poll},
};
use tokio::sync::oneshot;

/// The primary future type for the [`Provider`].
///
/// This future abstracts over several potential data sources. It allows
/// providers to:
/// - produce data via an [`RpcCall`]
/// - produce data by waiting on a batched RPC [`Waiter`]
/// - proudce data via an arbitrary boxed future
/// - produce data in any synchronous way
///
/// [`Provider`]: crate::Provider
#[pin_project(project = ProviderCallProj)]
pub enum ProviderCall<Conn, Params, Resp, Output = Resp, Map = fn(Resp) -> Output>
where
    Conn: Transport + Clone,
    Params: RpcParam,
    Map: FnOnce(Resp) -> Output,
{
    /// An underlying call to an RPC server.
    RpcCall(RpcCall<Conn, Params, Resp, Output, Map>),
    /// A waiter for a batched call to a remote RPC server.
    Waiter(Waiter<Resp, Output, Map>),
    /// A boxed future.
    BoxedFuture(Pin<Box<dyn Future<Output = TransportResult<Output>> + Send>>),
    /// The output, produces synchronously.
    Ready(Option<Output>),
}

impl<Conn, Params, Resp, Output, Map> ProviderCall<Conn, Params, Resp, Output, Map>
where
    Conn: Transport + Clone,
    Params: RpcParam,
    Map: FnOnce(Resp) -> Output,
{
    /// Instantiate a new [`ProviderCall`] from the output.
    pub const fn ready(output: Output) -> Self {
        Self::Ready(Some(output))
    }

    /// True if this is an RPC call.
    pub const fn is_rpc_call(&self) -> bool {
        matches!(self, Self::RpcCall(_))
    }

    /// Fallible cast to [`RpcCall`]
    pub const fn as_rpc_call(&self) -> Option<&RpcCall<Conn, Params, Resp, Output, Map>> {
        match self {
            Self::RpcCall(call) => Some(call),
            _ => None,
        }
    }

    /// Fallible cast to mutable [`RpcCall`]
    pub fn as_mut_rpc_call(&mut self) -> Option<&mut RpcCall<Conn, Params, Resp, Output, Map>> {
        match self {
            Self::RpcCall(call) => Some(call),
            _ => None,
        }
    }

    /// True if this is a waiter.
    pub const fn is_waiter(&self) -> bool {
        matches!(self, Self::Waiter(_))
    }

    /// Fallible cast to [`Waiter`]
    pub const fn as_waiter(&self) -> Option<&Waiter<Resp, Output, Map>> {
        match self {
            Self::Waiter(waiter) => Some(waiter),
            _ => None,
        }
    }

    /// Fallible cast to mutable [`Waiter`]
    pub fn as_mut_waiter(&mut self) -> Option<&mut Waiter<Resp, Output, Map>> {
        match self {
            Self::Waiter(waiter) => Some(waiter),
            _ => None,
        }
    }

    /// True if this is a boxed future.
    pub const fn is_boxed_future(&self) -> bool {
        matches!(self, Self::BoxedFuture(_))
    }

    /// Fallible cast to a boxed future.
    pub const fn as_boxed_future(
        &self,
    ) -> Option<&Pin<Box<dyn Future<Output = TransportResult<Output>> + Send>>> {
        match self {
            Self::BoxedFuture(fut) => Some(fut),
            _ => None,
        }
    }

    /// True if this is a ready value.
    pub const fn is_ready(&self) -> bool {
        matches!(self, Self::Ready(_))
    }

    /// Fallible cast to a ready value.
    ///
    /// # Panics
    ///
    /// Panics if the future is already complete
    pub const fn as_ready(&self) -> Option<&Output> {
        match self {
            Self::Ready(Some(output)) => Some(output),
            Self::Ready(None) => panic!("tried to access ready value after taking"),
            _ => None,
        }
    }

    /// Set a function to map the response into a different type. This is
    /// useful for transforming the response into a more usable type, e.g.
    /// changing `U64` to `u64`.
    ///
    /// This function fails if the inner future is not an [`RpcCall`] or
    /// [`Waiter`].
    ///
    /// ## Note
    ///
    /// Carefully review the rust documentation on [fn pointers] before passing
    /// them to this function. Unless the pointer is specifically coerced to a
    /// `fn(_) -> _`, the `NewMap` will be inferred as that function's unique
    /// type. This can lead to confusing error messages.
    ///
    /// [fn pointers]: https://doc.rust-lang.org/std/primitive.fn.html#creating-function-pointers
    pub fn map_resp<NewOutput, NewMap>(
        self,
        map: NewMap,
    ) -> Result<ProviderCall<Conn, Params, Resp, NewOutput, NewMap>, Self>
    where
        NewMap: FnOnce(Resp) -> NewOutput,
    {
        match self {
            Self::RpcCall(call) => Ok(ProviderCall::RpcCall(call.map_resp(map))),
            Self::Waiter(waiter) => Ok(ProviderCall::Waiter(waiter.map_resp(map))),
            _ => Err(self),
        }
    }
}

impl<Conn, Params, Resp, Output, Map> ProviderCall<Conn, &Params, Resp, Output, Map>
where
    Conn: Transport + Clone,
    Params: RpcParam,
    Params: ToOwned,
    Params::Owned: RpcParam,
    Map: FnOnce(Resp) -> Output,
{
    /// Convert this call into one with owned params, by cloning the params.
    ///
    /// # Panics
    ///
    /// Panics if called after the request has been polled.
    pub fn into_owned_params(self) -> ProviderCall<Conn, Params::Owned, Resp, Output, Map> {
        match self {
            Self::RpcCall(call) => ProviderCall::RpcCall(call.into_owned_params()),
            _ => panic!(),
        }
    }
}

impl<Conn, Params, Resp, Output, Map> std::fmt::Debug
    for ProviderCall<Conn, Params, Resp, Output, Map>
where
    Conn: Transport + Clone,
    Params: RpcParam,
    Map: FnOnce(Resp) -> Output,
{
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::RpcCall(call) => f.debug_tuple("RpcCall").field(call).finish(),
            Self::Waiter { .. } => f.debug_struct("Waiter").finish_non_exhaustive(),
            Self::BoxedFuture(_) => f.debug_struct("BoxedFuture").finish_non_exhaustive(),
            Self::Ready(_) => f.debug_struct("Ready").finish_non_exhaustive(),
        }
    }
}

impl<Conn, Params, Resp, Output, Map> From<RpcCall<Conn, Params, Resp, Output, Map>>
    for ProviderCall<Conn, Params, Resp, Output, Map>
where
    Conn: Transport + Clone,
    Params: RpcParam,
    Map: FnOnce(Resp) -> Output,
{
    fn from(call: RpcCall<Conn, Params, Resp, Output, Map>) -> Self {
        Self::RpcCall(call)
    }
}

impl<Conn, Params, Resp> From<Waiter<Resp>>
    for ProviderCall<Conn, Params, Resp, Resp, fn(Resp) -> Resp>
where
    Conn: Transport + Clone,
    Params: RpcParam,
{
    fn from(waiter: Waiter<Resp>) -> Self {
        Self::Waiter(waiter)
    }
}

impl<Conn, Params, Resp, Output, Map>
    From<Pin<Box<dyn Future<Output = TransportResult<Output>> + Send>>>
    for ProviderCall<Conn, Params, Resp, Output, Map>
where
    Conn: Transport + Clone,
    Params: RpcParam,
    Map: FnOnce(Resp) -> Output,
{
    fn from(fut: Pin<Box<dyn Future<Output = TransportResult<Output>> + Send>>) -> Self {
        Self::BoxedFuture(fut)
    }
}

impl<Conn, Params, Resp> From<oneshot::Receiver<TransportResult<Box<RawValue>>>>
    for ProviderCall<Conn, Params, Resp>
where
    Conn: Transport + Clone,
    Params: RpcParam,
{
    fn from(rx: oneshot::Receiver<TransportResult<Box<RawValue>>>) -> Self {
        Waiter::from(rx).into()
    }
}

impl<Conn, Params, Resp, Output, Map> Future for ProviderCall<Conn, Params, Resp, Output, Map>
where
    Conn: Transport + Clone,
    Params: RpcParam,
    Resp: RpcReturn,
    Output: 'static,
    Map: FnOnce(Resp) -> Output,
{
    type Output = TransportResult<Output>;

    fn poll(mut self: Pin<&mut Self>, cx: &mut task::Context<'_>) -> task::Poll<Self::Output> {
        match self.as_mut().project() {
            ProviderCallProj::RpcCall(call) => call.poll_unpin(cx),
            ProviderCallProj::Waiter(waiter) => waiter.poll_unpin(cx),
            ProviderCallProj::BoxedFuture(fut) => fut.poll_unpin(cx),
            ProviderCallProj::Ready(output) => {
                Poll::Ready(Ok(output.take().expect("output taken twice")))
            }
        }
    }
}