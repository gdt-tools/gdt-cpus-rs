//! CPU affinity mask for specifying sets of logical processors.
//!
//! This module provides the [`AffinityMask`] type, which represents a set of
//! logical processor IDs that a thread may run on. Unlike pinning to a single
//! core, affinity masks allow threads to migrate between a specified set of
//! cores, reducing scheduling latency while still constraining execution.

use std::fmt;
use std::fmt::Debug;
use std::num::ParseIntError;
use std::str::FromStr;

/// Number of `u64` words in the fixed bitset. 16 words = 1024 logical
/// processors, matching the Linux static `cpu_set_t` (`CPU_SETSIZE`).
const WORDS: usize = 16;

/// Error type for parsing affinity masks from strings
#[derive(Debug, thiserror::Error)]
pub enum AffinityMaskFromStrError {
    /// Bad string format, it must be a comma-separated list of CPU cores or ranges
    #[error("Bad string format, it must be a comma-separated list of CPU cores or ranges")]
    BadFormat,
    /// Failed to parse integer
    #[error("Failed to parse integer: {0}")]
    ParseIntError(#[from] ParseIntError),
}

/// A cross-platform CPU affinity mask representing a set of logical processors.
///
/// An `AffinityMask` specifies which logical processors (CPU threads) a thread
/// is allowed to execute on. This is useful for:
///
/// - Restricting latency-sensitive threads to performance cores (P-cores)
/// - Restricting background threads to efficiency cores (E-cores)
/// - Allowing thread migration within a subset of cores to reduce scheduling latency
///
/// # Example
///
/// ```
/// use gdt_cpus::AffinityMask;
///
/// // Create a mask for cores 0, 1, and 2
/// let mask = AffinityMask::from_cores(&[0, 1, 2]);
/// assert_eq!(mask.count(), 3);
/// assert!(mask.contains(0));
/// assert!(mask.contains(1));
/// assert!(mask.contains(2));
/// assert!(!mask.contains(3));
/// ```
///
/// # Capacity
///
/// A mask holds up to [`AffinityMask::MAX_LP_COUNT`] (1024) logical processors -
/// the same ceiling as the Linux static `cpu_set_t`, above which the OS affinity
/// APIs require dynamic allocation this crate does not perform. Ids at or above
/// that bound are out of range and are ignored by [`add`](Self::add) /
/// [`remove`](Self::remove) / [`contains`](Self::contains), so the mask can never
/// be forced to allocate or hold an out-of-range bit. The backing store is a fixed
/// `[u64; 16]`, so the type is `Copy` and its `PartialEq`/`Eq`/`Hash` are canonical
/// (two masks compare equal iff they hold the same set, with no representation
/// ambiguity).
///
/// # Platform Behavior
///
/// When used with [`set_thread_affinity`](crate::set_thread_affinity):
///
/// - **Linux/Windows**: The mask is applied directly to constrain thread execution
/// - **macOS**: Returns `Error::Unsupported`; use QoS classes instead
#[must_use = "a mask is built to be applied or inspected; discarding it does nothing"]
#[derive(Clone, Copy, PartialEq, Eq, Hash, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct AffinityMask {
    /// Fixed bitset: `WORDS` u64 words covering cores 0..1024.
    /// `bits[0]` = cores 0-63, `bits[1]` = 64-127, ..., `bits[15]` = 960-1023.
    bits: [u64; WORDS],
}

impl AffinityMask {
    /// Maximum number of logical processors a mask can represent (1024).
    ///
    /// Matches the Linux static `cpu_set_t` (`CPU_SETSIZE`); the OS affinity APIs
    /// cap here too (beyond it they need dynamic allocation this crate does not do).
    /// Ids at or above this are out of range and ignored by the mutators.
    ///
    /// NOTE(windows): an `os_id` can exceed this only on a >1024-LP machine
    /// (>= 16 processor groups), which is a multi-socket server, not a target.
    pub const MAX_LP_COUNT: usize = WORDS * 64;

    /// Creates an empty affinity mask with no cores set.
    ///
    /// # Example
    ///
    /// ```
    /// use gdt_cpus::AffinityMask;
    ///
    /// let mask = AffinityMask::empty();
    /// assert!(mask.is_empty());
    /// assert_eq!(mask.count(), 0);
    /// ```
    pub fn empty() -> Self {
        Self::default()
    }

