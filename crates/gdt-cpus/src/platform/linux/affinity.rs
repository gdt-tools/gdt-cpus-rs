//! Linux-specific thread affinity and priority management.
//!
//! This module provides functions to control thread affinity and thread priority
//! (via `nice` values under `SCHED_OTHER`) on Linux systems.
//!
//! **Important Notes for Linux:**
//! - **Thread Affinity:** Uses `sched_setaffinity` to restrict the current thread
//!   to a set of logical cores specified by an [`AffinityMask`]. The mask indices
//!   are mapped to OS-level logical processor IDs.
//! - **Thread Priority:** adjusts the thread's `nice` value with `setpriority`.
//!   Lower `nice` values (e.g., -20) mean higher priority. Negative values need
//!   privilege (`CAP_SYS_NICE` or a raised `RLIMIT_NICE`); when denied, the
//!   cascade asks rtkit for the negative nice (feature `rtkit`) and finally,
//!   if no broker delivers it, keeps the level the thread already has (reported
//!   as data, never an error). Real-time scheduling (`SCHED_RR`) is reachable
//!   only through the consent API ([`crate::promote_thread_to_realtime`]).
//!
//! The main functions provided are [`set_thread_affinity`] and [`set_thread_priority`].

use libc::{SYS_gettid, c_int, syscall};

use crate::{
    AffinityMask, AppliedPriority, BrokerError, Error, FallbackReason, Grant, Mechanism,
    MechanismPolicy, Result, ThreadPriority,
    platform::linux::scheduling_policy::{level_for_nice, nice_for},
};

/// Sets the CPU affinity of the current thread on Linux.
///
/// This function restricts the current thread to execute only on the logical
/// cores specified in the [`AffinityMask`]. It uses `libc::sched_setaffinity`
/// to apply the affinity mask.
///
/// # Arguments
///
/// * `mask`: An [`AffinityMask`] specifying which logical cores the thread may run on.
///   Core indices in the mask are mapped to OS-level logical processor IDs.
///
/// # Returns
///
/// - `Ok(())` if the affinity was successfully set.
/// - `Error::Affinity` if the mask is empty, no valid cores could be added, or
///   `sched_setaffinity` fails.
/// - Errors from [`crate::CpuInfo::detect()`] if CPU information cannot be retrieved.
///
/// # Safety
///
/// This function uses `unsafe` blocks for FFI calls to `libc::sched_setaffinity`,
/// `libc::CPU_ZERO`, and `libc::CPU_SET`. These are standard Linux system calls
/// and are safe when provided with valid arguments (a valid `cpu_set_t` and a TID of 0
/// for the current thread).
pub(crate) fn set_thread_affinity(mask: &AffinityMask) -> Result<()> {
    if mask.is_empty() {
        return Err(Error::Affinity(
            "Cannot set thread affinity with an empty mask".to_string(),
        ));
    }

    // NOTE: no topology lookup here - validating against detected LPs forced a
    // full (cached) detection inside an affinity call. The kernel validates
    // membership itself and returns EINVAL for CPUs outside the allowed set;
    // we only bound-check against cpu_set_t's capacity.
    let max_cpus = libc::CPU_SETSIZE as usize;

    // SAFETY: Zero-initializes the cpu_set_t structure.
    // cpu_set_t is POD without non-zeroable invariants; zeroing yields a valid struct.
    let mut cpuset: libc::cpu_set_t = unsafe { std::mem::zeroed() };
    // SAFETY: CPU_ZERO is safe to call with a valid cpu_set_t pointer.
    unsafe {
        libc::CPU_ZERO(&mut cpuset);
    }

    // Set all cores in the mask
    for core_idx in mask.iter() {
        if core_idx < max_cpus {
            // SAFETY: CPU_SET is safe with a valid cpu_set_t pointer and valid CPU index.
            unsafe {
                libc::CPU_SET(core_idx, &mut cpuset);
            }
        } else {
            return Err(Error::Affinity(format!(
                "OS core {} exceeds CPU_SETSIZE {}",
                core_idx, max_cpus
            )));
        }
    }

    // SAFETY: sched_setaffinity is a system call that sets the CPU affinity for the calling thread.
    // pid == 0 means the calling thread, and the size of the cpu_set_t is passed.
    let res =
        unsafe { libc::sched_setaffinity(0, std::mem::size_of::<libc::cpu_set_t>(), &cpuset) };

    if res == -1 {
        let err = std::io::Error::last_os_error();
        Err(map_sched_setaffinity_error(err))
    } else {
        Ok(())
    }
}

