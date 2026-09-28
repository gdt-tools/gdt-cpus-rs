//! Linux CPU topology detection - single-pass sysfs walk into the flat model.
//!
//! Pipeline (order is load-bearing - kinds must be final before per-kind cache
//! bucketing):
//! 1. online LP list (`devices/system/cpu/online`)
//! 2. per-LP topology: sibling masks (package/core ids on a tree without them) ->
//!    dense core indices, package ids -> socket indices, SMT order, explicit
//!    `core_type` when the kernel provides it (Intel hybrid)
//! 3. kind classification: `core_type` -> capacity thresholds -> all-Performance
//! 4. L3 domains: content-keyed by the lowest LP of each cache's
//!    `shared_cpu_list` - NEVER attributed per socket (chiplet CPUs have
//!    several L3 instances per socket) and never deduplicated by size
//! 5. per-kind L1/L2 from the first LP of each (now final) kind
//! 6. NUMA node ids from `devices/system/node/node*/cpulist`
//! 7. vendor/model/features (cpuid on x86_64, `/proc/cpuinfo` fallback)
//!
//! `detect_at()` takes the sysfs/procfs roots explicitly so recorded fixture
//! trees can drive the whole pipeline in tests.

use std::fs;
use std::path::Path;

use crate::{
    AffinityMask, CacheInfo, CoreKind, CpuFeatures, CpuInfo, Error, L2Domain, L3Domain, Lp, Result,
    Vendor,
};

use super::utils::{parse_range_list_str, parse_range_list_with};

pub(crate) mod features;
pub(crate) mod proc;

#[cfg(test)]
mod fixture_tests;

/// Detects CPU information from the live system.
pub fn detect_cpu_info() -> Result<CpuInfo> {
    detect_at(Path::new("/sys"), Path::new("/proc"))
}

/// Reads a sysfs file as a trimmed string; `None` if absent/unreadable.
fn read_str(path: &Path) -> Option<String> {
    fs::read_to_string(path).ok().map(|s| s.trim().to_string())
}

/// Reads a sysfs file as an integer; `None` if absent or unparseable.
fn read_u64(path: &Path) -> Option<u64> {
    read_str(path)?.parse().ok()
}

/// The LP's core key: the lowest id of its sibling mask (`core_cpus_list`, else the deprecated
/// `thread_siblings_list`) with the LP itself counted in. `None` when neither file yields a list.
fn sibling_mask_key(topo: &Path, os_id: usize) -> Option<u32> {
    let text = read_str(&topo.join("core_cpus_list"))
        .or_else(|| read_str(&topo.join("thread_siblings_list")))?;
    let mut lowest: Option<usize> = None;
    parse_range_list_with(&text, |id| lowest = Some(lowest.map_or(id, |l| l.min(id)))).ok()?;
    Some(lowest?.min(os_id) as u32)
}

/// Parses sysfs cache sizes: "32768K", "32M", bare bytes.
fn parse_cache_size(s: &str) -> u64 {
    if s.is_empty() {
        return 0;
    }

    let (digits, mult) = match s.as_bytes()[s.len() - 1] {
        b'K' => (&s[..s.len() - 1], 1024u64),
        b'M' => (&s[..s.len() - 1], 1024 * 1024),
        _ => (s, 1),
    };

    digits.parse::<u64>().map(|v| v * mult).unwrap_or(0)
}

