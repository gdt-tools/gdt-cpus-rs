//! rtkit / realtime-portal brokers - negotiated thread priority over D-Bus.
//!
//! Two services hand out priority to unprivileged processes:
//! - **rtkit** (`org.freedesktop.RealtimeKit1`, system bus) - the desktop
//!   daemon PipeWire and SDL use. `MakeThreadHighPriority` grants negative
//!   nice with NO strings attached (the thread stays `SCHED_OTHER`, so
//!   `RLIMIT_RTTIME` and rtkit's SIGKILL enforcement never apply - leash-free,
//!   safe for the default `set_thread_priority` path). `MakeThreadRealtime`
//!   grants `SCHED_RR` and is leashED: rtkit checks the process's hard
//!   `RLIMIT_RTTIME` at grant time and a busy-loop over the budget is killed.
//! - **the xdg realtime portal** (`org.freedesktop.portal.Realtime`, session
//!   bus) - same grants, but it remaps PIDs across sandbox namespaces, which
//!   makes it the path that works under Flatpak (direct rtkit sees the wrong
//!   PID there; under Steam's pressure-vessel the PID namespace is shared and
//!   direct rtkit works).
//!
//! Properties worth knowing (rtkit compile-time defaults): `MinNiceLevel`
//! -15, `MaxRealtimePriority` 20 - requests ABOVE the max are rejected, not
//! clamped - and `RTTimeUSecMax` 200 ms.

use std::sync::atomic::{AtomicBool, Ordering};

use super::dbus::{Arg, BusKind, CallError, Connection, parse_variant_int};
use crate::{BrokerError, FallbackReason};

const RTKIT_DEST: &str = "org.freedesktop.RealtimeKit1";
const RTKIT_PATH: &str = "/org/freedesktop/RealtimeKit1";
const RTKIT_IFACE: &str = "org.freedesktop.RealtimeKit1";

const PORTAL_DEST: &str = "org.freedesktop.portal.Desktop";
const PORTAL_PATH: &str = "/org/freedesktop/portal/desktop";
const PORTAL_IFACE: &str = "org.freedesktop.portal.Realtime";

const PROPERTIES_IFACE: &str = "org.freedesktop.DBus.Properties";

/// rtkit's compile-time default for `MinNiceLevel`, used when the property
/// read fails but the daemon is otherwise reachable.
pub(crate) const DEFAULT_MIN_NICE_LEVEL: i64 = -15;
/// rtkit's compile-time default for `RTTimeUSecMax` (200 ms).
pub(crate) const DEFAULT_RTTIME_USEC_MAX: i64 = 200_000;
/// rtkit's compile-time default for `MaxRealtimePriority`.
pub(crate) const DEFAULT_MAX_REALTIME_PRIORITY: i64 = 20;

// Set once a connect attempt or a ServiceUnknown reply proves the service
// absent - every later cascade skips the bus round-trip. Never reset: a
// daemon appearing mid-process is rare enough to ignore, a daemon that is
// absent gets probed exactly once.
static RTKIT_UNAVAILABLE: AtomicBool = AtomicBool::new(false);
static PORTAL_UNAVAILABLE: AtomicBool = AtomicBool::new(false);

/// A connected priority broker (rtkit or the realtime portal).
pub(crate) struct Broker {
    conn: Connection,
    dest: &'static str,
    path: &'static str,
    iface: &'static str,
    unavailable_flag: &'static AtomicBool,
}

/// Turns a failed call into the reason we report and whether to mark the broker
/// unavailable for the rest of the process (the `*_UNAVAILABLE` flag above).
///
/// The bool is the part that matters. Only a service that is provably gone
/// (`Absent`) sets it. A `TimedOut` must not: the usual cause is a daemon starved
/// by the very load we are prioritizing against, and giving up on rtkit after one
/// slow handshake would make every later call report "no broker" while the daemon
/// is sitting right there -- a passing stall turned into a permanent outage.
pub(crate) fn classify(e: &CallError) -> (FallbackReason, bool) {
    match e {
        CallError::Absent => (FallbackReason::NoBroker, true),
        CallError::TimedOut => (FallbackReason::BrokerTimedOut, false),
        CallError::Refused { name } => (
            FallbackReason::BrokerRefused(BrokerError::from_dbus_name(name)),
            false,
        ),
        CallError::Io(_) => (FallbackReason::NoBroker, false),
    }
}

impl Broker {
    /// Connects to rtkit on the system bus.
    pub(crate) fn rtkit() -> Result<Self, FallbackReason> {
        Self::open(
            BusKind::System,
            RTKIT_DEST,
            RTKIT_PATH,
            RTKIT_IFACE,
            &RTKIT_UNAVAILABLE,
        )
    }

    /// Connects to the realtime portal on the session bus.
    pub(crate) fn portal() -> Result<Self, FallbackReason> {
        Self::open(
            BusKind::Session,
            PORTAL_DEST,
            PORTAL_PATH,
            PORTAL_IFACE,
            &PORTAL_UNAVAILABLE,
        )
    }

