//! Defines thread priority levels used for scheduling.
//!
//! This module contains the [`ThreadPriority`] enum, which specifies various
//! priority levels that can be assigned to a thread. These levels help the
//! operating system determine how to schedule threads, which is crucial for
//! performance-sensitive applications like games.
//!
//! Each priority level is described with an example workload and notes on its
//! typical behavior, especially on Linux systems. Higher priority levels generally
//! mean that a thread is more likely to be run and less likely to be preempted,
//! but the exact behavior is OS-dependent. Using very high (real-time) priorities
//! often requires special permissions.

/// Represents different priority levels that can be assigned to a thread.
///
/// These priority levels are hints to the operating system's scheduler.
/// The actual behavior can vary based on the OS, system load, and other factors.
///
/// # What each level actually maps to
///
/// | Level | Linux | Windows | macOS (Apple Silicon) |
/// |---|---|---|---|
/// | `Background` | nice 19 | `THREAD_PRIORITY_IDLE` | QoS `BACKGROUND` (E-cores) |
/// | `Lowest` | nice 10 | `THREAD_PRIORITY_LOWEST` | QoS `UTILITY` (E-core-leaning) |
/// | `BelowNormal` | nice 5 | `THREAD_PRIORITY_BELOW_NORMAL` | QoS `DEFAULT` |
/// | `Normal` | nice 0 | `THREAD_PRIORITY_NORMAL` | QoS `USER_INITIATED` |
/// | `AboveNormal` | nice -5 ¹ | `THREAD_PRIORITY_ABOVE_NORMAL` | QoS `USER_INTERACTIVE` (rel -4) |
/// | `Highest` | nice -10 ¹ | `THREAD_PRIORITY_HIGHEST` | QoS `USER_INTERACTIVE` |
/// | `TimeCritical` | nice -20 ¹ | `THREAD_PRIORITY_TIME_CRITICAL` | `SCHED_RR` 47 ² |
///
/// ¹ Negative nice needs `CAP_SYS_NICE` or a raised `RLIMIT_NICE`. Without it
/// the cascade asks **rtkit** for the value (feature `rtkit`, on by default -
/// rtkit's default floor is nice -15, so `TimeCritical` lands on -15 there)
/// and finally keeps the level the thread already has - reported as data, never
/// an error. Probe the outcome up front with
/// [`crate::priority_capabilities`]. The Linux ladder is deliberately pure
/// timeshare - no level requests `SCHED_RR`; true real-time is the explicit
/// opt-in [`crate::promote_thread_to_realtime`] (a spinning RT thread owns
/// its core, and rtkit-brokered RT comes with a process-wide SIGKILL leash -
/// that trade-off belongs to the application, not a priority table).
///
/// ² macOS `TimeCritical` is a one-way door: `pthread_setschedparam` gives the
/// thread FIXED priority 47 (no timeshare decay - the audio-feeder use case) but
/// per Apple's `qos.h` it **permanently opts the thread out of the QoS system**.
/// Setting a QoS-backed level on that thread afterwards is handled by falling back
/// to legacy `SCHED_OTHER` with a scaled priority, but the thread never rejoins
/// QoS (and loses its P/E-core routing hints). Dedicate such threads.
///
/// # The same name is NOT the same strength everywhere
///
/// On Linux `TimeCritical` is the strongest *timeshare* slot (nice -20 ≈ a
/// ×9 CFS weight edge over `Highest`): it wins virtually every wake-up race
/// but cannot starve the machine - for preempt-everything semantics use
/// [`crate::promote_thread_to_realtime`]. On Windows it is the top of the
/// *dynamic* priority band (priority 15), not the `REALTIME_PRIORITY_CLASS`.
/// On macOS it is the top of the *user* band (47 = `MAXPRI_USER`) with fixed
/// (no-decay) semantics, not the Mach time-constraint band that CoreAudio
/// render threads occupy. Write code that treats these as strong hints, not
/// guarantees.
///
/// The enum derives common traits like `Debug`, `Clone`, `Copy`, `PartialEq`, `Eq`,
/// `PartialOrd`, `Ord`, `Hash`, and `Default` (where `Normal` is the default).
/// It also implements `Display` for easy printing of priority level names.
// repr(u8): the discriminant is used as an ordinal -- `priority as usize` indexes
// PriorityCaps::effective_rank -- so it is pinned to a compact, stable 0..6 byte.
// The C ABI uses a separate raw `i32` enum and reconstructs this type at the boundary.
#[repr(u8)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum ThreadPriority {
    /// Background priority: For tasks that should only run when CPU is idle.
    ///
    /// Ideal for non-critical background operations that should have minimal impact
    /// on foreground tasks.
    ///
    /// # Example Workloads
    /// *   Steam API synchronization, achievement updates, cloud saves.
    /// *   Other absolute background noise processes.
    ///
    /// # Platform Notes
    /// *   **Linux:** Typically uses `SCHED_OTHER` policy with a high `nice` value (e.g., 19).
    ///     Under heavy system load, p99 latency can spike significantly, potentially
    ///     into hundreds of milliseconds or even seconds.
    Background = 0,

    /// Lowest priority: throughput work that should yield under contention
    /// but still make real progress - roughly a ninth of a `Normal` thread's
    /// CPU share on Linux when both compete.
    ///
    /// # Example Workloads
    /// *   Shader/PSO compilation, navmesh and lighting bakes.
    /// *   Batch asset processing, analytics, telemetry.
    ///
    /// # Platform Notes
    /// *   **Linux:** `SCHED_OTHER` with nice 10.
    ///     Tail-latencies can be long under load - by design for this level.
    Lowest = 1,

    /// Below normal priority: For tasks that are less critical than normal operations.
    ///
    /// Use for asynchronous workers, secondary game systems, AI planning, or other
    /// non-urgent gameplay systems that can be preempted by more critical tasks.
    ///
    /// # Example Workloads
    /// *   Asynchronous worker threads, secondary game systems.
    /// *   AI pathfinding or planning.
    /// *   Non-urgent gameplay logic.
    ///
    /// # Platform Notes
    /// *   **Linux:** `SCHED_OTHER` with nice 5 - about a third of a `Normal`
    ///     thread's share under contention, gentle enough that streaming keeps
    ///     flowing while the frame is busy.
    BelowNormal = 2,

    /// Normal priority: The default priority for most threads.
    ///
    /// Suitable for general tasks like asset loading, streaming, or prefetching,
    /// where I/O is often the bottleneck but latency still matters.
    ///
    /// # Example Workloads
    /// *   Asset loading and streaming.
    /// *   Prefetching game data.
    /// *   Standard application threads.
    ///
    /// # Platform Notes
    /// *   **Linux:** Typically uses `SCHED_OTHER` with a `nice` value of 0.
    ///     Offers no real-time guarantees and can experience latency spikes under heavy load.
    #[default]
    Normal = 3,

    /// Above normal priority: For tasks that are more important than normal but not critical.
    ///
    /// Use for main game logic, input processing, or UI threads that need to be responsive
    /// but don't require hard real-time guarantees.
    ///
    /// # Example Workloads
    /// *   Main game loop, primary game logic.
    /// *   User input processing.
    /// *   UI rendering and interaction thread.
    ///
    /// # Platform Notes
    /// *   **Linux:** `SCHED_OTHER` with nice -5 (≈3× a `Normal` thread's
    ///     share). Needs privilege or rtkit; see the cascade note above.
    AboveNormal = 4,

    /// Highest priority: For critical tasks that are deadline-sensitive.
    ///
    /// Recommended for render threads or audio processing threads where meeting deadlines
    /// is crucial for smooth user experience.
    ///
    /// # Example Workloads
    /// *   Main render thread.
    /// *   Audio processing and mixing thread.
    ///
    /// # Platform Notes
    /// *   **Linux:** `SCHED_OTHER` with nice -10 (≈9× a `Normal` thread's
    ///     share - a pinned render thread owns its core without legally
    ///     starving anything). Needs privilege or rtkit; see the cascade
    ///     note above.
    Highest = 5,

    /// Time-critical priority: For extremely sensitive tasks requiring minimum latency.
    ///
    /// **Use with extreme caution.** This level gives threads the highest possible precedence
    /// and can potentially starve other system processes if not managed carefully.
    /// Ideal for short, critical bursts of work on performance cores.
    ///
    /// # Example Workloads
    /// *   Highly critical worker threads pinned to Performance-cores (P-cores).
    /// *   Tasks demanding absolute minimum latency.
    ///
    /// # Platform Notes
    /// *   **General:** the strongest level `set_thread_priority` hands out;
    ///     true real-time is the separate opt-in
    ///     [`crate::promote_thread_to_realtime`].
    /// *   **Linux:** `SCHED_OTHER` with nice -20 - ≈9× the share of a
    ///     `Highest` thread, so the audio feeder wins its wake-up races even
    ///     against your own best threads, without RT's ability to wedge a
    ///     core. rtkit-brokered grants clamp to the daemon's floor (default
    ///     -15).
    /// *   **macOS:** `SCHED_RR` 47 - fixed priority (no timeshare decay), no privileges needed,
    ///     but PERMANENTLY opts the thread out of the QoS system (see the table above).
    TimeCritical = 6,
}

