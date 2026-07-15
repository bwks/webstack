use std::{
    collections::HashMap,
    sync::Arc,
    time::{Duration, Instant},
};

use tokio::sync::Mutex;

const IDLE_EXPIRY: Duration = Duration::from_mins(15);
const MAX_BACKOFF: Duration = Duration::from_mins(1);

#[derive(Debug)]
struct FailureState {
    failures: u32,
    last_failure: Instant,
    blocked_until: Instant,
}

/// In-memory failed-login backoff shared by all login handlers.
#[derive(Clone, Debug, Default)]
pub struct LoginThrottle {
    entries: Arc<Mutex<HashMap<String, FailureState>>>,
}

impl LoginThrottle {
    /// Returns the remaining block duration for a key, if any.
    pub async fn remaining(&self, key: &str) -> Option<Duration> {
        let now = Instant::now();
        let mut entries = self.entries.lock().await;
        entries.retain(|_, state| now.duration_since(state.last_failure) < IDLE_EXPIRY);
        entries
            .get(key)
            .and_then(|state| state.blocked_until.checked_duration_since(now))
    }

    /// Records one failed login and returns the resulting block duration.
    pub async fn record_failure(&self, key: String) -> Duration {
        let now = Instant::now();
        let mut entries = self.entries.lock().await;
        let state = entries.entry(key).or_insert(FailureState {
            failures: 0,
            last_failure: now,
            blocked_until: now,
        });
        state.failures = state.failures.saturating_add(1);
        state.last_failure = now;
        let seconds = if state.failures <= 4 {
            0
        } else {
            1_u64.checked_shl(state.failures - 5).unwrap_or(60).min(60)
        };
        let delay = Duration::from_secs(seconds).min(MAX_BACKOFF);
        state.blocked_until = now + delay;
        delay
    }

    /// Clears failures after a successful login.
    pub async fn clear(&self, key: &str) {
        self.entries.lock().await.remove(key);
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::LoginThrottle;

    #[tokio::test]
    async fn fifth_failure_begins_bounded_exponential_backoff() {
        let throttle = LoginThrottle::default();
        for _attempt in 0..4 {
            assert_eq!(
                throttle.record_failure("client:user".to_owned()).await,
                Duration::ZERO
            );
        }
        assert_eq!(
            throttle.record_failure("client:user".to_owned()).await,
            Duration::from_secs(1)
        );
        assert!(throttle.remaining("client:user").await.is_some());
    }

    #[tokio::test]
    async fn success_clears_the_throttle_key() {
        let throttle = LoginThrottle::default();
        for _attempt in 0..5 {
            throttle.record_failure("client:user".to_owned()).await;
        }
        throttle.clear("client:user").await;
        assert!(throttle.remaining("client:user").await.is_none());
    }
}
