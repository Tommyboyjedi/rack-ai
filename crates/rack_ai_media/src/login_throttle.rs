use std::time::{Duration, Instant};
const MAX_BACKOFF_SECONDS: u64 = 30;
const FORGET_AFTER_SECONDS: u64 = 300;
#[derive(Default)]
pub struct LoginThrottle {
    failures: u32,
    retry_at: Option<Instant>,
    last_failure: Option<Instant>,
}
impl LoginThrottle {
    pub fn remaining(&mut self, now: Instant) -> u64 {
        if self.last_failure.is_some_and(|last| {
            now.saturating_duration_since(last) >= Duration::from_secs(FORGET_AFTER_SECONDS)
        }) {
            self.success();
        }
        self.retry_at
            .map(|at| {
                let remaining = at.saturating_duration_since(now);
                remaining.as_secs() + u64::from(remaining.subsec_nanos() > 0)
            })
            .unwrap_or(0)
    }
    pub fn failure(&mut self, now: Instant) {
        self.failures = self.failures.saturating_add(1).min(6);
        let delay = (1u64 << (self.failures - 1)).min(MAX_BACKOFF_SECONDS);
        self.retry_at = Some(now + Duration::from_secs(delay));
        self.last_failure = Some(now);
    }
    pub fn success(&mut self) {
        self.failures = 0;
        self.retry_at = None;
        self.last_failure = None;
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn bounded_backoff_expires_and_success_clears_it() {
        let mut throttle = LoginThrottle::default();
        let mut now = Instant::now();
        for expected in [1, 2, 4, 8, 16, 30, 30] {
            assert_eq!(throttle.remaining(now), 0);
            throttle.failure(now);
            assert!(throttle.remaining(now) <= 31);
            now += Duration::from_secs(expected);
        }
        assert_eq!(throttle.remaining(now), 0);
        throttle.success();
        throttle.failure(now);
        assert_eq!(throttle.remaining(now + Duration::from_secs(1)), 0);
        assert_eq!(throttle.remaining(now + Duration::from_secs(301)), 0);
    }
}