impl std::fmt::Display for ThreadPriority {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ThreadPriority::Background => write!(f, "Background"),
            ThreadPriority::Lowest => write!(f, "Lowest"),
            ThreadPriority::BelowNormal => write!(f, "BelowNormal"),
            ThreadPriority::Normal => write!(f, "Normal"),
            ThreadPriority::AboveNormal => write!(f, "AboveNormal"),
            ThreadPriority::Highest => write!(f, "Highest"),
            ThreadPriority::TimeCritical => write!(f, "TimeCritical"),
        }
    }
}

/// How the OS satisfied a thread-priority request.
///
/// The `path`/`tier` split is deliberate: a request can be brokered *and*
/// real-time (Linux rtkit granting `SCHED_RR`), so "how it was applied" and
/// "what tier it landed in" are orthogonal questions.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum Grant {
    /// Applied directly by the OS scheduler (`setpriority`, `SetThreadPriority`,
    /// `pthread_set_qos_class_self_np`).
    Direct,
    /// Negotiated through a privilege broker - Linux rtkit / the xdg realtime
    /// portal - because the direct syscall was denied.
    Brokered,
    /// A real-time policy was engaged (macOS `TimeCritical` `SCHED_RR`, or the
    /// consent API [`crate::promote_thread_to_realtime`]).
    Realtime,
}

