//! Retry waits stay bounded and network changes restart the short-delay sequence.

use std::future::Future;
use std::time::Duration;

pub(super) struct ReconnectBackoff {
    delay: Duration,
}

impl Default for ReconnectBackoff {
    fn default() -> Self {
        Self {
            delay: Duration::from_secs(/*secs*/ 1),
        }
    }
}

impl ReconnectBackoff {
    pub(super) async fn wait_for_retry(&mut self, network_changed: impl Future<Output = ()>) {
        tokio::select! {
            biased;
            () = network_changed => *self = Self::default(),
            () = tokio::time::sleep(self.delay) => {
                self.delay = (self.delay * 2).min(Duration::from_secs(/*secs*/ 4 * 60 * 60));
            }
        }
    }
}

#[cfg(test)]
#[path = "retry_tests.rs"]
mod tests;
