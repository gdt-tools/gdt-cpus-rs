//! Kernel range-list parsing ("0-3,7,10-11" -> CPU IDs).
//!
//! `AffinityMask::from_str` parses this grammar on every platform, so the
//! module is ungated. The sysfs-facing entry points (Linux `cpu/online`,
//! `cache/index*/shared_cpu_list`, `node*/cpulist`, and the fixture checker's
//! `expected.txt` lists) are gated to the Linux family (linux, android) and
//! test builds.

use crate::{Error, Result};

/// Parses a kernel range-list string ("0-3,7,10-11"), calling `sink` once per
/// CPU id in order. Allocates nothing - the caller decides what to do with each
/// id (count it, set a mask bit, track a minimum), so the hot detection loops
/// that only need a membership test / count / min never materialize a throwaway
/// `Vec` per sysfs line.
///
/// # Errors
///
/// Returns `Error::Detection` on malformed ranges or non-numeric IDs.
#[cfg(any(target_os = "linux", target_os = "android", test))]
pub(crate) fn parse_range_list_with<F: FnMut(usize)>(range_str: &str, sink: F) -> Result<()> {
    parse_range_list_inner(range_str, None, sink)
}

/// Like [`parse_range_list_with`], but rejects an id at or above
/// `max_exclusive` *before* expanding the range that names it: `"0-4000000000"`
/// takes minutes to expand, so a bound checked in the sink is too late.
///
/// Detection stays unbounded, so a machine wider than the caller's bitset
/// degrades instead of failing to enumerate.
///
/// # Errors
///
/// Returns `Error::InvalidCoreId` for an id at or above `max_exclusive`, plus
/// everything [`parse_range_list_with`] returns.
pub(crate) fn parse_range_list_bounded<F: FnMut(usize)>(
    range_str: &str,
    max_exclusive: usize,
    sink: F,
) -> Result<()> {
    parse_range_list_inner(range_str, Some(max_exclusive), sink)
}

fn parse_range_list_inner<F: FnMut(usize)>(
    range_str: &str,
    max_exclusive: Option<usize>,
    mut sink: F,
) -> Result<()> {
    let in_bounds = |id: usize| match max_exclusive {
        Some(max) if id >= max => Err(Error::InvalidCoreId(id)),
        _ => Ok(id),
    };

    for part in range_str.trim().split(',') {
        let part = part.trim();

        if part.is_empty() {
            continue;
        }

        if part.contains('-') {
            let mut iter = part.splitn(2, '-');

            let start_str = iter
                .next()
                .ok_or_else(|| Error::Detection(format!("Invalid CPU range format: {}", part)))?;
            let end_str = iter
                .next()
                .ok_or_else(|| Error::Detection(format!("Invalid CPU range format: {}", part)))?;

            let start = start_str
                .parse::<usize>()
                .map_err(|_| Error::Detection(format!("Invalid CPU range start: {}", start_str)))?;
            let end = end_str
                .parse::<usize>()
                .map_err(|_| Error::Detection(format!("Invalid CPU range end: {}", end_str)))?;

            if start > end {
                return Err(Error::Detection(format!(
                    "Invalid CPU range order: {}-{}",
                    start, end
                )));
            }

            // Both ends before the loop: bounding only `start` would still walk
            // a 4-billion-wide range to reject it.
            in_bounds(start)?;
            in_bounds(end)?;

            for id in start..=end {
                sink(id);
            }
        } else {
            let cpu_id = part
                .parse::<usize>()
                .map_err(|_| Error::Detection(format!("Invalid CPU ID in range list: {}", part)))?;

            sink(in_bounds(cpu_id)?);
        }
    }

    Ok(())
}

/// Parses a kernel range-list string ("0-3,7,10-11") into CPU IDs.
///
/// Convenience over [`parse_range_list_with`] for the call sites that genuinely
/// need the materialized list (`cpu/online`, the fixture checker).
///
/// # Errors
///
/// Returns `Error::Detection` on malformed ranges or non-numeric IDs.
#[cfg(any(target_os = "linux", target_os = "android", test))]
pub(crate) fn parse_range_list_str(range_str: &str) -> Result<Vec<usize>> {
    let mut cpus = Vec::new();

    parse_range_list_with(range_str, |id| cpus.push(id))?;

    Ok(cpus)
}
