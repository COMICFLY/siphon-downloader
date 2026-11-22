use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;
use tokio::sync::Mutex;
use tokio::time::Instant;
use tokio_util::sync::CancellationToken;

/// Async token-bucket rate limiter supporting hot runtime limit changes.
/// `limit_bps == 0` disables throttling completely.
#[derive(Debug)]
pub struct TokenBucket {
    limit_bps: AtomicU64,
    state: Mutex<BucketState>,
}

#[derive(Debug)]
struct BucketState {
    available_bytes: f64,
    last_refill: Instant,
}

impl TokenBucket {
    pub fn new(limit_bps: u64) -> Arc<Self> {
        Arc::new(Self {
            limit_bps: AtomicU64::new(limit_bps),
            state: Mutex::new(BucketState {
                available_bytes: (limit_bps as f64 * 0.2).max(16384.0),
                last_refill: Instant::now(),
            }),
        })
    }

    pub fn set_limit(&self, limit_bps: u64) {
        self.limit_bps.store(limit_bps, Ordering::Relaxed);
    }

    pub fn limit(&self) -> u64 {
        self.limit_bps.load(Ordering::Relaxed)
    }

    /// Consume `bytes` tokens, asynchronously sleeping if the bucket is temporarily depleted.
    pub async fn consume(&self, bytes: u64, cancel: &CancellationToken) -> bool {
        let mut remaining = bytes as f64;
        while remaining > 0.0 {
            let limit = self.limit_bps.load(Ordering::Relaxed);
            if limit == 0 {
                return true;
            }

            let sleep_dur = {
                let mut st = self.state.lock().await;
                let now = Instant::now();
                let elapsed = now.duration_since(st.last_refill).as_secs_f64();
                st.last_refill = now;

                // Cap burst at 250ms of quota (minimum 16 KiB) for smooth throttling
                let burst_cap = (limit as f64 * 0.25).max(16384.0);
                st.available_bytes = (st.available_bytes + elapsed * limit as f64).min(burst_cap);

                if st.available_bytes >= remaining {
                    st.available_bytes -= remaining;
                    remaining = 0.0;
                    None
                } else if st.available_bytes > 0.0 {
                    remaining -= st.available_bytes;
                    st.available_bytes = 0.0;
                    let wait_secs = (remaining / limit as f64).clamp(0.01, 0.25);
                    Some(Duration::from_secs_f64(wait_secs))
                } else {
                    let wait_secs = (remaining / limit as f64).clamp(0.01, 0.25);
                    Some(Duration::from_secs_f64(wait_secs))
                }
            };

            if let Some(dur) = sleep_dur {
                tokio::select! {
                    _ = cancel.cancelled() => return false,
                    _ = tokio::time::sleep(dur) => {}
                }
            }
        }
        true
    }
}

/// Two-level hierarchical limiter: enforces both global and per-task rate caps.
#[derive(Clone, Debug)]
pub struct HierarchicalLimiter {
    pub global: Arc<TokenBucket>,
    pub task: Arc<TokenBucket>,
}

impl HierarchicalLimiter {
    pub fn new(global: Arc<TokenBucket>, task: Arc<TokenBucket>) -> Self {
        Self { global, task }
    }

    pub async fn consume(&self, bytes: u64, cancel: &CancellationToken) -> bool {
        if !self.task.consume(bytes, cancel).await {
            return false;
        }
        self.global.consume(bytes, cancel).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn unlimited_bucket_returns_immediately() {
        let bucket = TokenBucket::new(0);
        let cancel = CancellationToken::new();
        let start = Instant::now();
        assert!(bucket.consume(10_000_000, &cancel).await);
        assert!(start.elapsed() < Duration::from_millis(50));
    }

    #[tokio::test]
    async fn limited_bucket_throttles_reasonably() {
        // 100 KB/s limit, consume 60 KB in chunks (after initial burst)
        let bucket = TokenBucket::new(100_000);
        let cancel = CancellationToken::new();
        let start = Instant::now();
        for _ in 0..8 {
            assert!(bucket.consume(10_000, &cancel).await);
        }
        // Initial burst is 20 KB -> remaining 60 KB at 100 KB/s ~= 600ms (allow generous tolerance)
        assert!(start.elapsed() >= Duration::from_millis(350));
    }
}
