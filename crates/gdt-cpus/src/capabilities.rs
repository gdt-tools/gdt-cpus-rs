//! Querying what the priority ladder can actually deliver right now.
//!
//! On Windows and macOS all seven [`ThreadPriority`] levels are always
//! distinct. On Linux, negative nice needs privilege - without it (and
//! without rtkit) `AboveNormal`, `Highest` and `TimeCritical` all resolve to
//! `nice(0)`, i.e. `Normal`. [`priority_capabilities`] predicts the outcome
//! so an engine can pick its threading strategy up front instead of
//! discovering the collapse from frame times.

use crate::ThreadPriority;

/// What each [`ThreadPriority`] level will effectively deliver, as opaque
/// ranks. Returned by [`priority_capabilities`].
///
/// `effective_rank[level as usize]` grows with effective strength; the
/// absolute numbers carry no meaning beyond ordering. Two levels with equal
/// rank currently resolve to the same scheduler behavior.
///
/// The snapshot is point-in-time: it reflects this process's rlimits and the
/// reachability of the system's priority broker (rtkit) at the moment of the
/// call. rtkit in particular can silently withdraw cooperation later (its
/// watchdog demotes a process that starves the canary), so treat the result
/// as a planning hint, not a contract.
#[must_use = "priority_capabilities() has no side effect; its return value is the whole point -- \
              inspect distinct()/rank() to plan the threading strategy"]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PriorityCaps {
    /// Effective strength rank per level, indexed by `ThreadPriority as usize`
    /// (`Background` = 0 … `TimeCritical` = 6).
    pub effective_rank: [u8; 7],
}

impl PriorityCaps {
    /// The effective strength rank of `priority` (higher = stronger).
    #[must_use]
    pub fn rank(&self, priority: ThreadPriority) -> u8 {
        self.effective_rank[priority as usize]
    }

    /// `true` when `a` and `b` currently resolve to different scheduler
    /// behavior. `distinct(Highest, Normal) == false` is the classic
    /// unprivileged-Linux-without-rtkit signal: your render thread will NOT
    /// outrank your workers, plan accordingly.
    #[must_use]
    pub fn distinct(&self, a: ThreadPriority, b: ThreadPriority) -> bool {
        self.rank(a) != self.rank(b)
    }

    /// Number of effectively distinct levels (7 = the full ladder works).
    #[must_use]
    pub fn distinct_levels(&self) -> u8 {
        let mut ranks: Vec<u8> = self.effective_rank.to_vec();

        ranks.sort_unstable();
        ranks.dedup();

        ranks.len() as u8
    }
}

/// Predicts what each [`ThreadPriority`] level will resolve to under this
/// process's current privileges. Touches no thread state.
///
/// Linux: computed from `RLIMIT_NICE` plus the rtkit daemon's `MinNiceLevel`
/// when the daemon is reachable (feature `rtkit`). Windows and macOS: all
/// seven levels are always distinct.
///
/// ```
/// use gdt_cpus::{ThreadPriority, priority_capabilities};
///
/// let caps = priority_capabilities();
/// if !caps.distinct(ThreadPriority::Highest, ThreadPriority::Normal) {
///     eprintln!("priority is flat here - consider promote_thread_to_realtime() for the audio feeder");
/// }
/// ```
pub fn priority_capabilities() -> PriorityCaps {
    #[cfg(any(target_os = "linux", target_os = "android"))]
    {
        crate::platform::linux::capabilities::priority_capabilities()
    }
    #[cfg(not(any(target_os = "linux", target_os = "android")))]
    {
        PriorityCaps {
            effective_rank: [0, 1, 2, 3, 4, 5, 6],
        }
    }
}