    /// Creates an affinity mask with a single core set.
    ///
    /// # Arguments
    ///
    /// * `logical_core_id` - The logical processor ID to include in the mask
    ///
    /// # Example
    ///
    /// ```
    /// use gdt_cpus::AffinityMask;
    ///
    /// let mask = AffinityMask::single(5);
    /// assert_eq!(mask.count(), 1);
    /// assert!(mask.contains(5));
    /// ```
    pub fn single(logical_core_id: usize) -> Self {
        let mut mask = Self::empty();

        mask.add(logical_core_id);

        mask
    }

    /// Creates an affinity mask from a slice of core IDs.
    ///
    /// # Arguments
    ///
    /// * `core_ids` - Slice of logical processor IDs to include
    ///
    /// # Example
    ///
    /// ```
    /// use gdt_cpus::AffinityMask;
    ///
    /// let mask = AffinityMask::from_cores(&[0, 2, 4, 6]);
    /// assert_eq!(mask.count(), 4);
    /// assert!(mask.contains(0));
    /// assert!(!mask.contains(1));
    /// assert!(mask.contains(2));
    /// ```
    pub fn from_cores(core_ids: &[usize]) -> Self {
        let mut mask = Self::empty();

        for &id in core_ids {
            mask.add(id);
        }

        mask
    }

    /// Adds a logical core to the mask.
    ///
    /// If the core is already in the mask, this is a no-op. Ids at or above
    /// [`MAX_LP_COUNT`](Self::MAX_LP_COUNT) are out of range and are ignored.
    ///
    /// # Arguments
    ///
    /// * `logical_core_id` - The logical processor ID to add
    ///
    /// # Example
    ///
    /// ```
    /// use gdt_cpus::AffinityMask;
    ///
    /// let mut mask = AffinityMask::empty();
    /// mask.add(0);
    /// mask.add(1);
    /// assert_eq!(mask.count(), 2);
    /// ```
    pub fn add(&mut self, logical_core_id: usize) {
        if logical_core_id >= Self::MAX_LP_COUNT {
            return;
        }

        let word_idx = logical_core_id / 64;
        let bit_idx = logical_core_id % 64;

        self.bits[word_idx] |= 1u64 << bit_idx;
    }

    /// Removes a logical core from the mask.
    ///
    /// If the core is not in the mask (or is out of range), this is a no-op.
    ///
    /// # Arguments
    ///
    /// * `logical_core_id` - The logical processor ID to remove
    ///
    /// # Example
    ///
    /// ```
    /// use gdt_cpus::AffinityMask;
    ///
    /// let mut mask = AffinityMask::from_cores(&[0, 1, 2]);
    /// mask.remove(1);
    /// assert_eq!(mask.count(), 2);
    /// assert!(!mask.contains(1));
    /// ```
    pub fn remove(&mut self, logical_core_id: usize) {
        if logical_core_id >= Self::MAX_LP_COUNT {
            return;
        }

        let word_idx = logical_core_id / 64;
        let bit_idx = logical_core_id % 64;

        self.bits[word_idx] &= !(1u64 << bit_idx);
    }

    /// Checks if a logical core is in the mask.
    ///
    /// # Arguments
    ///
    /// * `logical_core_id` - The logical processor ID to check
    ///
    /// # Returns
    ///
    /// `true` if the core is in the mask, `false` otherwise (including any id at
    /// or above [`MAX_LP_COUNT`](Self::MAX_LP_COUNT)).
    ///
    /// # Example
    ///
    /// ```
    /// use gdt_cpus::AffinityMask;
    ///
    /// let mask = AffinityMask::from_cores(&[0, 2, 4]);
    /// assert!(mask.contains(0));
    /// assert!(!mask.contains(1));
    /// assert!(mask.contains(2));
    /// ```
    #[must_use]
    pub fn contains(&self, logical_core_id: usize) -> bool {
        if logical_core_id >= Self::MAX_LP_COUNT {
            return false;
        }

        let word_idx = logical_core_id / 64;
        let bit_idx = logical_core_id % 64;

        (self.bits[word_idx] & (1u64 << bit_idx)) != 0
    }

    /// Returns the number of cores in the mask.
    ///
    /// # Example
    ///
    /// ```
    /// use gdt_cpus::AffinityMask;
    ///
    /// let mask = AffinityMask::from_cores(&[0, 1, 2, 3]);
    /// assert_eq!(mask.count(), 4);
    /// ```
    #[must_use]
    pub fn count(&self) -> usize {
        self.bits.iter().map(|w| w.count_ones() as usize).sum()
    }