impl std::fmt::Display for Grant {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Grant::Direct => write!(f, "Direct"),
            Grant::Brokered => write!(f, "Brokered"),
            Grant::Realtime => write!(f, "Realtime"),
        }
    }
}

/// Why a thread-priority request didn't get a clean, direct grant of exactly
/// what was asked.
///
/// Carried by [`AppliedPriority::reason`]. `None` means you got the requested
/// level directly (or brokered at full strength) - nothing to worry about.
/// `Some(_)` is the answer to *"my engine feels wonky on this box - what did my
/// priority actually do?"* - returned as data so the caller decides (retry,
/// warn, telemeter), never as a hidden log side-effect.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum FallbackReason {
    /// Direct syscall denied and no broker could satisfy it - feature `rtkit`
    /// off, no system bus, or no rtkit daemon. The effective level reports
    /// what the thread kept.
    NoBroker,
    /// Broker reached but it didn't answer in time - a busy bus, or a daemon
    /// starved by the very load you're prioritizing against. Transient; a retry
    /// when the system is calmer may succeed. The effective level reports what
    /// the thread kept.
    BrokerTimedOut,
    /// Broker reached and it explicitly refused (policy / rate limit). The
    /// payload is the typed refusal - branch on it (`AccessDenied` vs
    /// `LimitsExceeded`) to decide retry vs give-up. The effective level
    /// reports what the thread kept.
    BrokerRefused(BrokerError),
    /// Broker granted, but weaker than asked - it hit its ceiling (rtkit caps
    /// negative nice at `MinNiceLevel`, default -15). You kept the *level* but
    /// lost strength; reach for [`crate::promote_thread_to_realtime`] if you
    /// need the real thing.
    Clamped,
}

impl std::fmt::Display for FallbackReason {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            FallbackReason::NoBroker => write!(f, "NoBroker"),
            FallbackReason::BrokerTimedOut => write!(f, "BrokerTimedOut"),
            FallbackReason::BrokerRefused(_) => write!(f, "BrokerRefused"),
            FallbackReason::Clamped => write!(f, "Clamped"),
        }
    }
}

