//! Linux real-time promotion - the platform half of the consent API
//! ([`crate::promote_thread_to_realtime`] / [`crate::demote_thread_from_realtime`]).
//!
//! The chain, in order:
//! 1. **direct `SCHED_RR`** (root, `CAP_SYS_NICE`, or a raised `RLIMIT_RTPRIO`
//!    on RT-permissive distros) - unleashed: no `RLIMIT_RTTIME` is set, the
//!    thread answers to nobody but the kernel's global RT throttle;
//! 2. **the xdg realtime portal** (session bus) - the path that works inside
//!    Flatpak, where direct rtkit sees the sandbox's PID namespace;
//! 3. **rtkit `MakeThreadRealtime`** (system bus) - the plain desktop case
//!    (and Steam's pressure-vessel, which shares the PID namespace).
//!
//! Steps 2-3 are brokered and LEASHED: rtkit checks the process's hard
//! `RLIMIT_RTTIME` at grant time and SIGKILLs the WHOLE PROCESS if an RT
//! thread overruns it without blocking. The limit is set here, before the
//! call (soft = caller's budget -> SIGXCPU warning, hard = the daemon's
//! `RTTimeUSecMax` -> the kill). The budget resets every blocking syscall -
//! being late is fine, spinning forever is not. Lowering the hard limit is
//! IRREVERSIBLE without `CAP_SYS_RESOURCE` and process-wide; that is the
//! consent the caller signs by invoking promotion.

use std::time::Duration;

use crate::{
    AppliedPriority, Error, FallbackReason, Grant, Mechanism, MechanismPolicy, Result,
    ThreadPriority,
};

use super::affinity::{current_nice, set_thread_realtime_rr};
use super::scheduling_policy::{RT_PRIORITY, level_for_nice, nice_for};

/// Real-time promotion is conceptually "above" the named ladder; report it as
/// the strongest named level so [`AppliedPriority`] stays uniform.
fn rt_applied(mechanism: Mechanism) -> AppliedPriority {
    AppliedPriority::new(
        ThreadPriority::TimeCritical,
        ThreadPriority::TimeCritical,
        Grant::Realtime,
        mechanism,
    )
}

fn kept_timeshare(reason: FallbackReason) -> AppliedPriority {
    let current = current_nice().unwrap_or(nice_for(ThreadPriority::Normal));

    AppliedPriority::new(
        ThreadPriority::TimeCritical,
        level_for_nice(current),
        Grant::Direct,
        Mechanism {
            policy: MechanismPolicy::Nice,
            value: current as i8,
        },
    )
    .with_reason(reason)
}

pub(crate) fn promote(budget: Duration) -> Result<AppliedPriority> {
    if budget.is_zero() {
        return Err(Error::InvalidParameter(
            "real-time budget must be greater than zero".to_string(),
        ));
    }

    // 1. Direct SCHED_RR - privileged or RT-permissive environments.
    match set_thread_realtime_rr(RT_PRIORITY) {
        Ok(()) => {
            return Ok(rt_applied(Mechanism {
                policy: MechanismPolicy::SchedRr,
                value: RT_PRIORITY as i8,
            }));
        }
        Err(Error::PermissionDenied(_)) => {}
        Err(e) => return Err(e),
    };

    #[cfg(all(feature = "rtkit", target_os = "linux"))]
    {
        use super::rtkit::Broker;

        let mut reason = FallbackReason::NoBroker;

        let tid = super::affinity::current_tid()? as u64;

        // SAFETY: getpid is always safe to call and cannot fail.
        let pid = unsafe { libc::getpid() } as u64;

        // Portal first: under Flatpak it is the ONLY working path, everywhere
        // else it proxies to the same rtkit daemon.
        match Broker::portal() {
            Ok(mut portal) => {
                let prio = request_priority(portal.max_realtime_priority());
                let rttime_max = portal.rttime_usec_max();

                if set_rttime_rlimit(budget, rttime_max).is_ok() {
                    match portal.make_thread_realtime_with_pid(pid, tid, prio) {
                        Ok(()) => {
                            return Ok(rt_applied(Mechanism {
                                policy: MechanismPolicy::SchedRr,
                                value: prio as i8,
                            }));
                        }
                        Err(e) => {
                            let (r, _) = super::rtkit::classify(&e);

                            reason = r;
                        }
                    }
                }
            }
            Err(r) => {
                reason = r;
            }
        }

        match Broker::rtkit() {
            Ok(mut rtkit) => {
                let prio = request_priority(rtkit.max_realtime_priority());
                let rttime_max = rtkit.rttime_usec_max();

                if set_rttime_rlimit(budget, rttime_max).is_ok() {
                    match rtkit.make_thread_realtime(tid, prio) {
                        Ok(()) => {
                            return Ok(rt_applied(Mechanism {
                                policy: MechanismPolicy::SchedRr,
                                value: prio as i8,
                            }));
                        }
                        Err(e) => {
                            let (r, _) = super::rtkit::classify(&e);

                            reason = r;
                        }
                    }
                }
            }
            Err(r) => {
                reason = r;
            }
        }

        Ok(kept_timeshare(reason))
    }
    #[cfg(not(all(feature = "rtkit", target_os = "linux")))]
    {
        let _ = budget;
        Ok(kept_timeshare(FallbackReason::NoBroker))
    }
}