/// Detection against explicit filesystem roots - the fixture-test seam.
///
/// `sysfs_root`/`procfs_root` replace `/sys` and `/proc`; recorded fixture
/// trees from the shared fixture corpus drive the full pipeline through this
/// function.
pub(crate) fn detect_at(sysfs_root: &Path, procfs_root: &Path) -> Result<CpuInfo> {
    let cpu_base = sysfs_root.join("devices/system/cpu");
    if !cpu_base.exists() {
        return Err(Error::Detection(format!(
            "CPU sysfs path not found: {:?}",
            cpu_base
        )));
    }

    // --- 1. Online LPs ---
    let online_str = read_str(&cpu_base.join("online"))
        .ok_or_else(|| Error::Detection("Failed to read cpu/online".to_string()))?;
    let mut online = parse_range_list_str(&online_str)?;

    online.sort_unstable();
    online.dedup();

    if online.is_empty() {
        return Err(Error::Detection("No online CPUs reported".to_string()));
    }

    if *online.last().unwrap() > u16::MAX as usize {
        return Err(Error::Detection(format!(
            "Logical processor id {} exceeds the supported range",
            online.last().unwrap()
        )));
    }

    // --- 2. Per-LP topology ---
    let mut lps: Vec<Lp> = Vec::with_capacity(online.len());
    let mut core_keys: Vec<u32> = Vec::new(); // one per physical core, dense by position
    let mut socket_ids: Vec<u16> = Vec::new();
    let mut capacities: Vec<Option<u64>> = Vec::with_capacity(online.len());

    // A core is named by the lowest LP of its sibling mask, which the kernel documents as the
    // CPUs within the same core. `core_id` is only the platform's identifier: SoCs restart it per
    // cluster inside one package, and the kernel's default is 0 on every CPU.
    // NOTE(linux): all-or-nothing. With any online LP lacking a mask (trees recorded without the
    // lists), every LP keys by `(package << 16) | core_id`, so the two key spaces never mix.
    let mask_keys: Option<Vec<u32>> = online
        .iter()
        .map(|&os_id| sibling_mask_key(&cpu_base.join(format!("cpu{}/topology", os_id)), os_id))
        .collect();

    for (i, &os_id) in online.iter().enumerate() {
        let topo = cpu_base.join(format!("cpu{}/topology", os_id));

        let pkg = read_u64(&topo.join("physical_package_id")).unwrap_or(0) as u16;
        let key = match &mask_keys {
            Some(keys) => keys[i],
            None => {
                let core_id = read_u64(&topo.join("core_id")).unwrap_or(0) as u16;
                (u32::from(pkg) << 16) | u32::from(core_id)
            }
        };

        let (core_idx, smt_index) = match core_keys.iter().position(|&k| k == key) {
            Some(idx) => {
                let siblings = lps.iter().filter(|lp| lp.core == idx as u16).count();
                (idx as u16, siblings as u8)
            }
            None => {
                core_keys.push(key);
                ((core_keys.len() - 1) as u16, 0)
            }
        };

        let socket_idx = match socket_ids.iter().position(|&s| s == pkg) {
            Some(idx) => idx as u8,
            None => {
                socket_ids.push(pkg);
                (socket_ids.len() - 1) as u8
            }
        };

        // Kind pass 1: explicit core_type (Intel hybrid kernels).
        let kind = match read_str(&topo.join("core_type")).as_deref() {
            Some("performance") | Some("0") => CoreKind::Performance,
            Some("efficiency") | Some("1") => CoreKind::Efficiency,
            _ => CoreKind::Unknown,
        };

        // Capacity is PRESENCE-TRACKED: absent files never enter the
        // classification comparison (defaulting them would mislabel machines
        // where only some LPs expose cpu_capacity).
        capacities.push(read_u64(
            &cpu_base.join(format!("cpu{}/cpu_capacity", os_id)),
        ));

        lps.push(Lp {
            os_id: os_id as u16,
            core: core_idx,
            socket: socket_idx,
            l3_domain: Lp::NO_L3,
            l2_domain: Lp::NO_L2,
            numa_node: 0,
            kind,
            smt_index,
            // perf_hint stamped below from cpu_capacity (0 when absent).
            perf_hint: 0,
            // cpu_part stamped below from /proc/cpuinfo (0 when absent, x86).
            cpu_part: 0,
        });
    }

    let core_count = core_keys.len() as u16;
    let socket_count = socket_ids.len() as u8;

    // --- 3. Kind pass 2: capacity thresholds for LPs without core_type ---
    // Classification is by THRESHOLD relative to max, never exact-equality
    // tiers (Intel ITMT favored cores give per-core jitter WITHIN the P tier).
    // Thresholds: >= 3/4 max -> Performance, >= 2/5 max -> Efficiency, below ->
    // LpEfficiency. Applied only when at least one capacity file existed AND
    // min < max; uniform/no signal ⇒ all-Performance (the homogeneous
    // invariant - never report a machine as all-Efficiency).
    let present: Vec<u64> = capacities.iter().flatten().copied().collect();
    let cap_max = present.iter().copied().max().unwrap_or(0);
    let cap_min = present.iter().copied().min().unwrap_or(0);
    let capacity_applies = !present.is_empty() && cap_min < cap_max;

    for (lp, capacity) in lps.iter_mut().zip(capacities.iter()) {
        // perf_hint = raw kernel capacity regardless of how the kind was
        // decided (core_type machines may expose capacity too); 0 = absent.
        lp.perf_hint = capacity.unwrap_or(0).min(u16::MAX as u64) as u16;

        if lp.kind != CoreKind::Unknown {
            continue;
        }

        lp.kind = match capacity {
            Some(cap) if capacity_applies => {
                if cap * 4 >= cap_max * 3 {
                    CoreKind::Performance
                } else if cap * 5 >= cap_max * 2 {
                    CoreKind::Efficiency
                } else {
                    CoreKind::LpEfficiency
                }
            }
            _ => CoreKind::Performance,
        };
    }

    // --- 4. L3 domains, content-keyed ---
    let mut l3_domains: Vec<L3Domain> = Vec::new();
    let mut domain_first_lp: Vec<usize> = Vec::new();

    for lp in lps.iter_mut() {
        for index in 0..10u32 {
            let idx_base = cpu_base.join(format!("cpu{}/cache/index{}", lp.os_id, index));

            let level = match read_u64(&idx_base.join("level")) {
                Some(l) => l,
                None => break,
            };

            if level != 3 {
                continue;
            }

            if read_str(&idx_base.join("type")).is_some_and(|t| t != "Unified") {
                continue;
            }

            let shared = match read_str(&idx_base.join("shared_cpu_list")) {
                Some(s) => s,
                None => break,
            };

            let mut mask = AffinityMask::empty();
            let mut first: Option<usize> = None; // lowest member = the L3 content key

            if parse_range_list_with(&shared, |id| {
                mask.add(id);
                first = Some(first.map_or(id, |f| f.min(id)));
            })
            .is_err()
            {
                break;
            }

            let Some(first) = first else {
                break;
            };

            let domain = match domain_first_lp.iter().position(|&k| k == first) {
                Some(d) => d,
                None => {
                    // A new L3 domain. Cap BEFORE pushing: NO_L3 (255) is the
                    // "no L3 domain" sentinel, so the last usable index is 254.
                    // Rejecting at the cap keeps a real domain from being labelled
                    // L3-less (index == NO_L3) and never appends a phantom domain.
                    if l3_domains.len() >= Lp::NO_L3 as usize {
                        break;
                    }

                    let size = read_str(&idx_base.join("size"))
                        .map(|s| parse_cache_size(&s))
                        .unwrap_or(0);

                    domain_first_lp.push(first);
                    l3_domains.push(L3Domain {
                        size_bytes: size,
                        mask,
                        core_count: 0,
                    });

                    l3_domains.len() - 1
                }
            };

            lp.l3_domain = domain as u8;

            break;
        }
    }

    for lp in &lps {
        if lp.smt_index == 0 && lp.l3_domain != Lp::NO_L3 {
            l3_domains[lp.l3_domain as usize].core_count += 1;
        }
    }

    // --- 4b. L2 domains, content-keyed (the step-4 loop at level 2) ---
    // NOTE(lifecycle): the ascending-lowest-LP order of `l2_domains` comes from
    // iterating `lps` in ascending os_id order (the online enumeration); a domain
    // is first seen at its lowest member, so new domains append in that order.
    let mut l2_domains: Vec<L2Domain> = Vec::new();
    let mut l2_first_lp: Vec<usize> = Vec::new();

    for lp in lps.iter_mut() {
        for index in 0..10u32 {
            let idx_base = cpu_base.join(format!("cpu{}/cache/index{}", lp.os_id, index));

            let level = match read_u64(&idx_base.join("level")) {
                Some(l) => l,
                None => break,
            };

            if level != 2 {
                continue;
            }

            if read_str(&idx_base.join("type")).is_some_and(|t| t != "Unified") {
                continue;
            }

            let shared = match read_str(&idx_base.join("shared_cpu_list")) {
                Some(s) => s,
                None => break,
            };

            let mut mask = AffinityMask::empty();
            let mut first: Option<usize> = None; // lowest member = the L2 content key

            if parse_range_list_with(&shared, |id| {
                mask.add(id);
                first = Some(first.map_or(id, |f| f.min(id)));
            })
            .is_err()
            {
                break;
            }

            let Some(first) = first else {
                break;
            };

            let domain = match l2_first_lp.iter().position(|&k| k == first) {
                Some(d) => d,
                None => {
                    // Cap BEFORE pushing: NO_L2 (0xFFFF) is the "no L2 domain"
                    // sentinel, so the last usable index is 0xFFFE.
                    if l2_domains.len() >= Lp::NO_L2 as usize {
                        break;
                    }

                    let size = read_str(&idx_base.join("size"))
                        .map(|s| parse_cache_size(&s))
                        .unwrap_or(0);

                    l2_first_lp.push(first);
                    l2_domains.push(L2Domain {
                        size_bytes: size,
                        mask,
                        core_count: 0,
                        // Every member of this L2 shares one L3; take it from the
                        // current (member) LP, already stamped by step 4.
                        l3_domain: lp.l3_domain,
                    });

                    l2_domains.len() - 1
                }
            };

            lp.l2_domain = domain as u16;

            break;
        }
    }

    for lp in &lps {
        if lp.smt_index == 0 && lp.l2_domain != Lp::NO_L2 {
            l2_domains[lp.l2_domain as usize].core_count += 1;
        }
    }

    // --- 5. Per-kind L1/L2 from the first LP of each (final) kind ---
    let mut l1d = [CacheInfo::default(); CoreKind::COUNT];
    let mut l1i = [CacheInfo::default(); CoreKind::COUNT];
    let mut l2 = [CacheInfo::default(); CoreKind::COUNT];

    for lp in &lps {
        let k = lp.kind.index();

        if l1d[k].size_bytes != 0 && l2[k].size_bytes != 0 {
            continue;
        }

        for index in 0..10u32 {
            let idx_base = cpu_base.join(format!("cpu{}/cache/index{}", lp.os_id, index));
            let level = match read_u64(&idx_base.join("level")) {
                Some(l) => l,
                None => break,
            };

            if level > 2 {
                continue;
            }

            let ctype = read_str(&idx_base.join("type")).unwrap_or_default();
            let mut ci = CacheInfo {
                size_bytes: read_str(&idx_base.join("size"))
                    .map(|s| parse_cache_size(&s))
                    .unwrap_or(0),
                line_bytes: read_u64(&idx_base.join("coherency_line_size")).unwrap_or(0) as u16,
                shared_by: 0,
            };

            if let Some(shared) = read_str(&idx_base.join("shared_cpu_list")) {
                let mut shared_by: u16 = 0;

                if parse_range_list_with(&shared, |_| shared_by += 1).is_ok() {
                    ci.shared_by = shared_by;
                }
            }

            match (level, ctype.as_str()) {
                (2, _) => l2[k] = ci,
                (1, "Data") => l1d[k] = ci,
                (1, "Instruction") => l1i[k] = ci,
                (1, "Unified") => {
                    l1d[k] = ci;
                    l1i[k] = ci;
                }
                _ => {}
            }
        }
    }

    // --- 6. NUMA nodes ---
    // Enumerate the nodes that actually exist. Prefer `node/online` (a
    // cpulist-style range, e.g. "0-1" or "0,2-3") so SPARSE node ids - a
    // depopulated socket, CXL/heterogeneous-memory node-id gaps - are handled.
    // Fall back to scanning a bounded range and SKIPPING gaps; never `break` on
    // the first gap (that truncated the count and left distant LPs on node 0).
    let online_nodes: Vec<usize> = read_str(&sysfs_root.join("devices/system/node/online"))
        .and_then(|s| parse_range_list_str(&s).ok())
        .filter(|v| !v.is_empty())
        .unwrap_or_else(|| (0..=u8::MAX as usize).collect());
    let mut present_nodes: u32 = 0;
    let mut claimed = vec![false; lps.len()];
    let mut degenerate = false;

    for node in online_nodes {
        let cpulist = sysfs_root.join(format!("devices/system/node/node{}/cpulist", node));
        let Some(list) = read_str(&cpulist) else {
            continue;
        };

        present_nodes += 1;

        let _ = parse_range_list_with(&list, |id| {
            if let Some((i, lp)) = lps
                .iter_mut()
                .enumerate()
                .find(|(_, lp)| lp.os_id as usize == id)
            {
                // A CPU listed under more than one node is not a real NUMA
                // partition (nodes must be disjoint) - degenerate/fake NUMA.
                if claimed[i] {
                    degenerate = true;
                }

                claimed[i] = true;
                lp.numa_node = node as u8;
            }
        });
    }

    let mut numa_node_count = present_nodes.min(u8::MAX as u32) as u8;

    if numa_node_count == 0 {
        numa_node_count = 1;
    }

    // Degenerate NUMA (a CPU appeared in multiple nodes): collapse to one domain.
    // Trigger seen in the wild is the Pi 5 / BCM2712: it splits RAM into 8 NUMA
    // nodes (~1 GB each) but EVERY CPU is in EVERY node and the whole distance
    // matrix is uniform (numactl: all 10) - i.e. zero access locality, one
    // effective domain. Confirmed by lscpu + numactl, no `numa=` override, 3 units;
    // not a parsing artifact. `numa_node_count` is a memory-domain contract, so N
    // phantom domains would mislead NUMA-aware placement. Real NUMA partitions CPUs
    // into disjoint nodes, so this never trips there. (A stricter signal would be
    // "all node distances equal"; the CPU-overlap test already covers this case.)
    if degenerate {
        numa_node_count = 1;

        for lp in lps.iter_mut() {
            lp.numa_node = 0;
        }
    }

    // --- 7. Vendor / model / features ---
    let mut vendor = Vendor::Unknown;
    let mut model_name = "Unknown".to_string();
    let mut cpu_features = CpuFeatures::default();

    #[cfg(target_arch = "x86_64")]
    crate::platform::common_x86_64::detect_via_cpuid(
        &mut vendor,
        &mut model_name,
        &mut cpu_features,
    );

    if vendor == Vendor::Unknown || model_name == "Unknown" || cpu_features.is_empty() {
        proc::detect_via_proc_cpuinfo(procfs_root, &mut vendor, &mut model_name, &mut cpu_features);
    }

    // --- 7b. Per-core microarch (ARM MIDR part) from /proc/cpuinfo ---
    // Runs UNCONDITIONALLY, independent of the identity fallback above: the
    // `CPU part` is per-core on heterogeneous ARM (big cores report a different
    // part than little cores), so it cannot ride the first-block identity read.
    // On x86 the file has no such field, so every LP stays 0.
    if let Ok(content) = std::fs::read_to_string(procfs_root.join("cpuinfo")) {
        for (os_id, part) in proc::parse_cpu_parts(&content) {
            if let Some(lp) = lps.iter_mut().find(|lp| lp.os_id == os_id) {
                lp.cpu_part = part;
            }
        }
    }

    // --- 8. Kind core counts ---
    let mut kind_core_counts = [0u16; CoreKind::COUNT];
    for lp in &lps {
        if lp.smt_index == 0 {
            kind_core_counts[lp.kind.index()] += 1;
        }
    }

    let mut info = CpuInfo {
        lps,
        core_count,
        socket_count,
        numa_node_count,
        kind_core_counts,
        l3_domains,
        l2_domains,
        l1d,
        l1i,
        l2,
        vendor,
        model_name,
        features: cpu_features,
    };
    info.normalize_domain_order();
    Ok(info)
}