/// The specific reason a privilege broker REFUSED a grant - the payload of
/// [`FallbackReason::BrokerRefused`], also reachable through the
/// [`AppliedPriority::broker_error`] convenience accessor.
///
/// This is the *actionable* classification behind a refusal: branch on it to
/// decide whether to retry. The variants are platform-neutral; on Linux they
/// are produced by mapping the broker's D-Bus error name, and we deliberately
/// keep only the well-known names as variants and collapse everything else to
/// [`Other`](BrokerError::Other) - the name is the signal, the daemon's
/// free-text message is not worth a heap allocation for the rare unmapped case
/// (read the rtkit journal for that).
///
/// `#[non_exhaustive]`: brokers can grow error names; matching must carry a `_`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[non_exhaustive]
pub enum BrokerError {
    /// Policy denied the grant - polkit, rlimits, or no active/seated login
    /// session (common over SSH). Treat as persistent for the current process
    /// and configuration; retry only after a privilege or session change.
    AccessDenied,
    /// The broker's rate limit was hit (rtkit caps actions per interval, e.g.
    /// 25 / 20 s / UID). Transient - back off and retry later, or elevate fewer
    /// threads.
    LimitsExceeded,
    /// The broker rejected the arguments (e.g. a priority out of its range). A
    /// bug on our side or a daemon-version skew, not a transient condition.
    InvalidArgs,
    /// A generic daemon-side failure with no more specific name.
    Failed,
    /// An error name this version doesn't map. The grant was refused; the
    /// specific cause is in the rtkit journal.
    Other,
}

// Compiled exactly where the rtkit broker is - the only producer of D-Bus
// error names.
#[cfg(all(feature = "rtkit", target_os = "linux"))]
impl BrokerError {
    /// Maps a D-Bus error name to its [`BrokerError`]. Unmapped names (including
    /// rtkit-private `org.freedesktop.RealtimeKit1.Error.*` ones) become
    /// [`Other`](BrokerError::Other).
    pub(crate) fn from_dbus_name(name: &str) -> BrokerError {
        match name {
            "org.freedesktop.DBus.Error.AccessDenied" => BrokerError::AccessDenied,
            "org.freedesktop.DBus.Error.LimitsExceeded" => BrokerError::LimitsExceeded,
            "org.freedesktop.DBus.Error.InvalidArgs" => BrokerError::InvalidArgs,
            "org.freedesktop.DBus.Error.Failed" => BrokerError::Failed,
            _ => BrokerError::Other,
        }
    }
}

impl std::fmt::Display for BrokerError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            BrokerError::AccessDenied => write!(f, "AccessDenied"),
            BrokerError::LimitsExceeded => write!(f, "LimitsExceeded"),
            BrokerError::InvalidArgs => write!(f, "InvalidArgs"),
            BrokerError::Failed => write!(f, "Failed"),
            BrokerError::Other => write!(f, "Other"),
        }
    }
}

/// Which OS scheduler API set a thread's priority -- the discriminant that says
/// how to read [`Mechanism::value`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum MechanismPolicy {
    /// Linux `SCHED_OTHER` via `setpriority` -- `value` is the nice (-20..=19).
    Nice,
    /// `SCHED_RR` real-time -- `value` is the RR priority (Linux RT, macOS `TimeCritical`).
    SchedRr,
    /// POSIX `SCHED_OTHER` `sched_priority` band -- `value` is the band (macOS QoS opt-out fallback).
    SchedOther,
    /// macOS Quality-of-Service -- `value` is a [`QosClass`] ordinal.
    Qos,
    /// Windows `SetThreadPriority` -- `value` is the `THREAD_PRIORITY_*` constant (-15..=15).
    WinPriority,
}

impl std::fmt::Display for MechanismPolicy {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            MechanismPolicy::Nice => write!(f, "nice"),
            MechanismPolicy::SchedRr => write!(f, "SCHED_RR"),
            MechanismPolicy::SchedOther => write!(f, "SCHED_OTHER"),
            MechanismPolicy::Qos => write!(f, "QoS"),
            MechanismPolicy::WinPriority => write!(f, "THREAD_PRIORITY"),
        }
    }
}

