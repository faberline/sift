//! Reload the peer mTLS material of a running Sift process when its files
//! change. The kubelet refreshes the mounted Secret files; the process keeps
//! its last good certificate until the files change again.
use std::time::Duration;

/// How often a running process checks the three peer TLS files.
pub(crate) const PEER_TLS_RELOAD_INTERVAL: Duration = Duration::from_secs(30);

/// Start one task that reloads `transport` when the content of the cert, key
/// or CA file changes. A failed reload keeps the previous generation and is
/// not retried until the content changes again.
pub(crate) fn spawn_peer_tls_reload(
    transport: raft_runtime::PeerTransport,
    config: peer_tls::PeerTlsConfig,
    interval: Duration,
) -> tokio::task::JoinHandle<()> {
    let mut seen = read_material(&config).ok();
    tokio::spawn(async move {
        loop {
            tokio::time::sleep(interval).await;
            let now = match read_material(&config) {
                Ok(material) => material,
                Err(error) => {
                    tracing::warn!(%error, "peer TLS material unreadable; keeping the current certificate");
                    continue;
                }
            };
            if seen.as_ref() == Some(&now) {
                continue;
            }
            seen = Some(now);
            match transport.reload(&config) {
                Ok(generation) => tracing::info!(generation, "peer TLS material reloaded"),
                Err(error) => tracing::warn!(
                    %error,
                    "peer TLS reload failed; keeping the previous generation"
                ),
            }
        }
    })
}

fn read_material(config: &peer_tls::PeerTlsConfig) -> std::io::Result<[Vec<u8>; 3]> {
    Ok([
        std::fs::read(config.cert())?,
        std::fs::read(config.key())?,
        std::fs::read(config.ca())?,
    ])
}

#[cfg(test)]
mod tests;