    /// Returns `true` if the mask is empty (no cores set).
    ///
    /// # Example
    ///
    /// ```
    /// use gdt_cpus::AffinityMask;
    ///
    /// let empty = AffinityMask::empty();
    /// assert!(empty.is_empty());
    ///
    /// let non_empty = AffinityMask::single(0);
    /// assert!(!non_empty.is_empty());
    /// ```
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.bits.iter().all(|&w| w == 0)
    }

    /// Returns an iterator over the core IDs in the mask.
    ///
    /// # Example
    ///
    /// ```
    /// use gdt_cpus::AffinityMask;
    ///
    /// let mask = AffinityMask::from_cores(&[1, 3, 5]);
    /// let cores: Vec<usize> = mask.iter().collect();
    /// assert_eq!(cores, vec![1, 3, 5]);
    /// ```
    pub fn iter(&self) -> impl Iterator<Item = usize> + '_ {
        self.bits.iter().enumerate().flat_map(|(word_idx, &word)| {
            (0..64).filter_map(move |bit_idx| {
                if (word & (1u64 << bit_idx)) != 0 {
                    Some(word_idx * 64 + bit_idx)
                } else {
                    None
                }
            })
        })
    }

    /// Returns the union of this mask with another.
    ///
    /// The resulting mask contains all cores that are in either mask.
    ///
    /// # Example
    ///
    /// ```
    /// use gdt_cpus::AffinityMask;
    ///
    /// let a = AffinityMask::from_cores(&[0, 1]);
    /// let b = AffinityMask::from_cores(&[1, 2]);
    /// let union = a.union(&b);
    /// assert_eq!(union.count(), 3);
    /// assert!(union.contains(0));
    /// assert!(union.contains(1));
    /// assert!(union.contains(2));
    /// ```
    pub fn union(&self, other: &AffinityMask) -> AffinityMask {
        let mut bits = [0u64; WORDS];

        for (i, slot) in bits.iter_mut().enumerate() {
            *slot = self.bits[i] | other.bits[i];
        }

        AffinityMask { bits }
    }

    /// Returns the intersection of this mask with another.
    ///
    /// The resulting mask contains only cores that are in both masks.
    ///
    /// # Example
    ///
    /// ```
    /// use gdt_cpus::AffinityMask;
    ///
    /// let a = AffinityMask::from_cores(&[0, 1, 2]);
    /// let b = AffinityMask::from_cores(&[1, 2, 3]);
    /// let intersection = a.intersection(&b);
    /// assert_eq!(intersection.count(), 2);
    /// assert!(intersection.contains(1));
    /// assert!(intersection.contains(2));
    /// ```
    pub fn intersection(&self, other: &AffinityMask) -> AffinityMask {
        let mut bits = [0u64; WORDS];

        for (i, slot) in bits.iter_mut().enumerate() {
            *slot = self.bits[i] & other.bits[i];
        }

        AffinityMask { bits }
    }

    /// Returns the first 64 cores as a raw `u64` bitmask.
    ///
    /// This is useful for platform APIs that only support 64 cores.
    /// Cores beyond index 63 are not included.
    ///
    /// # Example
    ///
    /// ```
    /// use gdt_cpus::AffinityMask;
    ///
    /// let mask = AffinityMask::from_cores(&[0, 1, 63]);
    /// let raw = mask.as_raw_u64();
    /// assert_eq!(raw, 0x8000_0000_0000_0003);
    /// ```
    pub fn as_raw_u64(&self) -> u64 {
        self.bits[0]
    }

    /// Returns the raw bits as a slice.
    ///
    /// Each element represents 64 cores: `bits[0]` = cores 0-63,
    /// `bits[1]` = cores 64-127, etc. The slice is always
    /// [`MAX_LP_COUNT`](Self::MAX_LP_COUNT)`/ 64` words long.
    pub fn as_raw_bits(&self) -> &[u64] {
        &self.bits
    }
}

impl fmt::Debug for AffinityMask {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("AffinityMask")
            .field("cores", &Ranges::<true>(self))
            .field("count", &self.count())
            .finish()
    }
}

impl fmt::Display for AffinityMask {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        // Display is the value, not the type: a bare range list (`0-3, 6-9`, ``)
        Ranges::<false>(self).fmt(f)
    }
}