/// macOS Quality-of-Service class, stored as [`Mechanism::value`] when the policy
/// is [`MechanismPolicy::Qos`]. A stable ordinal (NOT the raw darwin `qos_class_t`
/// hex) so the C ABI and serialized conformance stay trivial.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[repr(i8)]
pub enum QosClass {
    /// macOS `QOS_CLASS_BACKGROUND`.
    Background = 0,
    /// macOS `QOS_CLASS_UTILITY`.
    Utility = 1,
    /// macOS `QOS_CLASS_DEFAULT`.
    Default = 2,
    /// macOS `QOS_CLASS_USER_INITIATED`.
    UserInitiated = 3,
    /// macOS `QOS_CLASS_USER_INTERACTIVE`.
    UserInteractive = 4,
}

impl QosClass {
    /// The [`QosClass`] for a stored ordinal, or `None` if out of range.
    #[must_use]
    pub fn from_value(value: i8) -> Option<QosClass> {
        match value {
            0 => Some(QosClass::Background),
            1 => Some(QosClass::Utility),
            2 => Some(QosClass::Default),
            3 => Some(QosClass::UserInitiated),
            4 => Some(QosClass::UserInteractive),
            _ => None,
        }
    }
}

impl std::fmt::Display for QosClass {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            QosClass::Background => write!(f, "Background"),
            QosClass::Utility => write!(f, "Utility"),
            QosClass::Default => write!(f, "Default"),
            QosClass::UserInitiated => write!(f, "UserInitiated"),
            QosClass::UserInteractive => write!(f, "UserInteractive"),
        }
    }
}

/// The concrete OS scheduling mechanism a priority request landed on -- the typed
/// replacement for the old human `detail` string. [`value`](Self::value) is
/// interpreted per [`policy`](Self::policy) (see [`MechanismPolicy`]). Two bytes,
/// no allocation; the [`Display`](std::fmt::Display) impl renders the human form
/// (e.g. `nice -15`, `QoS UserInteractive`, `SCHED_RR 47`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct Mechanism {
    /// Which OS scheduler API set the priority.
    pub policy: MechanismPolicy,
    /// The applied parameter, read per `policy` (nice / RR priority / QoS ordinal /
    /// band / `THREAD_PRIORITY_*` constant). Fits a signed byte on every platform.
    pub value: i8,
}

impl std::fmt::Display for Mechanism {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self.policy {
            MechanismPolicy::Qos => match QosClass::from_value(self.value) {
                Some(class) => write!(f, "QoS {class}"),
                None => write!(f, "QoS {}", self.value),
            },
            other => write!(f, "{} {}", other, self.value),
        }
    }
}

/// What a thread-priority request actually produced.
///
/// Returned by [`set_thread_priority`](crate::set_thread_priority) and
/// [`promote_thread_to_realtime`](crate::promote_thread_to_realtime) in place
/// of `()`. The unit return was a lie by omission: on Linux a "successful"
/// `set_thread_priority(Highest)` can mean you got `Highest`, or that every
/// privileged path was denied and you silently kept the level you already had
/// (`Normal` for a fresh thread). This says which - uniformly across platforms,
/// so callers never `#[cfg]`.
///
/// Branch on [`Grant`] / [`AppliedPriority::degraded`] / [`AppliedPriority::reason`]
/// / [`AppliedPriority::mechanism`] for logic; use the `Display` impl for the
/// human-readable form.
#[must_use = "a priority request can silently fall back (no privilege or no broker); \
              inspect the result -- degraded()/effective/reason -- instead of discarding it, \
              or the downgrade goes unnoticed"]
#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize))]
pub struct AppliedPriority {
    /// The level the caller requested. For the consent API this is
    /// [`ThreadPriority::TimeCritical`] - the strongest named level - since
    /// real-time has no level above it.
    requested: ThreadPriority,
    /// The level actually in effect. Strictly weaker than `requested` means the
    /// request fell back to a different, lower level.
    effective: ThreadPriority,
    /// How the request was satisfied.
    grant: Grant,
    /// Why the request fell short, if it did - see [`FallbackReason`]. `None`
    /// means you got exactly what you asked for. A broker refusal carries its
    /// typed detail inside [`FallbackReason::BrokerRefused`].
    reason: Option<FallbackReason>,
    /// The concrete OS scheduling mechanism the request landed on, as typed data
    /// (the former human `detail` string). `value` is interpreted per `policy`.
    mechanism: Mechanism,
}

impl AppliedPriority {
    /// Rebuilds an outcome from structured data.
    ///
    /// Every combination of parts is a valid outcome.
    pub fn from_parts(
        requested: ThreadPriority,
        effective: ThreadPriority,
        grant: Grant,
        reason: Option<FallbackReason>,
        mechanism: Mechanism,
    ) -> Self {
        Self {
            requested,
            effective,
            grant,
            reason,
            mechanism,
        }
    }

