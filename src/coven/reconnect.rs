// ABOUTME: Exponential backoff reconnection for coven gRPC streams.
// ABOUTME: Retries with 2s, 4s, 8s... up to 60s max delay on stream drop.

use std::time::Duration;

/// Backoff configuration for gRPC stream reconnection
#[derive(Debug, Clone)]
pub struct BackoffConfig {
    /// Starting delay between retries
    pub initial_delay: Duration,
    /// Maximum delay between retries
    pub max_delay: Duration,
    /// Multiplier applied to delay after each failure
    pub multiplier: u32,
    /// Maximum number of consecutive failures before giving up (0 = unlimited)
    pub max_retries: u32,
}

impl Default for BackoffConfig {
    fn default() -> Self {
        Self {
            initial_delay: Duration::from_secs(2),
            max_delay: Duration::from_secs(60),
            multiplier: 2,
            max_retries: 0, // unlimited
        }
    }
}

/// Tracks reconnection state with exponential backoff
#[derive(Debug)]
pub struct BackoffState {
    config: BackoffConfig,
    consecutive_failures: u32,
    current_delay: Duration,
}

impl BackoffState {
    /// Create a new backoff state with the given config
    pub fn new(config: BackoffConfig) -> Self {
        let current_delay = config.initial_delay;
        Self {
            config,
            consecutive_failures: 0,
            current_delay,
        }
    }

    /// Record a successful connection (resets backoff)
    pub fn record_success(&mut self) {
        self.consecutive_failures = 0;
        self.current_delay = self.config.initial_delay;
    }

    /// Record a failure and return the delay before next retry, or None if max retries exceeded
    pub fn record_failure(&mut self) -> Option<Duration> {
        self.consecutive_failures += 1;

        // Check if we've exceeded max retries
        if self.config.max_retries > 0 && self.consecutive_failures > self.config.max_retries {
            return None;
        }

        let delay = self.current_delay;

        // Calculate next delay with exponential backoff, capped at max_delay
        self.current_delay = std::cmp::min(
            self.current_delay * self.config.multiplier,
            self.config.max_delay,
        );

        Some(delay)
    }

    /// Get the number of consecutive failures
    pub fn consecutive_failures(&self) -> u32 {
        self.consecutive_failures
    }

    /// Get the current delay that would be used on next failure
    pub fn current_delay(&self) -> Duration {
        self.current_delay
    }
}

// =============================================================================
// Tests
// =============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_backoff_config() {
        let config = BackoffConfig::default();
        assert_eq!(config.initial_delay, Duration::from_secs(2));
        assert_eq!(config.max_delay, Duration::from_secs(60));
        assert_eq!(config.multiplier, 2);
        assert_eq!(config.max_retries, 0);
    }

    #[test]
    fn test_exponential_backoff_sequence() {
        let config = BackoffConfig::default();
        let mut state = BackoffState::new(config);

        // First failure: 2s delay
        assert_eq!(state.record_failure(), Some(Duration::from_secs(2)));

        // Second failure: 4s delay
        assert_eq!(state.record_failure(), Some(Duration::from_secs(4)));

        // Third failure: 8s delay
        assert_eq!(state.record_failure(), Some(Duration::from_secs(8)));

        // Fourth failure: 16s delay
        assert_eq!(state.record_failure(), Some(Duration::from_secs(16)));

        // Fifth failure: 32s delay
        assert_eq!(state.record_failure(), Some(Duration::from_secs(32)));

        // Sixth failure: capped at 60s
        assert_eq!(state.record_failure(), Some(Duration::from_secs(60)));

        // Seventh failure: still 60s
        assert_eq!(state.record_failure(), Some(Duration::from_secs(60)));

        assert_eq!(state.consecutive_failures(), 7);
    }

    #[test]
    fn test_success_resets_backoff() {
        let config = BackoffConfig::default();
        let mut state = BackoffState::new(config);

        // Build up some failures
        state.record_failure();
        state.record_failure();
        state.record_failure();
        assert_eq!(state.consecutive_failures(), 3);

        // Success resets everything
        state.record_success();
        assert_eq!(state.consecutive_failures(), 0);
        assert_eq!(state.current_delay(), Duration::from_secs(2));

        // Next failure starts from initial delay again
        assert_eq!(state.record_failure(), Some(Duration::from_secs(2)));
    }

    #[test]
    fn test_max_retries_exceeded() {
        let config = BackoffConfig {
            max_retries: 3,
            ..BackoffConfig::default()
        };
        let mut state = BackoffState::new(config);

        // First three retries succeed
        assert!(state.record_failure().is_some());
        assert!(state.record_failure().is_some());
        assert!(state.record_failure().is_some());

        // Fourth retry exceeds max_retries (3)
        assert_eq!(state.record_failure(), None);
    }

    #[test]
    fn test_unlimited_retries() {
        let config = BackoffConfig {
            max_retries: 0, // unlimited
            ..BackoffConfig::default()
        };
        let mut state = BackoffState::new(config);

        // Should never return None
        for _ in 0..100 {
            assert!(state.record_failure().is_some());
        }
        assert_eq!(state.consecutive_failures(), 100);
    }

    #[test]
    fn test_delay_caps_at_max() {
        let config = BackoffConfig {
            initial_delay: Duration::from_secs(1),
            max_delay: Duration::from_secs(10),
            multiplier: 3,
            max_retries: 0,
        };
        let mut state = BackoffState::new(config);

        // 1s
        assert_eq!(state.record_failure(), Some(Duration::from_secs(1)));
        // 3s
        assert_eq!(state.record_failure(), Some(Duration::from_secs(3)));
        // 9s
        assert_eq!(state.record_failure(), Some(Duration::from_secs(9)));
        // 10s (capped, not 27s)
        assert_eq!(state.record_failure(), Some(Duration::from_secs(10)));
        // Still 10s
        assert_eq!(state.record_failure(), Some(Duration::from_secs(10)));
    }
}
