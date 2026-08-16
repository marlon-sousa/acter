//! Adapter: `SystemClock` implements the `Clock` driven port over the real monotonic
//! clock. The `Instant` stops here — the domain is handed `Duration`s measured from an
//! origin captured at construction, so nothing downstream can reach the wall clock or
//! depend on when the process started (spec B1.5, decision 3).
//!
//! This is the crate's one component that exists ahead of its consumer: no actor is
//! spawned until the real session service lands. It is written now anyway, because a
//! port whose only implementer is a test fake is a port shaped for nobody — the mistake
//! B1's spec named when it deferred declaring traits. Being `pub` in the facade is what
//! keeps it from reading as dead code while it waits.

use std::time::{Duration, Instant};

use acter_core::{Clock, Timer};
use tokio::sync::oneshot;

/// The real clock. `now()` counts from construction, which is close enough to session
/// start for every deadline the pacing policy sets, and is never compared against a
/// reading from a different clock.
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
    /// `Instant::elapsed` is monotonic, so this satisfies the port's "never decreases".
    fn now(&self) -> Duration {
        self.origin.elapsed()
    }

    /// Must be called from within a tokio runtime — always true of the session actor,
    /// which runs as a task on the runtime Tauri owns. Dropping the returned [`Timer`]
    /// drops the receiver, so a re-armed deadline needs no explicit cancellation: the
    /// orphaned task finds nobody listening and its send is discarded.
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

    /// The one place in the workspace that touches real time: `acter-core` never does,
    /// and this adapter exists precisely to. The waits are awaited rather than slept
    /// through, so they are as short as the clock allows rather than padded guesses.
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
        // Armed together, so a second timer must not satisfy the first, nor cancel it.
        let long = clock.timer(TICK * 4);
        let short = clock.timer(TICK);

        short.await;
        long.await;

        // Deliberately not an assertion about the gap *between* them: OS timer
        // granularity is tens of milliseconds on Windows, so two short deadlines can
        // genuinely land in the same tick. What must hold is that the later one was
        // still honored rather than collapsed into the earlier.
        assert!(clock.now() - start >= TICK * 4, "both deadlines honored");
    }

    #[tokio::test]
    async fn dropping_a_timer_abandons_it() {
        let clock = SystemClock::new();
        drop(clock.timer(TICK));

        // Re-arming is how the actor cancels: it drops the old timer and asks for a new
        // one. The orphaned wake must not resurface as a spurious completion, and the
        // dropped receiver must not take the runtime down with it.
        clock.timer(TICK * 2).await;
        assert!(clock.now() >= TICK * 2);
    }
}