    pub(crate) fn new(
        requested: ThreadPriority,
        effective: ThreadPriority,
        grant: Grant,
        mechanism: Mechanism,
    ) -> Self {
        Self::from_parts(requested, effective, grant, None, mechanism)
    }

    /// The level the caller requested.
    #[must_use]
    pub fn requested(&self) -> ThreadPriority {
        self.requested
    }

    /// The level actually in effect.
    #[must_use]
    pub fn effective(&self) -> ThreadPriority {
        self.effective
    }

    /// How the request was satisfied.
    #[must_use]
    pub fn grant(&self) -> Grant {
        self.grant
    }

    /// Why the request fell short, if it did.
    #[must_use]
    pub fn reason(&self) -> Option<FallbackReason> {
        self.reason
    }

    /// The concrete OS scheduling mechanism the request landed on.
    #[must_use]
    pub fn mechanism(&self) -> Mechanism {
        self.mechanism
    }

    /// The typed reason a broker refused the grant - the payload of
    /// [`FallbackReason::BrokerRefused`], surfaced as a convenience so callers
    /// can branch without a nested match.
    #[must_use]
    pub fn broker_error(&self) -> Option<BrokerError> {
        match self.reason {
            Some(FallbackReason::BrokerRefused(e)) => Some(e),
            _ => None,
        }
    }

    /// Records why the request fell short. Builder-style so the clean-grant
    /// call sites (the overwhelming majority) don't mention it at all. Only the
    /// Linux-family cascade clamps or falls back; Windows/macOS produce clean
    /// grants or hard errors, so this is compiled only where a producer exists.
    #[cfg(any(target_os = "linux", target_os = "android"))]
    pub(crate) fn with_reason(mut self, reason: FallbackReason) -> Self {
        self.reason = Some(reason);

        self
    }

    /// `true` when the request did NOT get a clean grant of exactly what was
    /// asked - a fall back to a weaker level (`Highest` -> `Normal`) *or* a
    /// broker clamp within the level (`TimeCritical` -> nice -15). Equivalent to
    /// `reason.is_some()`; the [`FallbackReason`] says which.
    #[must_use]
    pub fn degraded(&self) -> bool {
        self.reason.is_some()
    }
}

#[cfg(feature = "serde")]
impl<'de> serde::Deserialize<'de> for AppliedPriority {
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        #[derive(serde::Deserialize)]
        struct Parts {
            requested: ThreadPriority,
            effective: ThreadPriority,
            grant: Grant,
            reason: Option<FallbackReason>,
            mechanism: Mechanism,
        }

        let parts = Parts::deserialize(deserializer)?;

        Ok(AppliedPriority::from_parts(
            parts.requested,
            parts.effective,
            parts.grant,
            parts.reason,
            parts.mechanism,
        ))
    }
}

impl std::fmt::Display for AppliedPriority {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        if self.requested != self.effective {
            write!(f, "{} -> {}", self.requested, self.effective)?;

            if self.grant != Grant::Direct || self.reason.is_some() {
                write!(f, " [")?;

                let mut sep = "";

                if self.grant != Grant::Direct {
                    write!(f, "{}", self.grant)?;
                    sep = ", ";
                }

                if let Some(reason) = self.reason {
                    write!(f, "{sep}{reason}")?;
                }

                write!(f, "]")?;
            }
        } else {
            write!(f, "{}", self.effective)?;

            if self.grant != Grant::Direct || self.reason.is_some() {
                write!(f, " [")?;

                let mut sep = "";

                if self.grant != Grant::Direct {
                    write!(f, "{}", self.grant)?;
                    sep = ", ";
                }

                if let Some(reason) = self.reason {
                    write!(f, "{sep}{reason}")?;
                }

                write!(f, "]")?;
            }
        }

        if let Some(broker_error) = self.broker_error() {
            write!(f, " ({broker_error})")?;
        }

