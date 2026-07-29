use crate::{
    AffinityMask, CacheInfo, CoreKind, CpuFeatures, L2Domain, L3Domain, Lp, Result, Vendor,
};

/// The system's CPU topology and identity - a flat, by-value description.
///
/// The model is per-LP records ([`Lp`]) plus first-class [`L3Domain`]s and
/// per-kind caches. There is deliberately no socket -> core tree: a per-socket
/// hierarchy cannot represent chiplet CPUs (a Ryzen 5950X is ONE socket with
/// TWO 32 MiB L3 domains) and per-core cache copies only duplicate data.
/// Socket membership lives on each `Lp`; socket totals are derived counts.
///
/// Obtain it with [`CpuInfo::detect()`] and store it wherever you want - the
/// struct owns all its data and there is no global state in the library.
#[must_use]
#[derive(Debug, Clone)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct CpuInfo {
    /// One record per online logical processor.
    pub lps: Vec<Lp>,
    /// Physical core count (SMT siblings counted once).
    pub core_count: u16,
    /// Socket count (derived; sockets are not containers in this model).
    pub socket_count: u8,
    /// NUMA node count (1 on single-node systems and macOS).
    pub numa_node_count: u8,
    /// Physical cores per [`CoreKind`], indexed by [`CoreKind::index()`].
    pub kind_core_counts: [u16; CoreKind::COUNT],

    /// L3 cache domains (CCDs / clusters), content-keyed during detection.
    pub l3_domains: Vec<L3Domain>,
    /// L2 cache domains (the cores sharing each L2 instance), content-keyed
    /// during detection. Ordered by ascending lowest member LP id - a stable,
    /// reproducible order that usually matches the topology's grouping, but is
    /// not a formal physical-distance guarantee. The finest "these cores are
    /// closest" signal: slice cooperating threads out of an [`l3_domains`](Self::l3_domains)
    /// entry by taking whole L2 domains (filter by [`L2Domain::l3_domain`]).
    pub l2_domains: Vec<L2Domain>,
    /// L1 data cache per core kind, indexed by [`CoreKind::index()`].
    pub l1d: [CacheInfo; CoreKind::COUNT],
    /// L1 instruction cache per core kind.
    pub l1i: [CacheInfo; CoreKind::COUNT],
    /// L2 cache per core kind.
    pub l2: [CacheInfo; CoreKind::COUNT],

    /// The CPU manufacturer.
    pub vendor: Vendor,
    /// Model name as reported by the system (cpuid brand string, sysctl, …).
    pub model_name: String,
    /// Runtime-detected ISA feature flags.
    pub features: CpuFeatures,
}

