//! Explicit real-time promotion - the consent API.
//!
//! [`set_thread_priority`](crate::set_thread_priority) is deliberately
//! timeshare-only on Linux: a real-time thread that spins owns its core
//! until the kernel's throttle or an `RLIMIT_RTTIME` SIGKILL intervenes, and
//! that trade-off belongs to the application, not to a library default.
//! These functions are the opt-in.

use std::time::Duration;

use crate::{AppliedPriority, Result};

/// Promotes the current thread to the platform's real-time tier.
///
/// `budget` is the thread's promise: the longest stretch it will compute
/// without blocking (an audio feeder filling a 5 ms buffer might promise
/// 1 ms). Keep it honest and small.
///
/// # Platform behavior
///
/// * **Linux** - tries, in order: direct `SCHED_RR` 85 (root, `CAP_SYS_NICE`,
///   raised `RLIMIT_RTPRIO`), the xdg realtime portal (the path that works
///   inside Flatpak), and rtkit (plain desktops, Steam's pressure-vessel).
///   The brokered paths require an `RLIMIT_RTTIME` and this function sets it:
///   soft = `budget` (delivers a catchable `SIGXCPU` warning), hard = the
///   daemon's ceiling (default 200 ms - delivers **SIGKILL to the whole
///   process**). The budget meter resets on every blocking syscall: being
///   late is fine, spinning forever is fatal. Lowering the hard limit is
///   irreversible and process-wide - that is the consent this call signs.
///   The granted RT priority over the brokered paths is the daemon's maximum
///   (default 20), not 85 - still above every timeshare thread.
/// * **macOS** - `SCHED_RR` 47 fixed priority (no decay), no privileges
///   needed, no leash; `budget` is unused. Per Apple's `qos.h` the thread
///   PERMANENTLY leaves the QoS system - dedicate it.
/// * **Windows** - `THREAD_PRIORITY_TIME_CRITICAL`; `budget` is unused, no
///   leash (the dynamic-priority band, not `REALTIME_PRIORITY_CLASS`).
///
/// A denied Linux promotion returns an [`AppliedPriority`] whose effective
/// level and mechanism report the timeshare state the thread kept, with
/// [`crate::FallbackReason`] and [`crate::BrokerError`] carrying the broker
/// result when one answered. Other platform failures that prevent even asking
/// for realtime remain [`crate::Error`] values.
///
/// # Do not install signal handlers for this
///
/// The library installs none. If you want the `SIGXCPU` warning on Linux,
/// handle it yourself (set a flag, then call
/// [`demote_thread_from_realtime`] from the thread) - self-demotion before
/// the hard limit is the recovery path, a library-owned handler would fight
/// the application's own signal management.
pub fn promote_thread_to_realtime(budget: Duration) -> Result<AppliedPriority> {
    #[cfg(any(target_os = "linux", target_os = "android"))]
    {
        crate::platform::linux::realtime::promote(budget)
    }
    #[cfg(target_os = "macos")]
    {
        let _ = budget;
        crate::platform::macos::affinity::promote_thread_to_realtime()
    }
    #[cfg(target_os = "windows")]
    {
        let _ = budget;
        crate::platform::windows::affinity::promote_thread_to_realtime()
    }
    #[cfg(not(any(
        target_os = "linux",
        target_os = "android",
        target_os = "macos",
        target_os = "windows"
    )))]
    {
        let _ = budget;
        Err(crate::Error::Unsupported(
            "Real-time promotion is not supported on this platform.".to_string(),
        ))
    }
}

/// Returns the current thread from the real-time tier to normal scheduling.
///
/// This is the self-demotion half of the consent: call it when the
/// real-time phase ends, or from your own watchdog when a `SIGXCPU` warning
/// fired. On Linux the thread goes back to `SCHED_OTHER` (the lowered hard
/// `RLIMIT_RTTIME` remains - irreversible - but stops mattering once no
/// thread runs real-time). On macOS, a thread that never entered fixed-priority
/// scheduling is set to QoS Normal; a thread that did enter `SCHED_RR` cannot
/// rejoin QoS and lands on the legacy `SCHED_OTHER` band at Normal strength.
/// On Windows it returns to `THREAD_PRIORITY_NORMAL`.
pub fn demote_thread_from_realtime() -> Result<()> {
    #[cfg(any(target_os = "linux", target_os = "android"))]
    {
        crate::platform::linux::realtime::demote()
    }
    #[cfg(target_os = "macos")]
    {
        crate::platform::macos::affinity::set_thread_priority(crate::ThreadPriority::Normal)
            .map(|_| ())
    }
    #[cfg(target_os = "windows")]
    {
        crate::platform::windows::affinity::set_thread_priority(crate::ThreadPriority::Normal)
            .map(|_| ())
    }
    #[cfg(not(any(
        target_os = "linux",
        target_os = "android",
        target_os = "macos",
        target_os = "windows"
    )))]
    {
        Err(crate::Error::Unsupported(
            "Real-time promotion is not supported on this platform.".to_string(),
        ))
    }
}
