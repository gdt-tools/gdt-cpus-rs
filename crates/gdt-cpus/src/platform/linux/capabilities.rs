//! Linux half of [`crate::priority_capabilities`] - predicts what each
//! [`ThreadPriority`](crate::ThreadPriority) level will RESOLVE to under the
//! current process's privileges, without touching any thread.
//!
//! The prediction mirrors the `set_thread_priority` cascade: a nice value is
//! reachable directly when it is non-negative or within the `RLIMIT_NICE`
//! floor; otherwise rtkit grants it clamped to the daemon's `MinNiceLevel`;
//! otherwise the cascade lands on `nice(0)`.

use crate::PriorityCaps;

/// The nice ladder, indexed by `ThreadPriority as usize`
/// (Background..TimeCritical). Must match `scheduling_policy.rs`.
const NICE_LADDER: [i32; 7] = [19, 10, 5, 0, -5, -10, -20];

pub(crate) fn priority_capabilities() -> PriorityCaps {
    let nice_floor = nice_floor_from_rlimit();

    #[cfg(all(feature = "rtkit", target_os = "linux"))]
    let rtkit_min_nice = crate::platform::linux::rtkit::Broker::rtkit()
        .ok()
        .map(|mut broker| broker.min_nice_level().clamp(-20, 0) as i32);
    #[cfg(not(all(feature = "rtkit", target_os = "linux")))]
    let rtkit_min_nice: Option<i32> = None;

    PriorityCaps {
        effective_rank: effective_ranks(nice_floor, rtkit_min_nice),
    }
}

/// The most negative nice this process may set directly: `20 - rlim_cur`
/// per setpriority(2). The default `rlim_cur = 0` yields a floor of 20 -
/// no negative nice at all (clamped into the valid nice range below only
/// for arithmetic; any floor above 19 means "non-negative only").
fn nice_floor_from_rlimit() -> i32 {
    let mut limit = libc::rlimit {
        rlim_cur: 0,
        rlim_max: 0,
    };

    // SAFETY: getrlimit with a valid resource constant and a valid out-pointer.
    if unsafe { libc::getrlimit(libc::RLIMIT_NICE, &mut limit) } != 0 {
        return 20; // unreadable limit - assume no privilege
    }

    let cur = limit.rlim_cur.min(40) as i32;

    20 - cur
}

/// Pure rank computation over injected floors - the unit-testable core.
///
/// `effective_rank[level]` counts the distinct effective strengths strictly
/// weaker than `level`'s; equal ranks mean two levels currently resolve to
/// the same scheduler behavior.
fn effective_ranks(nice_floor: i32, rtkit_min_nice: Option<i32>) -> [u8; 7] {
    let effective = NICE_LADDER.map(|nice| effective_nice(nice, nice_floor, rtkit_min_nice));

    // Distinct effective values, weakest (highest nice) first.
    let mut distinct: Vec<i32> = effective.to_vec();
    distinct.sort_unstable_by(|a, b| b.cmp(a));
    distinct.dedup();

    effective.map(|nice| distinct.iter().position(|&v| v == nice).unwrap() as u8)
}

fn effective_nice(nice: i32, nice_floor: i32, rtkit_min_nice: Option<i32>) -> i32 {
    if nice >= 0 || nice >= nice_floor {
        // Raising nice always works; lowering works within the rlimit floor.
        nice
    } else if let Some(min_nice) = rtkit_min_nice {
        // rtkit grants the request clamped to its MinNiceLevel.
        nice.max(min_nice)
    } else {
        // The cascade's last resort.
        0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unprivileged_no_rtkit_collapses_the_top() {
        // Default desktop without rtkit: floor 20, negative rungs land on 0.
        let ranks = effective_ranks(20, None);
        // 19, 10, 5, then {0, -5 -> 0, -10 -> 0, -20 -> 0} all rank equal.
        assert_eq!(ranks, [0, 1, 2, 3, 3, 3, 3]);
    }

    #[test]
    fn unprivileged_with_rtkit_keeps_seven_distinct() {
        // rtkit default MinNiceLevel -15: -20 clamps to -15, still distinct.
        let ranks = effective_ranks(20, Some(-15));
        assert_eq!(ranks, [0, 1, 2, 3, 4, 5, 6]);
    }

    #[test]
    fn privileged_full_ladder() {
        let ranks = effective_ranks(-20, None);
        assert_eq!(ranks, [0, 1, 2, 3, 4, 5, 6]);
    }

    #[test]
    fn partial_rlimit_collapses_only_what_it_must() {
        // RLIMIT_NICE grants down to -10: Highest works directly, but
        // TimeCritical (-20) exceeds the floor and falls back to nice 0 -
        // landing it BELOW AboveNormal/Highest and equal to Normal.
        let ranks = effective_ranks(-10, None);
        assert_eq!(ranks, [0, 1, 2, 3, 4, 5, 3]);
        let caps = PriorityCaps {
            effective_rank: ranks,
        };
        assert!(!caps.distinct(
            crate::ThreadPriority::Normal,
            crate::ThreadPriority::TimeCritical
        ));
        assert!(caps.distinct(
            crate::ThreadPriority::Normal,
            crate::ThreadPriority::Highest
        ));
    }
}
