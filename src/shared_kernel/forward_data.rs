//! Transport data shared by query forwarding and HTTP rendering.

use bytes::Bytes;

pub(crate) type ForwardHeaders = Vec<(String, Vec<u8>)>;

pub(crate) enum ForwardFrame {
    Data(Bytes),
    Trailers(ForwardHeaders),
}

#[derive(Clone, Copy)]
pub(crate) struct BodyLength {
    pub(crate) lower: u64,
    pub(crate) upper: Option<u64>,
}
