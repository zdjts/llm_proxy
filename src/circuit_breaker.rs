//! Circuit breaker with half-open state for per-key failure tracking.
//!
//! Tracks failure counts per (pool_id, key_hash) with configurable thresholds.
//! States: Closed → (failures >= threshold) → Open → (cooldown elapsed) → HalfOpen → (success) → Closed
//!
//! More sophisticated than the raw BadKeyRegistry — supports gradual recovery.

use std::sync::Arc;
use std::sync::atomic::{AtomicU32, AtomicU64, Ordering};
use std::time::{Duration, Instant};

use dashmap::DashMap;

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum CircuitState {
    Closed,
    Open,
    HalfOpen,
}

struct CircuitEntry {
    failures: AtomicU32,
    successes: AtomicU32,
    last_failure_at: std::sync::Mutex<Option<Instant>>,
    opened_at: std::sync::Mutex<Option<Instant>>,
    state: std::sync::Mutex<CircuitState>,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct CircuitSnapshot {
    pub pool_id: String,
    pub key_hash: String,
    pub state: String,
    pub failures: u32,
}

impl CircuitEntry {
    fn new() -> Self {
        Self {
            failures: AtomicU32::new(0),
            successes: AtomicU32::new(0),
            last_failure_at: std::sync::Mutex::new(None),
            opened_at: std::sync::Mutex::new(None),
            state: std::sync::Mutex::new(CircuitState::Closed),
        }
    }
}

#[derive(Clone)]
pub struct CircuitBreaker {
    circuits: Arc<DashMap<(String, String), Arc<CircuitEntry>>>,
    failure_threshold: u32,
    cooldown_secs: u64,
    half_open_max: u32,
    total_trips: Arc<AtomicU64>,
}

impl CircuitBreaker {
    pub fn new(failure_threshold: u32, cooldown_secs: u64, half_open_max: u32) -> Self {
        Self {
            circuits: Arc::new(DashMap::new()),
            failure_threshold,
            cooldown_secs,
            half_open_max,
            total_trips: Arc::new(AtomicU64::new(0)),
        }
    }

    pub fn with_defaults() -> Self {
        Self::new(5, 60, 3)
    }

    /// Check if a key is allowed to be used. Returns true if the circuit allows.
    pub fn allow(&self, pool_id: &str, key_hash: &str) -> bool {
        let key = (pool_id.to_owned(), key_hash.to_owned());
        let entry = self
            .circuits
            .entry(key)
            .or_insert_with(|| Arc::new(CircuitEntry::new()));

        let state = *entry.state.lock().unwrap();

        match state {
            CircuitState::Closed => true,
            CircuitState::Open => {
                let opened = *entry.opened_at.lock().unwrap();
                let cooldown = Duration::from_secs(self.cooldown_secs);
                if let Some(at) = opened
                    && at.elapsed() >= cooldown
                {
                    *entry.state.lock().unwrap() = CircuitState::HalfOpen;
                    entry.successes.store(0, Ordering::Relaxed);
                    true
                } else {
                    false
                }
            }
            CircuitState::HalfOpen => entry.successes.load(Ordering::Relaxed) < self.half_open_max,
        }
    }

    /// Record a successful request against this key.
    pub fn record_success(&self, pool_id: &str, key_hash: &str) {
        let key = (pool_id.to_owned(), key_hash.to_owned());
        let entry = self
            .circuits
            .entry(key)
            .or_insert_with(|| Arc::new(CircuitEntry::new()));

        let mut state = entry.state.lock().unwrap();

        match *state {
            CircuitState::Closed => {
                entry.failures.store(0, Ordering::Relaxed);
            }
            CircuitState::HalfOpen => {
                let succ = entry.successes.fetch_add(1, Ordering::Relaxed) + 1;
                if succ >= self.half_open_max {
                    *state = CircuitState::Closed;
                    entry.failures.store(0, Ordering::Relaxed);
                }
            }
            CircuitState::Open => {}
        }
    }

    /// Record a failure against this key. Returns true if circuit just tripped.
    pub fn record_failure(&self, pool_id: &str, key_hash: &str) -> bool {
        let key = (pool_id.to_owned(), key_hash.to_owned());
        let entry = self
            .circuits
            .entry(key)
            .or_insert_with(|| Arc::new(CircuitEntry::new()));

        let mut state = entry.state.lock().unwrap();

        match *state {
            CircuitState::Closed => {
                entry
                    .last_failure_at
                    .lock()
                    .unwrap()
                    .replace(Instant::now());
                let failures = entry.failures.fetch_add(1, Ordering::Relaxed) + 1;
                if failures >= self.failure_threshold {
                    *state = CircuitState::Open;
                    entry.opened_at.lock().unwrap().replace(Instant::now());
                    self.total_trips.fetch_add(1, Ordering::Relaxed);
                    true
                } else {
                    false
                }
            }
            CircuitState::HalfOpen => {
                *state = CircuitState::Open;
                entry.opened_at.lock().unwrap().replace(Instant::now());
                self.total_trips.fetch_add(1, Ordering::Relaxed);
                true
            }
            CircuitState::Open => false,
        }
    }

