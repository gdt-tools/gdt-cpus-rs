//! Linux-specific implementations for CPU information and thread management.
//!
//! This module provides the Linux-specific logic for:
//! - Detecting detailed CPU information, including vendor, model, features,
//!   and topology (sockets, cores, logical processors, caches). This is primarily
//!   handled by the [`cpu`] submodule.
//! - Managing thread affinity and the nice-based priority cascade via the
//!   [`affinity`] submodule (priority tables in [`scheduling_policy`]).
//! - Negotiating priority with rtkit / the xdg realtime portal over a minimal
//!   hand-rolled D-Bus client ([`rtkit`], [`dbus`] - feature `rtkit`).
//! - Real-time promotion/demotion for the consent API ([`realtime`]) and the
//!   priority-outcome probe ([`capabilities`]).
//! - Common utility functions in the [`utils`] submodule.

pub(crate) mod affinity;
pub(crate) mod capabilities;
pub(crate) mod cpu;
#[cfg(all(feature = "rtkit", not(target_os = "android")))]
pub(crate) mod dbus;
pub(crate) mod realtime;
#[cfg(all(feature = "rtkit", not(target_os = "android")))]
pub(crate) mod rtkit;
pub(crate) mod scheduling_policy;
pub(crate) mod utils;
