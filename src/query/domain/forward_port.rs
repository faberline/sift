//! Request forwarding with pure request, header and response frame data.

use crate::shared_kernel::forward_data::{BodyLength, ForwardFrame, ForwardHeaders};
use crate::shared_kernel::port_future::PortFuture;
use crate::shared_kernel::query_request::QueryRequestV1;
use bytes::Bytes;

pub(crate) trait ForwardBody: Send {
    type Error: 'static;
    fn next_frame(&mut self) -> PortFuture<'_, Option<ForwardFrame>, Self::Error>;
    fn read_bounded(self: Box<Self>, limit: usize) -> PortFuture<'static, Bytes, Self::Error>;
    fn size_hint(&self) -> BodyLength;
    fn is_end_stream(&self) -> bool;
}

pub(crate) struct ForwardReply<E> {
    pub(crate) status: u16,
    pub(crate) status_text: String,
    pub(crate) headers: ForwardHeaders,
    pub(crate) body: Box<dyn ForwardBody<Error = E>>,
}

pub(crate) type ForwardFuture<'a, E, B> = PortFuture<'a, ForwardReply<B>, E>;

pub(crate) trait QueryForwardPort: Send + Sync {
    type Error;
    type BodyError: 'static;
    fn forward<'a>(
        &'a self,
        headers: ForwardHeaders,
        request: &'a QueryRequestV1,
    ) -> ForwardFuture<'a, Self::Error, Self::BodyError>;
}