    /// Get the current state of a circuit.
    pub fn state(&self, pool_id: &str, key_hash: &str) -> CircuitState {
        let key = (pool_id.to_owned(), key_hash.to_owned());
        match self.circuits.get(&key) {
            Some(entry) => *entry.state.lock().unwrap(),
            None => CircuitState::Closed,
        }
    }

    /// Force reset a circuit to closed.
    pub fn reset(&self, pool_id: &str, key_hash: &str) {
        let key = (pool_id.to_owned(), key_hash.to_owned());
        if let Some(entry) = self.circuits.get(&key) {
            entry.failures.store(0, Ordering::Relaxed);
            entry.successes.store(0, Ordering::Relaxed);
            *entry.state.lock().unwrap() = CircuitState::Closed;
        }
    }

    /// Number of times a circuit has tripped (aggregate).
    pub fn total_trips(&self) -> u64 {
        self.total_trips.load(Ordering::Relaxed)
    }

    /// Snapshot of all active circuits for dashboard.
    pub fn snapshot(&self) -> Vec<CircuitSnapshot> {
        self.circuits
            .iter()
            .map(|entry| {
                let (pool_id, key_hash) = entry.key();
                let e = entry.value();
                let state_str = match *e.state.lock().unwrap() {
                    CircuitState::Closed => "closed",
                    CircuitState::Open => "open",
                    CircuitState::HalfOpen => "half_open",
                };
                CircuitSnapshot {
                    pool_id: pool_id.clone(),
                    key_hash: key_hash.clone(),
                    state: state_str.to_string(),
                    failures: e.failures.load(Ordering::Relaxed),
                }
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn circuit_starts_closed() {
        let cb = CircuitBreaker::with_defaults();
        assert!(cb.allow("p1", "k1"));
        assert_eq!(cb.state("p1", "k1"), CircuitState::Closed);
    }

    #[test]
    fn circuit_opens_after_threshold_failures() {
        let cb = CircuitBreaker::new(3, 60, 2);
        assert!(cb.allow("p1", "k1"));
        cb.record_failure("p1", "k1");
        cb.record_failure("p1", "k1");
        assert!(cb.allow("p1", "k1"));
        cb.record_failure("p1", "k1");
        assert!(!cb.allow("p1", "k1"));
        assert_eq!(cb.state("p1", "k1"), CircuitState::Open);
    }

    #[test]
    fn circuit_goes_half_open_after_cooldown() {
        let cb = CircuitBreaker::new(2, 1, 2);
        cb.record_failure("p1", "k1");
        cb.record_failure("p1", "k1");
        assert!(!cb.allow("p1", "k1"));
        // cooldown is 1s, so not yet half-open
        std::thread::sleep(std::time::Duration::from_secs(1));
        assert!(cb.allow("p1", "k1"));
        assert_eq!(cb.state("p1", "k1"), CircuitState::HalfOpen);
    }

    #[test]
    fn circuit_closes_after_half_open_successes() {
        let cb = CircuitBreaker::new(2, 0, 2);
        cb.record_failure("p1", "k1");
        cb.record_failure("p1", "k1");
        assert!(cb.allow("p1", "k1"));
        cb.record_success("p1", "k1");
        cb.record_success("p1", "k1");
        assert_eq!(cb.state("p1", "k1"), CircuitState::Closed);
    }

    #[test]
    fn circuit_reopens_on_half_open_failure() {
        let cb = CircuitBreaker::new(2, 0, 2);
        cb.record_failure("p1", "k1");
        cb.record_failure("p1", "k1");
        assert!(cb.allow("p1", "k1"));
        cb.record_failure("p1", "k1");
        assert_eq!(cb.state("p1", "k1"), CircuitState::Open);
    }

    #[test]
    fn reset_clears_failures() {
        let cb = CircuitBreaker::new(2, 60, 2);
        cb.record_failure("p1", "k1");
        cb.record_failure("p1", "k1");
        assert!(!cb.allow("p1", "k1"));
        cb.reset("p1", "k1");
        assert!(cb.allow("p1", "k1"));
        assert_eq!(cb.state("p1", "k1"), CircuitState::Closed);
    }

    #[test]
    fn total_trips_increments() {
        let cb = CircuitBreaker::new(2, 60, 2);
        assert_eq!(cb.total_trips(), 0);
        cb.record_failure("p1", "k1");
        cb.record_failure("p1", "k1");
        assert_eq!(cb.total_trips(), 1);
    }
}
