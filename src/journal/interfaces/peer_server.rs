//! The dedicated, mutually authenticated Raft peer listener.

use axum::Router;

use crate::app::service_state::ServiceState;

impl ServiceState {
    /// Return the dedicated mutually authenticated Raft listener parts.
    /// Raft routes must never be merged into the public Sift API router.
    pub fn peer_server(&self) -> Option<(raft_runtime::PeerTransport, u16, Router)> {
        Some((
            self.peer_transport.clone()?,
            self.peer_port?,
            self.raft.as_ref()?.router(),
        ))
    }
}