impl CpuInfo {
    /// Detects the CPU topology using platform-specific methods.
    ///
    /// This reads OS interfaces only (sysfs, sysctl, Win32) - no global state
    /// is created and repeated calls are independent. Detect once at startup
    /// and keep the value.
    #[must_use = "detecting topology has a cost; keep and reuse the returned CpuInfo"]
    pub fn detect() -> Result<Self> {
        #[cfg(any(target_os = "linux", target_os = "android"))]
        {
            crate::platform::linux::cpu::detect_cpu_info()
        }
        #[cfg(target_os = "macos")]
        {
            crate::platform::macos::cpu::detect_cpu_info()
        }
        #[cfg(target_os = "windows")]
        {
            crate::platform::windows::cpu::detect_cpu_info()
        }
        #[cfg(not(any(
            target_os = "windows",
            target_os = "linux",
            target_os = "android",
            target_os = "macos"
        )))]
        {
            Err(crate::Error::Unsupported(
                "CPU information detection is not supported on this platform.".to_string(),
            ))
        }
    }

    /// Total number of physical cores (SMT siblings counted once).
    pub fn num_physical_cores(&self) -> usize {
        self.core_count as usize
    }

    /// Total number of logical processors (hardware threads).
    pub fn num_logical_cores(&self) -> usize {
        self.lps.len()
    }

    /// Physical cores classified as [`CoreKind::Performance`].
    ///
    /// On homogeneous machines this equals `num_physical_cores()` - the
    /// classification invariant is "homogeneous means all Performance".
    pub fn num_performance_cores(&self) -> usize {
        self.kind_core_counts[CoreKind::Performance.index()] as usize
    }

    /// Physical cores classified as [`CoreKind::Efficiency`]
    /// (plus see [`CpuInfo::num_lp_efficiency_cores`] for the LP-E tier).
    pub fn num_efficiency_cores(&self) -> usize {
        self.kind_core_counts[CoreKind::Efficiency.index()] as usize
    }

    /// Physical cores classified as [`CoreKind::LpEfficiency`].
    pub fn num_lp_efficiency_cores(&self) -> usize {
        self.kind_core_counts[CoreKind::LpEfficiency.index()] as usize
    }

    /// `true` when more than one of the {Performance, Efficiency, LpEfficiency}
    /// kinds is present.
    pub fn is_hybrid(&self) -> bool {
        let kinds_present = [
            CoreKind::Performance,
            CoreKind::Efficiency,
            CoreKind::LpEfficiency,
        ]
        .iter()
        .filter(|k| self.kind_core_counts[k.index()] > 0)
        .count();
        kinds_present > 1
    }

    /// All OS logical-processor ids, in detection order.
    pub fn logical_processor_ids(&self) -> Vec<usize> {
        self.lps.iter().map(|lp| lp.os_id as usize).collect()
    }

    /// Mask of every online LP.
    pub fn all_cores_mask(&self) -> AffinityMask {
        self.mask_where(|_| true)
    }

    /// Mask of LPs whose core is of `kind`.
    pub fn kind_mask(&self, kind: CoreKind) -> AffinityMask {
        self.mask_where(|lp| lp.kind == kind)
    }

    /// Mask of Performance-core LPs. Never empty: homogeneous machines are
    /// all-Performance by the classification invariant.
    pub fn performance_core_mask(&self) -> AffinityMask {
        self.kind_mask(CoreKind::Performance)
    }

    /// Mask of Efficiency-core LPs (empty on non-hybrid machines).
    pub fn efficiency_core_mask(&self) -> AffinityMask {
        self.kind_mask(CoreKind::Efficiency)
    }

    /// Mask of LpEfficiency-core LPs (empty on non-hybrid machines).
    pub fn lp_efficiency_core_mask(&self) -> AffinityMask {
        self.kind_mask(CoreKind::LpEfficiency)
    }

    /// Mask with ONE LP per physical core (`smt_index == 0`) - "no SMT siblings".
    pub fn primary_thread_mask(&self) -> AffinityMask {
        self.mask_where(|lp| lp.smt_index == 0)
    }

    /// Mask of the LPs in L3 domain `domain` (index into [`CpuInfo::l3_domains`]).
    pub fn l3_domain_mask(&self, domain: u8) -> AffinityMask {
        self.l3_domains
            .get(domain as usize)
            .map(|d| d.mask)
            .unwrap_or_else(AffinityMask::empty)
    }

    /// Mask of the LPs in L2 domain `domain` (index into [`CpuInfo::l2_domains`]).
    pub fn l2_domain_mask(&self, domain: u16) -> AffinityMask {
        self.l2_domains
            .get(domain as usize)
            .map(|d| d.mask)
            .unwrap_or_else(AffinityMask::empty)
    }

    /// Mask of the LPs on NUMA node `node`.
    pub fn numa_node_mask(&self, node: u8) -> AffinityMask {
        self.mask_where(|lp| lp.numa_node == node)
    }

    fn mask_where(&self, pred: impl Fn(&Lp) -> bool) -> AffinityMask {
        let mut mask = AffinityMask::empty();

        for lp in self.lps.iter().filter(|lp| pred(lp)) {
            mask.add(lp.os_id as usize);
        }

        mask
    }

    /// Sorts the L3 and L2 domain tables by ascending lowest member LP and
    /// remaps every `Lp`'s domain index to match - the order
    /// [`l3_domains`](Self::l3_domains) / [`l2_domains`](Self::l2_domains) promise.
    ///
    /// Detection appends domains in platform-iteration order. On a sparse online
    /// set (a cpuset-limited container, say) a domain's lowest member can be an
    /// offline LP that its `shared_cpu_list` still names, so iteration order and
    /// lowest-member order disagree; this restores the contract. Stable - an
    /// already-ascending table is left untouched.
    pub(crate) fn normalize_domain_order(&mut self) {
        let l3_remap = sort_domains_by_lowest_lp(&mut self.l3_domains, |d| &d.mask);
        let l2_remap = sort_domains_by_lowest_lp(&mut self.l2_domains, |d| &d.mask);

        for lp in &mut self.lps {
            if lp.l3_domain != Lp::NO_L3 {
                lp.l3_domain = l3_remap[lp.l3_domain as usize] as u8;
            }
            if lp.l2_domain != Lp::NO_L2 {
                lp.l2_domain = l2_remap[lp.l2_domain as usize] as u16;
            }
        }
    }
}

/// Sorts `domains` by ascending lowest member LP and returns the old-to-new
/// index `remap` (`remap[old] == new`). Stable, so an already-sorted table is
/// untouched and its remap is the identity.
fn sort_domains_by_lowest_lp<D: Clone>(
    domains: &mut Vec<D>,
    mask_of: impl Fn(&D) -> &AffinityMask,
) -> Vec<usize> {
    let mut order: Vec<usize> = (0..domains.len()).collect();
    order.sort_by_key(|&i| mask_of(&domains[i]).iter().next().unwrap_or(usize::MAX));

    let mut remap = vec![0usize; domains.len()];
    for (new_idx, &old_idx) in order.iter().enumerate() {
        remap[old_idx] = new_idx;
    }

    *domains = order.iter().map(|&i| domains[i].clone()).collect();
    remap
}
