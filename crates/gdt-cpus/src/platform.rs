//! Platform-Specific Implementations.
//!
//! This module acts as a dispatcher for platform-specific code. It uses
//! conditional compilation (`#[cfg]`) to include and re-export types and
//! functions relevant to the target operating system (Linux, macOS, Windows)
//! and architecture (e.g., x86_64).
//!
//! The primary goal is to abstract away platform differences, providing a
//! consistent API at the crate level for functionalities like:
//! - CPU information detection (`CpuInfo::detect()`).
//! - Thread affinity, priority, and real-time promotion.
//!
//! Internal helper modules, such as `common_x86_64`, provide shared logic for
//! specific architectures across different operating systems.

// This module contains platform-specific implementations for CPU information,
// thread affinity, and performance monitoring.

// Conditionally compile and export platform-specific modules.

#[cfg(target_arch = "x86_64")]
pub(crate) mod common_x86_64;

// Ungated: `AffinityMask::from_str` parses this grammar on every platform.
// The sysfs-facing entry points inside carry their own Linux-family/test gates.
pub(crate) mod ranges;

// Shared expected.txt checker for fixture-driven detection tests (Linux sysfs
// trees, macOS sysctl dumps) - test builds only.
#[cfg(test)]
pub(crate) mod fixture_expected;

#[cfg(any(target_os = "linux", target_os = "android"))]
pub(crate) mod linux;

// Compiled for tests on EVERY platform: the macOS detection pipeline is pure
// logic behind a SysctlSource seam, so Linux CI exercises it against recorded
// fixtures (only the live sysctl impl + thread control are macOS-gated inside).
#[cfg(any(target_os = "macos", test))]
pub(crate) mod macos;

#[cfg(target_os = "windows")]
pub(crate) mod windows;

// NOTE: priority mapping is an internal detail since 26.x - each platform's
// table lives in its own scheduling_policy module and is consumed only by
// that platform's set_thread_priority. There is no public SchedulingPolicy
// type anymore (nothing outside the crate ever consumed it).
