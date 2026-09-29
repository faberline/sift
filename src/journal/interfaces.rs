//! How the journal is reached from outside the process: the dedicated Raft peer
//! listener and the journal's Prometheus metrics.

pub(crate) mod journal_metrics;
pub(crate) mod peer_server;