        write!(f, " {}", self.mechanism)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn thread_priority_ordinals_are_stable() {
        assert_eq!(ThreadPriority::Background as u8, 0);
        assert_eq!(ThreadPriority::Lowest as u8, 1);
        assert_eq!(ThreadPriority::BelowNormal as u8, 2);
        assert_eq!(ThreadPriority::Normal as u8, 3);
        assert_eq!(ThreadPriority::AboveNormal as u8, 4);
        assert_eq!(ThreadPriority::Highest as u8, 5);
        assert_eq!(ThreadPriority::TimeCritical as u8, 6);
    }

    #[test]
    #[cfg(all(feature = "rtkit", target_os = "linux"))]
    fn broker_error_maps_known_dbus_names() {
        assert_eq!(
            BrokerError::from_dbus_name("org.freedesktop.DBus.Error.AccessDenied"),
            BrokerError::AccessDenied
        );
        assert_eq!(
            BrokerError::from_dbus_name("org.freedesktop.DBus.Error.LimitsExceeded"),
            BrokerError::LimitsExceeded
        );
        assert_eq!(
            BrokerError::from_dbus_name("org.freedesktop.DBus.Error.InvalidArgs"),
            BrokerError::InvalidArgs
        );
        assert_eq!(
            BrokerError::from_dbus_name("org.freedesktop.DBus.Error.Failed"),
            BrokerError::Failed
        );
    }

    #[test]
    #[cfg(all(feature = "rtkit", target_os = "linux"))]
    fn broker_error_unmapped_name_is_other() {
        assert_eq!(
            BrokerError::from_dbus_name("org.freedesktop.RealtimeKit1.Error.Whatever"),
            BrokerError::Other
        );
        assert_eq!(BrokerError::from_dbus_name(""), BrokerError::Other);
    }

    #[test]
    fn mechanism_display_renders_per_policy() {
        assert_eq!(
            Mechanism {
                policy: MechanismPolicy::Nice,
                value: -15
            }
            .to_string(),
            "nice -15"
        );
        assert_eq!(
            Mechanism {
                policy: MechanismPolicy::SchedRr,
                value: 47
            }
            .to_string(),
            "SCHED_RR 47"
        );
        assert_eq!(
            Mechanism {
                policy: MechanismPolicy::Qos,
                value: QosClass::UserInteractive as i8
            }
            .to_string(),
            "QoS UserInteractive"
        );
        assert_eq!(
            Mechanism {
                policy: MechanismPolicy::WinPriority,
                value: 2
            }
            .to_string(),
            "THREAD_PRIORITY 2"
        );
    }

    #[test]
    fn applied_priority_display_appends_mechanism() {
        // Clean direct grant: the effective level, then the mechanism.
        let clean = AppliedPriority::new(
            ThreadPriority::Normal,
            ThreadPriority::Normal,
            Grant::Direct,
            Mechanism {
                policy: MechanismPolicy::Nice,
                value: 0,
            },
        );
        assert_eq!(clean.to_string(), "Normal nice 0");

        let brokered = AppliedPriority::new(
            ThreadPriority::Highest,
            ThreadPriority::Highest,
            Grant::Brokered,
            Mechanism {
                policy: MechanismPolicy::Nice,
                value: -10,
            },
        );
        assert_eq!(brokered.to_string(), "Highest [Brokered] nice -10");

        // A clamp keeps the level, spells the loss out, then the kept mechanism.
        // from_parts is the only constructor available on every target.
        let clamped = AppliedPriority::from_parts(
            ThreadPriority::TimeCritical,
            ThreadPriority::TimeCritical,
            Grant::Brokered,
            Some(FallbackReason::Clamped),
            Mechanism {
                policy: MechanismPolicy::Nice,
                value: -15,
            },
        );
        assert_eq!(
            clamped.to_string(),
            "TimeCritical [Brokered, Clamped] nice -15"
        );

        // A refusal prints the bare variant in the bracket and the typed detail
        // as a suffix - the payload does not change the bracket text.
        let refused = AppliedPriority::from_parts(
            ThreadPriority::Highest,
            ThreadPriority::Normal,
            Grant::Direct,
            Some(FallbackReason::BrokerRefused(BrokerError::LimitsExceeded)),
            Mechanism {
                policy: MechanismPolicy::Nice,
                value: 0,
            },
        );
        assert_eq!(
            refused.to_string(),
            "Highest -> Normal [BrokerRefused] (LimitsExceeded) nice 0"
        );
        assert_eq!(refused.broker_error(), Some(BrokerError::LimitsExceeded));
    }
}