fn map_sched_setaffinity_error(err: std::io::Error) -> Error {
    match err.raw_os_error() {
        Some(libc::EINVAL) => Error::InvalidParameter(format!(
            "Invalid affinity mask for sched_setaffinity: {}",
            err
        )),
        _ => Error::Affinity(format!("sched_setaffinity failed: {}", err)),
    }
}

/// Reads the current thread's CPU affinity into an [`AffinityMask`] via
/// `sched_getaffinity(0)`.
///
/// # Returns
///
/// - `Ok(mask)` with one bit set per OS LP the thread may run on.
/// - `Error::Affinity` if `sched_getaffinity` fails.
pub(crate) fn current_affinity() -> Result<AffinityMask> {
    // SAFETY: cpu_set_t is POD; zeroing yields a valid (empty) set.
    let mut cpuset: libc::cpu_set_t = unsafe { std::mem::zeroed() };

    // SAFETY: sched_getaffinity fills `cpuset` for the calling thread (pid 0).
    let res =
        unsafe { libc::sched_getaffinity(0, std::mem::size_of::<libc::cpu_set_t>(), &mut cpuset) };
    if res == -1 {
        let err = std::io::Error::last_os_error();
        return Err(Error::Affinity(format!("sched_getaffinity failed: {err}")));
    }

    let mut mask = AffinityMask::empty();
    for cpu in 0..(libc::CPU_SETSIZE as usize) {
        // SAFETY: CPU_ISSET is safe with a valid cpu_set_t pointer and an index
        // below CPU_SETSIZE.
        if unsafe { libc::CPU_ISSET(cpu, &cpuset) } {
            mask.add(cpu);
        }
    }

    Ok(mask)
}

/// Returns the current thread's kernel TID (the id `setpriority` and rtkit
/// address threads by).
pub(crate) fn current_tid() -> Result<libc::id_t> {
    // SAFETY: syscall(SYS_gettid) is the standard way to get the current thread ID on Linux.
    // Returns -1 on error.
    let tid = unsafe { syscall(SYS_gettid) };
    if tid <= 0 {
        let err = std::io::Error::last_os_error();
        return Err(Error::SystemCall(format!(
            "Failed to get thread ID via gettid(): {}",
            err
        )));
    }

    Ok(tid as libc::id_t)
}

/// The current thread's `nice` value, via a raw `getpriority` syscall. The
/// kernel returns `20 - nice` (always positive, sidestepping the cooked
/// wrapper's `-1`/`errno` ambiguity) - decode it. Used to report the level a
/// thread ACTUALLY sits at when a priority request could not be applied.
///
/// # Errors
///
/// [`Error::SystemCall`] if `getpriority` fails (only a missing TID, which
/// cannot happen for the calling thread).
pub(crate) fn current_nice() -> Result<c_int> {
    let tid = current_tid()?;

    // SAFETY: raw getpriority for the current thread's TID; returns 20 - nice on
    // success, -1 (with errno) on failure.
    let rc = unsafe { libc::syscall(libc::SYS_getpriority, libc::PRIO_PROCESS, tid as c_int) };
    if rc < 0 {
        let err = std::io::Error::last_os_error();
        return Err(Error::SystemCall(format!(
            "getpriority for TID {}: {}",
            tid, err
        )));
    }

    Ok(20 - rc as c_int)
}

