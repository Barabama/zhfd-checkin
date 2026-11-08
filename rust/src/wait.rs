//! Bounded polling shared by the staged device-readiness waits.
//!
//! Device preparation has to wait for four different things in sequence: the
//! instance appearing in `ldconsole list2`, the VM reaching `running`, ADB
//! accepting the serial, and Android finishing its boot. Each stage has its own
//! budget and its own failure, so the loop itself lives here once, with the
//! clock and the sleep injectable — a unit test can then replay "not ready,
//! not ready, ready" without sleeping in real time.
//!
//! A probe error is deliberately *not* fatal. While the guest boots, `adb shell`
//! itself can be unavailable for tens of seconds; treating that as a failure
//! would turn a normal boot race into a hard device error. The error is recorded
//! instead, so a wait that never succeeds can still report why.

use anyhow::Result;
use std::time::{Duration, Instant};

/// Result of a bounded wait plus the poll statistics the caller needs for the
/// `wait_finished` event.
///
/// `value` follows `Result` semantics so the caller writes `outcome.value?` and
/// the classified failure propagates unchanged, while `attempts` stays readable
/// on the failure path too.
#[derive(Debug)]
pub(crate) struct WaitOutcome<T> {
    pub value: Result<T>,
    pub attempts: u32,
}

impl<T> WaitOutcome<T> {
    pub(crate) fn ok(value: T, attempts: u32) -> Self {
        Self {
            value: Ok(value),
            attempts,
        }
    }

    pub(crate) fn failed(error: anyhow::Error, attempts: u32) -> Self {
        Self {
            value: Err(error),
            attempts,
        }
    }

    pub(crate) fn is_ok(&self) -> bool {
        self.value.is_ok()
    }
}

/// Statistics of one [`poll_until`] call.
#[derive(Debug, Default)]
pub(crate) struct PollStats {
    pub attempts: u32,
    /// How many probes ran without an I/O error. Zero means the underlying tool
    /// never answered, which is a different failure from "answered not-ready".
    pub successful: u32,
    pub last_error: Option<String>,
}

/// Poll `probe` until it yields `Some(value)` or the budget is spent.
///
/// Exactly one probe runs for a zero budget, so the current state is always
/// observed instead of being silently reported as "not ready". Once the
/// deadline has been reached no further probe is issued: a probe is not free
/// (the boot check shells out three times), so spending one after the budget
/// both overruns the declared wait and could report success for a device that
/// only became ready after the operator's deadline. The final sleep is clamped
/// to the remaining budget, so the deadline is never overshot by an interval.
pub(crate) fn poll_until<T, P>(
    timeout: Duration,
    interval: Duration,
    now: impl Fn() -> Instant,
    sleep: impl Fn(Duration),
    mut probe: P,
) -> (Option<T>, PollStats)
where
    P: FnMut() -> Result<Option<T>>,
{
    let deadline = now() + timeout;
    let mut stats = PollStats::default();
    loop {
        // The first probe always runs, even with a spent budget.
        if stats.attempts > 0 && now() >= deadline {
            return (None, stats);
        }
        stats.attempts += 1;
        match probe() {
            Ok(Some(value)) => {
                stats.successful += 1;
                return (Some(value), stats);
            }
            Ok(None) => stats.successful += 1,
            Err(error) => stats.last_error = Some(format!("{error:#}")),
        }
        let current = now();
        if current >= deadline {
            return (None, stats);
        }
        // A zero interval would spin the CPU for the whole budget.
        let step = interval
            .max(Duration::from_millis(1))
            .min(deadline - current);
        sleep(step);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::Cell;

    /// Clock that only moves when the test sleeps, so no real time passes.
    struct FakeClock {
        base: Instant,
        elapsed: Cell<Duration>,
    }

    impl FakeClock {
        fn new() -> Self {
            Self {
                base: Instant::now(),
                elapsed: Cell::new(Duration::ZERO),
            }
        }

        fn now(&self) -> Instant {
            self.base + self.elapsed.get()
        }

        fn sleep(&self, duration: Duration) {
            self.elapsed.set(self.elapsed.get() + duration);
        }
    }

    #[test]
    fn poll_returns_the_first_value_and_stops_sleeping() {
        let clock = FakeClock::new();
        let answers = [None, None, Some(7)];
        let mut index = 0;
        let (found, stats) = poll_until(
            Duration::from_secs(10),
            Duration::from_secs(1),
            || clock.now(),
            |duration| clock.sleep(duration),
            || {
                let answer = answers[index];
                index += 1;
                Ok(answer)
            },
        );
        assert_eq!(found, Some(7));
        assert_eq!(stats.attempts, 3);
        assert_eq!(stats.successful, 3);
        // Two one-second sleeps; the probe that succeeded never slept.
        assert_eq!(clock.elapsed.get(), Duration::from_secs(2));
    }

    #[test]
    fn poll_gives_up_when_the_budget_is_spent_and_keeps_the_last_error() {
        let clock = FakeClock::new();
        let (found, stats) = poll_until::<(), _>(
            Duration::from_secs(3),
            Duration::from_secs(2),
            || clock.now(),
            |duration| clock.sleep(duration),
            || Err(anyhow::anyhow!("adb shell 不可用")),
        );
        assert!(found.is_none());
        assert_eq!(stats.successful, 0, "一次成功的探测都没有");
        assert!(stats.last_error.as_deref().unwrap().contains("adb shell"));
        // The last sleep is clamped to the remaining budget.
        assert_eq!(clock.elapsed.get(), Duration::from_secs(3));
    }

    #[test]
    fn poll_with_a_zero_budget_still_probes_once() {
        let clock = FakeClock::new();
        let mut calls = 0;
        let (found, stats) = poll_until(
            Duration::ZERO,
            Duration::from_secs(1),
            || clock.now(),
            |duration| clock.sleep(duration),
            || {
                calls += 1;
                Ok(None::<()>)
            },
        );
        assert!(found.is_none());
        assert_eq!(calls, 1);
        assert_eq!(stats.attempts, 1);
        assert_eq!(clock.elapsed.get(), Duration::ZERO);
    }

    #[test]
    fn poll_counts_successful_probes_separately_from_failed_ones() {
        let clock = FakeClock::new();
        let mut call = 0;
        let (found, stats) = poll_until::<(), _>(
            Duration::from_secs(3),
            Duration::from_secs(1),
            || clock.now(),
            |duration| clock.sleep(duration),
            || {
                call += 1;
                match call {
                    1 => Err(anyhow::anyhow!("transient")),
                    _ => Ok(None),
                }
            },
        );
        assert!(found.is_none());
        assert!(stats.successful > 0, "后续探测成功，不能报告成工具不可用");
        // Probes at 0 s, 1 s and 2 s; at 3 s the budget is spent and no fourth
        // probe is issued.
        assert_eq!(stats.attempts, 3);
    }
}