    fn open(
        kind: BusKind,
        dest: &'static str,
        path: &'static str,
        iface: &'static str,
        unavailable_flag: &'static AtomicBool,
    ) -> Result<Self, FallbackReason> {
        if unavailable_flag.load(Ordering::Relaxed) {
            return Err(FallbackReason::NoBroker);
        }

        match Connection::open(kind) {
            Ok(conn) => Ok(Broker {
                conn,
                dest,
                path,
                iface,
                unavailable_flag,
            }),
            Err(e) => {
                let (reason, absent) = classify(&e);

                if absent {
                    unavailable_flag.store(true, Ordering::Relaxed);
                }

                Err(reason)
            }
        }
    }

    fn property(&mut self, name: &str) -> Option<i64> {
        match self.conn.call(
            self.dest,
            self.path,
            PROPERTIES_IFACE,
            "Get",
            &[Arg::Str(self.iface), Arg::Str(name)],
        ) {
            Ok(body) => parse_variant_int(&body),
            Err(e) => {
                if matches!(e, CallError::Absent) {
                    self.unavailable_flag.store(true, Ordering::Relaxed);
                }

                None
            }
        }
    }

    /// Most negative nice the broker will grant (rtkit default -15).
    pub(crate) fn min_nice_level(&mut self) -> i64 {
        self.property("MinNiceLevel")
            .unwrap_or(DEFAULT_MIN_NICE_LEVEL)
    }

    /// Highest `SCHED_RR` priority the broker will grant (rtkit default 20).
    /// Requests above this are REJECTED outright, not clamped.
    pub(crate) fn max_realtime_priority(&mut self) -> i64 {
        self.property("MaxRealtimePriority")
            .unwrap_or(DEFAULT_MAX_REALTIME_PRIORITY)
    }

    /// Largest `RLIMIT_RTTIME` (µs) the broker accepts at grant time.
    pub(crate) fn rttime_usec_max(&mut self) -> i64 {
        self.property("RTTimeUSecMax")
            .unwrap_or(DEFAULT_RTTIME_USEC_MAX)
    }

    fn method(&mut self, member: &str, args: &[Arg<'_>]) -> Result<(), CallError> {
        match self
            .conn
            .call(self.dest, self.path, self.iface, member, args)
        {
            Ok(_) => Ok(()),
            Err(e) => {
                if matches!(e, CallError::Absent) {
                    self.unavailable_flag.store(true, Ordering::Relaxed);
                }

                Err(e)
            }
        }
    }

    /// Grants negative nice to `tid` (must belong to the calling process).
    /// Leash-free: the thread stays `SCHED_OTHER`.
    pub(crate) fn make_thread_high_priority(
        &mut self,
        tid: u64,
        nice: i32,
    ) -> Result<(), CallError> {
        self.method("MakeThreadHighPriority", &[Arg::U64(tid), Arg::I32(nice)])
    }

    /// Grants `SCHED_RR` `priority` to `tid` via rtkit. The process's hard
    /// `RLIMIT_RTTIME` must be within the broker's `RTTimeUSecMax` BEFORE
    /// this call - rtkit validates it at grant time.
    pub(crate) fn make_thread_realtime(
        &mut self,
        tid: u64,
        priority: u32,
    ) -> Result<(), CallError> {
        self.method("MakeThreadRealtime", &[Arg::U64(tid), Arg::U32(priority)])
    }

    /// Portal form of [`Self::make_thread_realtime`] - carries the pid so the
    /// portal can remap it across sandbox PID namespaces (the Flatpak case).
    pub(crate) fn make_thread_realtime_with_pid(
        &mut self,
        pid: u64,
        tid: u64,
        priority: u32,
    ) -> Result<(), CallError> {
        self.method(
            "MakeThreadRealtimeWithPID",
            &[Arg::U64(pid), Arg::U64(tid), Arg::U32(priority)],
        )
    }
}

/// One step of the `set_thread_priority` cascade: when direct `setpriority`
/// is denied, ask rtkit for the negative nice instead (clamped to the daemon's
/// `MinNiceLevel`). On success returns the nice value actually granted (which
/// may be weaker than requested - the caller detects the clamp); on failure
/// returns the classified [`FallbackReason`]. A daemon that answered with a
/// D-Bus ERROR classifies as [`FallbackReason::BrokerRefused`] carrying the
/// typed [`BrokerError`] mapped from its error name - so the caller can branch
/// on WHY a grant was refused (e.g. `AccessDenied` vs `LimitsExceeded`).
pub(crate) fn try_high_priority(tid: u64, requested_nice: i32) -> Result<i32, FallbackReason> {
    let mut broker = Broker::rtkit()?;

    let min_nice = broker.min_nice_level().clamp(-20, 0) as i32;
    let nice = requested_nice.max(min_nice);

    match broker.make_thread_high_priority(tid, nice) {
        Ok(()) => Ok(nice),
        Err(e) => {
            let (reason, _absent) = classify(&e);

            Err(reason)
        }
    }
}
