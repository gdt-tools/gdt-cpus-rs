//! Thread affinity and priority control for the CURRENT thread.
//!
//! [`pin_thread_to_core`] / [`set_thread_affinity`] set hard CPU affinity
//! (Linux, Windows; macOS returns [`crate::Error::Unsupported`] - Apple
//! Silicon ignores affinity, QoS via [`set_thread_priority`] is the only
//! placement tool there). [`set_thread_priority`] maps the 7 portable
//! [`ThreadPriority`] levels onto each OS scheduler.

use crate::{AffinityMask, AppliedPriority, ThreadPriority, error::Result};

/// Pins the current thread to a single logical core (OS LP id).
///
/// Convenience over [`set_thread_affinity`] with a one-bit mask. Logical core
/// IDs are the OS's own ids - get them from
/// [`crate::CpuInfo::logical_processor_ids()`] or the mask helpers
/// ([`crate::CpuInfo::performance_core_mask()`] etc.).
///
/// # Examples
///
/// ```
/// use gdt_cpus::{CpuInfo, pin_thread_to_core};
///
/// if let Ok(info) = CpuInfo::detect() {
///     if let Some(&first) = info.logical_processor_ids().first() {
///         if let Err(e) = pin_thread_to_core(first) {
///             eprintln!("pin failed: {}", e);
///         }
///     }
/// }
/// ```
pub fn pin_thread_to_core(logical_core_id: usize) -> Result<()> {
    set_thread_affinity(&AffinityMask::single(logical_core_id))
}

/// Sets the current thread's hard CPU affinity to `mask` (OS LP ids).
///
/// Linux: `sched_setaffinity`. Windows: `SetThreadGroupAffinity` - a thread's
/// hard affinity is single-group by OS design, so masks spanning multiple
/// 64-LP processor groups return [`crate::Error::InvalidParameter`].
/// macOS and other platforms: [`crate::Error::Unsupported`].
pub fn set_thread_affinity(mask: &AffinityMask) -> Result<()> {
    #[cfg(target_os = "windows")]
    {
        crate::platform::windows::affinity::set_thread_affinity(mask)
    }
    #[cfg(any(target_os = "linux", target_os = "android"))]
    {
        crate::platform::linux::affinity::set_thread_affinity(mask)
    }
    #[cfg(not(any(target_os = "windows", target_os = "linux", target_os = "android")))]
    {
        let _ = mask;
        Err(crate::Error::Unsupported(
            "Thread affinity is not supported on this platform.".to_string(),
        ))
    }
}

/// Reads the current thread's hard CPU affinity as an [`AffinityMask`].
///
/// Linux: `sched_getaffinity`. Windows: the thread's `GROUP_AFFINITY` (a
/// thread's hard affinity is single-group by OS design, so the mask carries
/// bits from one 64-LP processor group). macOS and other platforms:
/// [`crate::Error::Unsupported`] (Apple Silicon ignores affinity).
///
/// Pairs with [`set_thread_affinity`] for save/restore - read the current mask,
/// pin to a subset, then restore.
///
/// # Examples
///
/// ```
/// # #[cfg(any(target_os = "linux", target_os = "windows"))]
/// # {
/// use gdt_cpus::current_affinity;
///
/// if let Ok(mask) = current_affinity() {
///     println!("running on {} logical processors", mask.count());
/// }
/// # }
/// ```
pub fn current_affinity() -> Result<AffinityMask> {
    #[cfg(target_os = "windows")]
    {
        crate::platform::windows::affinity::current_affinity()
    }
    #[cfg(any(target_os = "linux", target_os = "android"))]
    {
        crate::platform::linux::affinity::current_affinity()
    }
    #[cfg(not(any(target_os = "windows", target_os = "linux", target_os = "android")))]
    {
        Err(crate::Error::Unsupported(
            "Reading thread affinity is not supported on this platform.".to_string(),
        ))
    }
}

/// Sets the current thread's SOFT affinity to `mask` (OS LP ids) - Windows only.
///
/// Soft affinity (the CPU Sets API, `SetThreadSelectedCpuSets`) tells the
/// scheduler to PREFER the given LPs while still allowing migration under
/// contention - the mode Intel's game guidance recommends over hard masks
/// (it cooperates with Thread Director and core parking instead of fighting
/// them), and the only cross-processor-group placement tool. Returns
/// [`crate::Error::Unsupported`] on every other platform.
pub fn set_thread_soft_affinity(mask: &AffinityMask) -> Result<()> {
    #[cfg(target_os = "windows")]
    {
        crate::platform::windows::affinity::set_thread_soft_affinity(mask)
    }
    #[cfg(not(target_os = "windows"))]
    {
        let _ = mask;
        Err(crate::Error::Unsupported(
            "Soft affinity (CPU Sets) is a Windows API; there is no equivalent here.".to_string(),
        ))
    }
}

/// Sets the current thread's priority.
///
/// Mapping per OS (full tables on [`ThreadPriority`] and in the platform
/// modules): Linux - pure timeshare nice 19/10/5/0/-5/-10/-20; denied
/// negative values cascade to rtkit (feature `rtkit`, on by default) and
/// finally to nice 0 - real-time is the separate opt-in
/// [`crate::promote_thread_to_realtime`]. macOS - QoS classes for every
/// level except `TimeCritical`, which is `SCHED_RR` 47 (a permanent QoS
/// opt-out). Windows - `THREAD_PRIORITY_IDLE..TIME_CRITICAL`.
///
/// Returns an [`AppliedPriority`] describing what the request actually
/// produced - on Linux a "success" can mean the level was granted directly,
/// brokered through rtkit, or silently degraded to `Normal`. Branch on
/// [`AppliedPriority::degraded`] / [`crate::Grant`], or `Display` it for the
/// platform mechanism string.
///
/// # Examples
///
/// ```
/// use gdt_cpus::{ThreadPriority, set_thread_priority};
///
/// match set_thread_priority(ThreadPriority::Highest) {
///     Ok(applied) if applied.degraded() => {
///         eprintln!("priority silently weakened: {}", applied);
///     }
///     Ok(applied) => println!("priority set: {}", applied),
///     Err(e) => eprintln!("priority change failed: {}", e),
/// }
/// ```
pub fn set_thread_priority(priority: ThreadPriority) -> Result<AppliedPriority> {
    #[cfg(target_os = "windows")]
    {
        crate::platform::windows::affinity::set_thread_priority(priority)
    }
    #[cfg(any(target_os = "linux", target_os = "android"))]
    {
        crate::platform::linux::affinity::set_thread_priority(priority)
    }
    #[cfg(target_os = "macos")]
    {
        crate::platform::macos::affinity::set_thread_priority(priority)
    }
    #[cfg(not(any(
        target_os = "windows",
        target_os = "linux",
        target_os = "android",
        target_os = "macos"
    )))]
    {
        let _ = priority;
        Err(crate::Error::Unsupported(
            "Setting thread priority is not supported on this platform.".to_string(),
        ))
    }
}