impl FromStr for AffinityMask {
    type Err = AffinityMaskFromStrError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let mut mask = Self::empty();

        if s.is_empty() {
            return Ok(mask);
        }

        for s in s.split(',') {
            let mut parts = s.trim_ascii().split('-');
            let range_start = parts
                .next()
                .ok_or(AffinityMaskFromStrError::BadFormat)?
                .parse()?;

            if let Some(range_end) = parts.next() {
                let range_end = range_end.parse()?;

                mask.extend(range_start..=range_end);
            } else {
                mask.add(range_start);
            }
        }

        Ok(mask)
    }
}

/// Formats the set core ids as a bracketed (if `BRACKETS = true`), comma-separated
/// range list: `[]`, `[5]`, `[0-3]`, `[0-3, 6-9, 15]`.
/// Relies on [`AffinityMask::iter`] yielding ids in ascending order, so consecutive
/// runs coalesce into `a-b`.
struct Ranges<'a, const BRACKETS: bool>(&'a AffinityMask);

impl<const BRACKETS: bool> fmt::Debug for Ranges<'_, BRACKETS> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let mut iter = self.0.iter().peekable();
        let mut first = true;

        if BRACKETS {
            write!(f, "[")?;
        }
        while let Some(start) = iter.next() {
            let mut end = start;
            while iter.peek() == Some(&(end + 1)) {
                end = iter.next().unwrap();
            }

            if !first {
                write!(f, ", ")?;
            }
            first = false;

            if start == end {
                write!(f, "{start}")?;
            } else {
                write!(f, "{start}-{end}")?;
            }
        }
        if BRACKETS {
            write!(f, "]")?;
        }

        Ok(())
    }
}

impl FromIterator<usize> for AffinityMask {
    fn from_iter<T: IntoIterator<Item = usize>>(iter: T) -> Self {
        let mut mask = AffinityMask::empty();
        mask.extend(iter);
        mask
    }
}

impl Extend<usize> for AffinityMask {
    fn extend<Iter>(&mut self, iter: Iter)
    where
        Iter: IntoIterator<Item = usize>,
    {
        for core in iter {
            self.add(core);
        }
    }
}

impl IntoIterator for &AffinityMask {
    type Item = usize;
    type IntoIter = std::vec::IntoIter<usize>;