/// The daemon REJECTS requests above its `MaxRealtimePriority` (it does not
/// clamp), so ask for the lesser of our band position and its ceiling.
#[cfg(all(feature = "rtkit", target_os = "linux"))]
fn request_priority(max_realtime_priority: i64) -> u32 {
    (RT_PRIORITY as i64).min(max_realtime_priority).clamp(1, 99) as u32
}

/// The Mozilla ritual: soft = budget (SIGXCPU, catchable warning), hard = the
/// daemon's ceiling (SIGKILL). Never raises the hard limit (impossible
/// unprivileged) - only lowers it toward what the grant requires.
#[cfg(all(feature = "rtkit", target_os = "linux"))]
fn set_rttime_rlimit(budget: Duration, rttime_usec_max: i64) -> Result<()> {
    let daemon_max = rttime_usec_max.max(1) as libc::rlim_t;

    let mut current = libc::rlimit {
        rlim_cur: 0,
        rlim_max: 0,
    };

    // SAFETY: getrlimit with a valid resource constant and a valid out-pointer.
    if unsafe { libc::getrlimit(libc::RLIMIT_RTTIME, &mut current) } != 0 {
        return Err(Error::SystemCall(format!(
            "getrlimit(RLIMIT_RTTIME) failed: {}",
            std::io::Error::last_os_error()
        )));
    }

    let hard = current.rlim_max.min(daemon_max);
    let soft = (budget.as_micros() as libc::rlim_t).clamp(1, hard);
    let wanted = libc::rlimit {
        rlim_cur: soft,
        rlim_max: hard,
    };

    // SAFETY: setrlimit with a valid resource constant and a valid limit struct.
    if unsafe { libc::setrlimit(libc::RLIMIT_RTTIME, &wanted) } != 0 {
        return Err(Error::SystemCall(format!(
            "setrlimit(RLIMIT_RTTIME, soft={}, hard={}) failed: {}",
            soft,
            hard,
            std::io::Error::last_os_error()
        )));
    }

    Ok(())
}

/// Returns the current thread to `SCHED_OTHER` at nice-neutral priority.
/// This is the self-demotion helper - call it from your own watchdog (or a
/// `SIGXCPU` handler's flag check) instead of letting the hard limit kill the
/// process. The lowered hard `RLIMIT_RTTIME` stays (irreversible), which is
/// harmless once no thread runs real-time.
pub(crate) fn demote() -> Result<()> {
    let tid = super::affinity::current_tid()?;

    // NOTE(linux): rtkit/portal grants arrive as SCHED_RR | SCHED_RESET_ON_FORK,
    // and CLEARING the reset-on-fork flag requires CAP_SYS_NICE - a plain
    // setschedparam(SCHED_OTHER) on a brokered thread fails EPERM (found
    // live). Demotion must read the current policy and carry the flag over.
    // SAFETY: sched_getscheduler with a valid tid; returns -1 on error.
    let current = unsafe { libc::sched_getscheduler(tid as libc::pid_t) };
    let reset_on_fork = if current >= 0 {
        current & libc::SCHED_RESET_ON_FORK
    } else {
        0
    };

    // SAFETY: sched_param is POD; zeroing it is safe. SCHED_OTHER requires
    // sched_priority 0.
    let param: libc::sched_param = unsafe { std::mem::zeroed() };

    // SAFETY: valid tid, valid policy, valid param.
    let res = unsafe {
        libc::sched_setscheduler(
            tid as libc::pid_t,
            libc::SCHED_NORMAL | reset_on_fork,
            &param,
        )
    };

    if res != 0 {
        let err = std::io::Error::last_os_error();
        return Err(Error::SystemCall(format!(
            "sched_setscheduler(SCHED_NORMAL) failed: {}",
            err
        )));
    }

    Ok(())
}
