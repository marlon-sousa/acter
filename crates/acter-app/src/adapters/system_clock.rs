//! Adapter: `SystemClock` implements the `Clock` driven port over the monotonic clock,
//! counting from construction.

use std::time::{Duration, Instant};

use acter_core::{Clock, Timer};
use tokio::sync::oneshot;

pub struct SystemClock {
    origin: Instant,
}

impl SystemClock {
    pub fn new() -> Self {
        Self {
            origin: Instant::now(),
        }
    }
}

impl Default for SystemClock {
    fn default() -> Self {
        Self::new()
    }
}

impl Clock for SystemClock {
    fn now(&self) -> Duration {
        self.origin.elapsed()
    }

    /// Must be called from within a tokio runtime.
    fn timer(&self, after: Duration) -> Timer {
        let (fire, fired) = oneshot::channel();
        tokio::spawn(async move {
            tokio::time::sleep(after).await;
            let _ = fire.send(());
        });
        Timer::new(fired)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const TICK: Duration = Duration::from_millis(5);

    #[tokio::test]
    async fn now_counts_from_construction_and_never_goes_backwards() {
        let clock = SystemClock::new();
        let first = clock.now();
        assert!(first < Duration::from_secs(1), "counts from construction");

        clock.timer(TICK).await;
        let second = clock.now();
        assert!(second >= first, "monotonic");
        assert!(second >= TICK, "time actually passed");
    }

    #[tokio::test]
    async fn a_timer_fires_no_earlier_than_its_deadline() {
        let clock = SystemClock::new();
        let before = clock.now();

        clock.timer(TICK).await;

        assert!(
            clock.now() - before >= TICK,
            "a wake must never arrive early: the policy re-reads the clock when it does"
        );
    }

    #[tokio::test]
    async fn timers_are_independent() {
        let clock = SystemClock::new();
        let start = clock.now();
        let long = clock.timer(TICK * 4);
        let short = clock.timer(TICK);

        short.await;
        long.await;

        // Windows timer granularity is tens of milliseconds, so the two deadlines can land in
        // the same tick and only the later one is asserted.
        assert!(clock.now() - start >= TICK * 4, "both deadlines honored");
    }

    #[tokio::test]
    async fn dropping_a_timer_abandons_it() {
        let clock = SystemClock::new();
        drop(clock.timer(TICK));

        clock.timer(TICK * 2).await;
        assert!(clock.now() >= TICK * 2);
    }
}
