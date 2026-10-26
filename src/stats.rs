use std::time::{Duration, Instant};

/// A query is slow once it has used half the time it is allowed, which is
/// early enough to see it coming before it times out.
pub fn is_slow(elapsed: Duration, limit: Duration) -> bool {
    elapsed * 2 >= limit
}

/// How long one feed's queries have been taking, summed up for the journal.
pub struct PollStats {
    count: u32,
    total: Duration,
    slowest: Duration,
    since: Instant,
}

impl PollStats {
    pub fn new(now: Instant) -> Self {
        Self { count: 0, total: Duration::ZERO, slowest: Duration::ZERO, since: now }
    }

    pub fn record(&mut self, elapsed: Duration) {
        self.count += 1;
        self.total += elapsed;
        self.slowest = self.slowest.max(elapsed);
    }

    /// The line to log once `every` has passed since the last one, after which
    /// the counts start over. None before then, or if nothing was polled.
    pub fn summary(&mut self, now: Instant, every: Duration) -> Option<String> {
        if now.duration_since(self.since) < every {
            return None;
        }
        let line = (self.count > 0).then(|| {
            format!(
                "{} polls, average {:.2}s, slowest {:.2}s",
                self.count,
                self.total.as_secs_f64() / f64::from(self.count),
                self.slowest.as_secs_f64()
            )
        });
        *self = Self::new(now);
        line
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn half_the_allowance_is_slow() {
        assert!(!is_slow(Duration::from_secs(4), Duration::from_secs(10)));
        assert!(is_slow(Duration::from_secs(5), Duration::from_secs(10)));
    }

    #[test]
    fn the_summary_comes_once_per_period_and_starts_over() {
        let t0 = Instant::now();
        let mut stats = PollStats::new(t0);
        stats.record(Duration::from_secs(1));
        stats.record(Duration::from_secs(3));
        let hour = Duration::from_secs(3600);
        assert_eq!(stats.summary(t0 + Duration::from_secs(60), hour), None);
        let line = stats.summary(t0 + hour, hour).unwrap();
        assert_eq!(line, "2 polls, average 2.00s, slowest 3.00s");
        assert_eq!(stats.summary(t0 + hour * 2, hour), None);
    }
}
