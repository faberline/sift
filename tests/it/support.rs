//! Helpers shared by the `it` integration-test modules.

/// The libtest name of `function` in the module whose `module_path!()` is
/// `module_path`, for re-running exactly that case with `--exact`.
#[cfg(unix)]
pub fn test_name(module_path: &str, function: &str) -> String {
    let module = module_path
        .split_once("::")
        .map_or("", |(_crate, module)| module);
    format!("{module}::{function}")
}

/// Open `root` as a new `ServiceState` after a previous one was dropped.
/// The data-root lock is released when the last clone of the old state is
/// gone, and a background worker can hold a clone for a short time. Retry
/// only the lock error, every 20 ms, until `timeout` has passed. Any other
/// error, and the lock error after the timeout, is returned unchanged.
pub async fn reopen_service_state(
    root: &std::path::Path,
    timeout: std::time::Duration,
) -> anyhow::Result<sift::ServiceState> {
    let deadline = std::time::Instant::now() + timeout;
    loop {
        match sift::ServiceState::open(root) {
            Ok(state) => return Ok(state),
            Err(error)
                if format!("{error:#}").contains("another Sift process")
                    && std::time::Instant::now() < deadline =>
            {
                tokio::time::sleep(std::time::Duration::from_millis(20)).await;
            }
            Err(error) => return Err(error),
        }
    }
}

/// The timeout that the restart tests use. It is far above the observed
/// release delay and far below the CI job timeout.
pub const REOPEN_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(10);

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::{reopen_service_state, REOPEN_TIMEOUT};

    #[tokio::test]
    async fn reopen_waits_until_the_first_state_is_released() {
        let temp = tempfile::tempdir().unwrap();
        let first = sift::ServiceState::open(temp.path()).unwrap();
        let releaser = std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(300));
            drop(first);
        });

        let reopened = reopen_service_state(temp.path(), REOPEN_TIMEOUT).await;
        releaser.join().unwrap();
        assert!(reopened.is_ok(), "{:#}", reopened.err().unwrap());
    }

    #[tokio::test]
    async fn reopen_returns_the_lock_error_after_the_timeout() {
        let temp = tempfile::tempdir().unwrap();
        let first = sift::ServiceState::open(temp.path()).unwrap();

        let result = reopen_service_state(temp.path(), Duration::from_millis(100)).await;
        let error = result.err().expect("the lock must refuse");
        assert!(
            format!("{error:#}").contains("another Sift process"),
            "{error:#}"
        );
        drop(first);
    }
}