/// Sets the `nice` value for the current thread on Linux - one direct
/// `setpriority` call, no fallback (the cascade lives in
/// [`set_thread_priority`]).
///
/// A lower `nice` value means higher priority. Raising the value (lowering
/// priority) always succeeds; lowering it requires `CAP_SYS_NICE` or a raised
/// `RLIMIT_NICE`. NOTE(linux): the errno for an unprivileged negative-nice
/// attempt is `EACCES`, not `EPERM` (see setpriority(2)) - callers must match
/// both. The check compares against the thread's CURRENT nice, so even
/// `nice(0)` can be denied for a thread sitting at a positive value (the
/// one-way ratchet) - the rtkit step of the cascade covers that case too.
///
/// # Safety
///
/// Uses `unsafe` for `syscall(SYS_gettid)` and `libc::setpriority` - standard
/// Linux system calls, safe with a valid TID and an in-range nice value.
fn set_thread_nice_value(nice_value: c_int) -> Result<()> {
    let tid = current_tid()?;

    // SAFETY: setpriority is used to set the nice value for a specific thread (using tid).
    // tid is a valid thread ID, and nice_value is the expected c_int value.
    let res = unsafe { libc::setpriority(libc::PRIO_PROCESS, tid, nice_value) };

    if res == -1 {
        let err = std::io::Error::last_os_error();
        match err.raw_os_error() {
            Some(libc::EACCES | libc::EPERM) => Err(Error::PermissionDenied(format!(
                "Setting nice value {} for TID {}: {}",
                nice_value, tid, err
            ))),
            Some(libc::ESRCH) => Err(Error::NotFound(format!(
                "Thread with TID {} not found for setpriority: {}",
                tid, err
            ))),
            Some(libc::EINVAL) => Err(Error::InvalidParameter(format!(
                "Invalid nice value {} for setpriority",
                nice_value
            ))),
            _ => Err(Error::SystemCall(format!(
                "setpriority failed for nice value {} for TID {}: {}",
                nice_value, tid, err
            ))),
        }
    } else {
        Ok(())
    }
}

/// Sets the priority of the current thread on Linux.
///
/// Every [`ThreadPriority`] level maps to a `nice` value under `SCHED_OTHER`
/// (table in `scheduling_policy.rs`) - the default path never requests a
/// real-time class. For values the kernel denies (negative nice without
/// privilege, or the one-way ratchet on a thread sitting above its target),
/// the cascade is:
///
/// 1. direct `setpriority` - works for non-negative values, and for negative
///    ones under `CAP_SYS_NICE` / a raised `RLIMIT_NICE`;
/// 2. rtkit `MakeThreadHighPriority` (feature `rtkit`, on by default) - the
///    desktop's broker for exactly this, leash-free (the thread stays
///    `SCHED_OTHER`, so rtkit's `RLIMIT_RTTIME`/SIGKILL enforcement never
///    applies); the request is clamped to the daemon's `MinNiceLevel`
///    (default -15);
/// 3. **keep the level you have** - a denied request leaves the thread's nice
///    untouched (a denied `setpriority` changes nothing), so the result reports
///    the level the thread ACTUALLY sits at with [`FallbackReason::NoBroker`].
///    A mere permission denial is therefore never an error - uniformly with the
///    other platforms' best-effort semantics. Only a genuine `setpriority`
///    failure (a kernel error other than the permission denial) is an error.
///
/// # Errors
///
/// [`Error::SystemCall`] / [`Error::InvalidParameter`] / [`Error::NotFound`]
/// for a `setpriority` failure that is NOT a permission denial. A permission
/// denial is reported in the returned [`AppliedPriority`] (degraded to the
/// current level), not as an error.
pub(crate) fn set_thread_priority(priority: ThreadPriority) -> Result<AppliedPriority> {
    // Map the portable level to its nice value (table in scheduling_policy.rs).
    let value = nice_for(priority);
    match set_thread_nice_value(value) {
        Ok(()) => Ok(AppliedPriority::new(
            priority,
            priority,
            Grant::Direct,
            Mechanism {
                policy: MechanismPolicy::Nice,
                value: value as i8,
            },
        )),
        Err(Error::PermissionDenied(_)) => {
            // Why the stronger request failed - refined by the rtkit attempt.
            // Defaults to NoBroker for the feature-off / no-tid paths.
            #[cfg_attr(not(feature = "rtkit"), allow(unused_mut))]
            let mut reason = FallbackReason::NoBroker;

            // The typed reason the broker REFUSED, when it answered with a D-Bus
            // ERROR - carried as data (NOT free text) so a caller can branch on it.
            #[cfg_attr(not(feature = "rtkit"), allow(unused_mut))]
            let mut broker_error: Option<BrokerError> = None;

            #[cfg(all(feature = "rtkit", not(target_os = "android")))]
            {
                if let Ok(tid) = current_tid() {
                    match crate::platform::linux::rtkit::try_high_priority(tid as u64, value) {
                        Ok(granted) if granted == value => {
                            return Ok(AppliedPriority::new(
                                priority,
                                priority,
                                Grant::Brokered,
                                Mechanism {
                                    policy: MechanismPolicy::Nice,
                                    value: granted as i8,
                                },
                            ));
                        }
                        // Granted, but weaker than asked (the broker's ceiling):
                        // keep the level, flag the clamp as a fall-short reason.
                        Ok(granted) => {
                            return Ok(AppliedPriority::new(
                                priority,
                                priority,
                                Grant::Brokered,
                                Mechanism {
                                    policy: MechanismPolicy::Nice,
                                    value: granted as i8,
                                },
                            )
                            .with_reason(FallbackReason::Clamped));
                        }
                        Err((r, be)) => {
                            reason = r;
                            broker_error = be;
                        }
                    }
                }
            }

            // No broker delivered it. Best-effort: a denied setpriority left the
            // thread's nice untouched, so report the level it ACTUALLY sits at
            // (NOT a hardcoded Normal - that could even DEMOTE a thread already
            // above normal) with WHY, as data. Never an error on a mere denial.
            let current = current_nice().unwrap_or(nice_for(ThreadPriority::Normal));

            // The mechanism is the nice the thread actually KEEPS; the structured
            // `reason` + `broker_error` carry the classification.
            let mut applied = AppliedPriority::new(
                priority,
                level_for_nice(current),
                Grant::Direct,
                Mechanism {
                    policy: MechanismPolicy::Nice,
                    value: current as i8,
                },
            )
            .with_reason(reason);

            if let Some(be) = broker_error {
                applied = applied.with_broker_error(be);
            }

            Ok(applied)
        }

        Err(other) => Err(other),
    }
}