    fn into_iter(self) -> Self::IntoIter {
        self.iter().collect::<Vec<_>>().into_iter()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_empty_mask() {
        let mask = AffinityMask::empty();
        assert!(mask.is_empty());
        assert_eq!(mask.count(), 0);
        assert!(!mask.contains(0));
    }

    #[test]
    fn test_single_core() {
        let mask = AffinityMask::single(5);
        assert!(!mask.is_empty());
        assert_eq!(mask.count(), 1);
        assert!(mask.contains(5));
        assert!(!mask.contains(4));
    }

    #[test]
    fn test_from_cores() {
        let mask = AffinityMask::from_cores(&[0, 2, 4, 6]);
        assert_eq!(mask.count(), 4);
        assert!(mask.contains(0));
        assert!(!mask.contains(1));
        assert!(mask.contains(2));
    }

    #[test]
    fn test_add_remove() {
        let mut mask = AffinityMask::empty();
        mask.add(0);
        mask.add(1);
        assert_eq!(mask.count(), 2);

        mask.remove(0);
        assert_eq!(mask.count(), 1);
        assert!(!mask.contains(0));
        assert!(mask.contains(1));
    }

    #[test]
    fn test_high_core_ids() {
        let mut mask = AffinityMask::empty();
        mask.add(0);
        mask.add(64);
        mask.add(128);

        assert_eq!(mask.count(), 3);
        assert!(mask.contains(0));
        assert!(mask.contains(64));
        assert!(mask.contains(128));
        assert!(!mask.contains(63));
        assert!(!mask.contains(65));
    }

    #[test]
    fn test_iter() {
        let mask = AffinityMask::from_cores(&[1, 3, 5, 64, 65]);
        let cores: Vec<usize> = mask.iter().collect();
        assert_eq!(cores, vec![1, 3, 5, 64, 65]);
    }

    #[test]
    fn test_as_raw_u64() {
        let mask = AffinityMask::from_cores(&[0, 1, 63]);
        assert_eq!(mask.as_raw_u64(), 0x8000_0000_0000_0003);
    }

    #[test]
    fn test_from_iterator() {
        let mask: AffinityMask = vec![0, 2, 4].into_iter().collect();
        assert_eq!(mask.count(), 3);
        assert!(mask.contains(0));
        assert!(mask.contains(2));
        assert!(mask.contains(4));
    }

    #[test]
    fn test_extend() {
        let mut mask = AffinityMask::from_iter([0, 2, 4]);
        // The out-of-range id is silently dropped, same as `add`.
        mask.extend([1, 3, AffinityMask::MAX_LP_COUNT]);
        assert_eq!(mask, AffinityMask::from_iter([0, 1, 2, 3, 4]));
    }

    // A fixed backing array makes equality canonical: add-then-remove of a
    // high core leaves the exact same representation as a fresh empty mask
    // (the old Vec<u64> kept a trailing zero word and compared unequal).
    #[test]
    fn test_equality_is_canonical() {
        let mut a = AffinityMask::single(64);
        a.remove(64);
        assert_eq!(a, AffinityMask::empty());

        let wide = AffinityMask::from_cores(&[0, 200]);
        let narrow = AffinityMask::from_cores(&[0]);
        assert_eq!(wide.intersection(&narrow), narrow);
        assert_eq!(narrow.union(&AffinityMask::empty()), narrow);
    }

    // The capacity boundary: 1023 is the last valid core; MAX_LP_COUNT and
    // above are out of range and silently ignored (never an allocation/panic).
    #[test]
    fn test_capacity_boundary() {
        let mut mask = AffinityMask::empty();
        mask.add(AffinityMask::MAX_LP_COUNT - 1); // 1023
        assert!(mask.contains(AffinityMask::MAX_LP_COUNT - 1));
        assert_eq!(mask.count(), 1);

        mask.add(AffinityMask::MAX_LP_COUNT); // 1024 - out of range
        mask.add(u32::MAX as usize); // far out of range
        assert!(!mask.contains(AffinityMask::MAX_LP_COUNT));
        assert_eq!(mask.count(), 1);
    }

    // Debug renders cores as a bracketed range list; the brackets keep an
    // empty mask from formatting as a blank field and keep multi-range output
    // from blurring into the trailing `count` field.
    #[test]
    fn test_debug_format() {
        assert_eq!(
            format!("{:?}", AffinityMask::empty()),
            "AffinityMask { cores: [], count: 0 }"
        );
        assert_eq!(
            format!("{:?}", AffinityMask::single(5)),
            "AffinityMask { cores: [5], count: 1 }"
        );
        assert_eq!(
            format!("{:?}", AffinityMask::from_cores(&[0, 1, 2, 3])),
            "AffinityMask { cores: [0-3], count: 4 }"
        );
        assert_eq!(
            format!(
                "{:?}",
                AffinityMask::from_cores(&[0, 1, 2, 3, 6, 7, 8, 9, 15])
            ),
            "AffinityMask { cores: [0-3, 6-9, 15], count: 9 }"
        );
    }

    // A run that straddles the bits[0]/bits[1] u64 word split (63|64|65) must
    // still coalesce, and 1023 is the max id, so `end + 1 == 1024` exercises
    // the no-overflow edge of the range-extension loop.
    #[test]
    fn test_debug_format_word_boundary_and_max_id() {
        assert_eq!(
            format!("{:?}", AffinityMask::from_cores(&[63, 64, 65, 1023])),
            "AffinityMask { cores: [63-65, 1023], count: 4 }"
        );
    }

    // Display is the bare value -- the range list with no decoration: `0-3`, ``.
    #[test]
    fn test_display_format() {
        assert_eq!(
            format!(
                "{}",
                AffinityMask::from_cores(&[0, 1, 2, 3, 6, 7, 8, 9, 15])
            ),
            "0-3, 6-9, 15"
        );
        assert_eq!(format!("{}", AffinityMask::empty()), "");
    }

    /// Passing from a string is the reverse of display formatting.
    #[test]
    fn test_parse_format() {
        assert_eq!(
            AffinityMask::from_str("0-3, 6-9,15").unwrap(),
            AffinityMask::from_cores(&[0, 1, 2, 3, 6, 7, 8, 9, 15])
        );
        assert_eq!(AffinityMask::from_str("").unwrap(), AffinityMask::empty());
    }
}
