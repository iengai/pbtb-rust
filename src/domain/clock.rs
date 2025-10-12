use std::time::{SystemTime, UNIX_EPOCH};

/// Clock trait for getting current timestamp
/// Allows injection of mock clock in tests
pub trait Clock: Send + Sync {
    fn now(&self) -> i64;
}

/// Real system clock implementation
pub struct SystemClock;

impl Clock for SystemClock {
    fn now(&self) -> i64 {
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("System time went backwards")
            .as_secs() as i64
    }
}

#[cfg(test)]
pub struct MockClock {
    pub timestamp: i64,
}

#[cfg(test)]
impl Clock for MockClock {
    fn now(&self) -> i64 {
        self.timestamp
    }
}