/// Puts the current thread on `SCHED_RR` at `priority` - one direct
/// `pthread_setschedparam` call, no fallback. Used by the consent API
/// ([`crate::promote_thread_to_realtime`]); never by [`set_thread_priority`].
pub(crate) fn set_thread_realtime_rr(priority: c_int) -> Result<()> {
    // SAFETY: These functions are safe to call; SCHED_RR is a valid policy.
    // They return -1 on error (e.g., if the policy is not supported, which is unlikely for SCHED_RR).
    let (rt_min, rt_max) = unsafe {
        (
            libc::sched_get_priority_min(libc::SCHED_RR),
            libc::sched_get_priority_max(libc::SCHED_RR),
        )
    };

    if rt_min == -1 || rt_max == -1 {
        let err = std::io::Error::last_os_error();
        return Err(Error::SystemCall(format!(
            "Failed to get SCHED_RR priority range: {}",
            err
        )));
    }

    if priority < rt_min || priority > rt_max {
        return Err(Error::InvalidParameter(format!(
            "Absolute priority {} is out of range [{}, {}] for SCHED_RR",
            priority, rt_min, rt_max
        )));
    }

    // SAFETY: pthread_self() always returns a valid handle to the current thread.
    let current_thread = unsafe { libc::pthread_self() };

    // SAFETY: sched_param is a POD structure; zeroing it is safe.
    // sched_priority is then set to the validated priority.
    let mut param: libc::sched_param = unsafe { std::mem::zeroed() };
    param.sched_priority = priority;

    let policy = libc::SCHED_RR | libc::SCHED_RESET_ON_FORK;

    // SAFETY: Sets the SCHED_RR policy for the current thread.
    // current_thread is valid, param.sched_priority is within [rt_min, rt_max].
    let res = unsafe { libc::pthread_setschedparam(current_thread, policy, &param) };

    if res != 0 {
        let err = std::io::Error::from_raw_os_error(res); // res is errno in this case
        match res {
            libc::EPERM => Err(Error::PermissionDenied(format!(
                "Setting SCHED_RR with priority {}: {}",
                priority, err
            ))),
            libc::EINVAL => Err(Error::InvalidParameter(format!(
                "Invalid parameters for SCHED_RR: priority={}. Error: {}",
                priority, err
            ))),
            libc::ESRCH => Err(Error::NotFound(format!(
                "Thread not found for pthread_setschedparam. Error: {}",
                err
            ))),
            _ => Err(Error::SystemCall(format!(
                "pthread_setschedparam failed for SCHED_RR with priority {}. Error: {}",
                priority, err
            ))),
        }
    } else {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sched_setaffinity_einval_maps_to_invalid_parameter() {
        let err = std::io::Error::from_raw_os_error(libc::EINVAL);
        assert!(matches!(
            map_sched_setaffinity_error(err),
            Error::InvalidParameter(_)
        ));
    }
}
