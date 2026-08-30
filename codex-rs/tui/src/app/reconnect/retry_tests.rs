use super::*;
use pretty_assertions::assert_eq;
use std::future::pending;
use tokio::time::Instant;

#[tokio::test(start_paused = true)]
async fn retries_continue_with_capped_exponential_delays() {
    let mut backoff = ReconnectBackoff::default();
    let mut elapsed = Vec::new();
    for _ in 0..17 {
        let start = Instant::now();
        backoff.wait_for_retry(pending()).await;
        elapsed.push(start.elapsed().as_secs());
    }
    assert_eq!(
        elapsed,
        [
            1, 2, 4, 8, 16, 32, 64, 128, 256, 512, 1024, 2048, 4096, 8192, 14400, 14400, 14400,
        ]
    );
}

#[tokio::test(start_paused = true)]
async fn network_change_shortens_wait_and_resets_backoff() {
    let mut backoff = ReconnectBackoff::default();
    for _ in 0..15 {
        backoff.wait_for_retry(pending()).await;
    }
    let start = Instant::now();
    backoff
        .wait_for_retry(tokio::time::sleep(Duration::from_secs(/*secs*/ 3)))
        .await;
    let changed_after = start.elapsed().as_secs();
    let start = Instant::now();
    backoff.wait_for_retry(pending()).await;
    assert_eq!((changed_after, start.elapsed().as_secs()), (3, 1));
}

#[tokio::test(start_paused = true)]
async fn dropping_retry_wait_cancels_it_promptly() {
    let mut backoff = ReconnectBackoff::default();
    for _ in 0..15 {
        backoff.wait_for_retry(pending()).await;
    }
    let start = Instant::now();
    assert!(
        tokio::time::timeout(
            Duration::from_millis(/*millis*/ 50),
            backoff.wait_for_retry(pending()),
        )
        .await
        .is_err()
    );
    assert_eq!(start.elapsed(), Duration::from_millis(/*millis*/ 50));
}
