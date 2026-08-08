# Changelog

All notable changes to this project will be documented in this file.

## [0.2608.0] - Unreleased

**Migration guide: [docs/migrations/MIGRATION-0.2608.0.md](docs/migrations/MIGRATION-0.2608.0.md)**.
The Rust API has two mechanical breaks (`FallbackReason::BrokerRefused` gained its payload,
`AppliedPriority::from_parts` lost a parameter); the C ABI is unchanged, not even a recompile.

### 🚀 Features

- Android support (#14, thanks @mokurin000): the crate builds and runs on `aarch64-linux-android`.
  Detection reads the same sysfs surface as desktop Linux, affinity pinning and all priority levels
  work, and `promote_thread_to_realtime` honestly reports `NoBroker` - Android has no rtkit and no
  session bus, so the broker slot is empty by design, not unimplemented. Handles bionic's
  `CPU_SETSIZE` typing (`usize` there, `c_int` on glibc).
- `FromStr` for `AffinityMask` (#13, thanks @nazar-pc): parses kernel range lists with or without
  brackets - `"[0-3, 6-9]"`, `"0-3,6-9"`, `"5"`, `"[]"` and `""` all parse, so both `Display`
  spellings round-trip and a value copied out of `/sys/devices/system/cpu/online` parses unchanged.
  Parsing delegates to the same range-list parser detection uses, so user text inherits detection's
  strictness: `"0-3-5"` and `"3-0"` are rejected instead of silently reinterpreted, out-of-range ids
  are an error (unlike `Extend`'s documented silent drop), and a huge range like `"0-4000000000"` is
  rejected up front instead of expanding for minutes. Errors are the crate's own
  `Error::InvalidParameter` / `Error::InvalidCoreId`.
- `{:#}` alternate `Display` form for `AffinityMask`: the exact kernel spelling (`0-3,6-9`, empty
  string when empty) with no whitespace, safe for `taskset -c`, config files, and whitespace-separated
  group lists (`"0-7,16-23 8-15,24-31"` - one mask per thread pool - splits and round-trips; fed
  whole to `FromStr` it errors rather than silently merging the groups). Plain `{}` keeps the
  bracketed, slice-like rendering.

### 🚜 Refactor

- Reworked the rtkit integration: the priority-fallback vocabulary (`FallbackReason`, `BrokerError`,
  `Grant::Brokered`, `AppliedPriority::{reason, broker_error, degraded}`) now compiles on every
  target and feature set - consumers branch on it without any `cfg` - while the broker code itself is
  confined to the one target that has it (Linux with the `rtkit` feature). `BrokerRefused` carries
  the typed `BrokerError` as its payload, so "a broker error without a broker-refused reason" is
  unrepresentable: `from_parts` is infallible now and the serde deserializer needs no validation.

### ⚠️ Breaking (Rust)

- `FallbackReason::BrokerRefused` -> `BrokerRefused(BrokerError)`: match arms need the payload
  pattern; the `broker_error()` accessor is unchanged and now reads the payload.
- `AppliedPriority::from_parts` takes 5 arguments (the trailing `broker_error: Option<BrokerError>`
  is gone - put it inside the reason) and returns `Self` instead of `Option<Self>`.
- serde: the `AppliedPriority` shape changed (the `broker_error` field folded into the
  `BrokerRefused` payload), so values serialized by 0.2606.1 do not deserialize.

### 🧹 Maintenance

- Dependency bumps: `libc` 0.2.189, `bitflags` 2.13.1, `serde` 1.0.229.

## [0.2606.1] - 2026-06-21

**Migration guide: [docs/migrations/MIGRATION-0.2606.1.md](docs/migrations/MIGRATION-0.2606.1.md)**.
The Rust API is additive (no changes needed); the C ABI is a recompile.

### 🚀 Features

- First-class L2 cache domains (`L2Domain`, `CpuInfo::l2_domains`,
  `CpuInfo::l2_domain_mask`), mirroring the L3 domain model (#10). Each domain
  carries the cores sharing one L2 instance, its own `size_bytes` (so
  heterogeneous L2 sizes are exact, not collapsed to a per-kind average), and a
  `l3_domain` back-link to the L3 it nests inside - the finest "these cores are
  closest" grouping for slicing cooperating threads out of an L3 domain.
  Domains are ordered by ascending lowest member LP (stable, not a formal
  distance guarantee). New per-LP `Lp::l2_domain` index (`u16`, `Lp::NO_L2`
  sentinel; wider than `l3_domain` because L2 instances scale with core count).
- C ABI: `GdtCpusL2Domain`, `GdtCpusCpuInfo::l2_domain_count`,
  `GdtCpusLp::l2_domain`, `GDT_CPUS_NO_L2`, and `gdt_cpus_get_l2_domain` /
  `gdt_cpus_get_l2_domain_lp` accessors.
- New `l2_domains` example: pack N cooperating cores into the tightest cache
  neighborhood by taking whole L2 groups out of an L3 domain. `basic_info` now
  lists L2 domains and each LP's L2 domain, and prints domain membership as
  ranges.
- `impl Extend<usize> for AffinityMask`; `FromIterator` now delegates to it, and
  out-of-range ids (`>= MAX_LP_COUNT`) are silently dropped (#11).

### 🐛 Bug Fixes

- Normalize L3 and L2 domain order: both tables are sorted by ascending lowest
  member LP and every `Lp`'s domain index is remapped to match. On a sparse
  online set - e.g. a cpuset-limited container whose cache `shared_cpu_list`
  still names offline siblings - detection could otherwise emit the domains out
  of the documented order.

### 🚜 Refactor

- `AffinityMask` `Debug` and `Display` now render logical-processor sets as
  coalesced bracketed ranges. `Debug` is the developer view
  (`AffinityMask { cores: [0-3, 6-9], count: N }`); `Display` is the bare value
  (`[0-3, 6-9]`, `[]` when empty), so a mask can be printed with `{}` directly
  (#9).

### 📚 Documentation

- Update the C example repository URLs.

### ⚠️ Breaking (C ABI)

- `GdtCpusLp` and `GdtCpusCpuInfo` grew fields (`l2_domain`, `l2_domain_count`),
  so their `#[repr(C)]` layout changed. Recompile C consumers against the
  regenerated `gdt_cpus.h`.

### 🧹 Maintenance

- Bump license copyright year to 2024-2026.

## [0.2606.0] - 2026-06-18

**Migration guide: [docs/migrations/MIGRATION-0.2606.md](docs/migrations/MIGRATION-0.2606.md)**.

### 📅 Versioning

- Version scheme moves to `0.YYMM.patch` (#5). Cargo treated `25.5 -> 25.12`
  as a compatible minor bump under the old bare-CalVer scheme; `0.YYMM` makes
  every month a semver-breaking epoch.
- `gdt-cpus-sys` now versions in lockstep with `gdt-cpus`.

### 🚀 Features

- Flat topology model: per-LP `Lp` records replace the `SocketInfo`/`CoreInfo`
  nesting; sockets are derived counts, not containers.
- First-class L3 cache domains (`L3Domain`, `CpuInfo::l3_domain_mask`):
  chiplet CPUs report every CCD/cluster instead of one L3 per socket (#7).
- N-ary `CoreKind` (Performance / Efficiency / LpEfficiency) replaces the
  boolean-ish `CoreType`; this models 3-kind Intel hybrids and capacity tiers.
- NUMA node ids per LP + `numa_node_mask` (Linux node dirs, Windows
  `RelationNumaNode`) (#6).
- `CpuInfo::detect()` is the entry point and returns an owned `CpuInfo`.
- Removed the cached `cpu_info()` shim; applications own and reuse `CpuInfo`.
- Per-kind L1/L2 caches now report sharing degree via `CacheInfo::shared_by`.
- `CacheLevel` and `CacheType` are no longer part of the public API; cache
  level/type are implied by the `CpuInfo::l1d`, `l1i`, `l2`, and `l3_domains`
  fields.
- Per-core microarch id: `Lp::cpu_part` exposes the raw ARM MIDR part (e.g.
  `0x0d81` Cortex-A720) read per-core from the full `/proc/cpuinfo`; `0` on x86.
- `Lp::cpu_part` is mirrored into the `GdtCpusLp` C struct. The library exposes
  the raw part id but ships no part-name table.
- Expanded x86 feature flags: `POPCNT`, `BMI1`, `BMI2`, `F16C`.
- Expanded aarch64 feature flags: `FP16`, `DOTPROD`, `I8MM`, `BF16`, `SVE2`,
  `LSE`, `JSCVT`, `LRCPC`, `PMULL`, `RDM`, `FHM`, `FCMA`, `LSE2`, `LRCPC2`,
  `SM3`, `SM4`, `SVEAES`, `SVEPMULL`, `SVEBITPERM`, `SVESHA3`, `SVESM4`,
  `SVEI8MM`, and `SVEBF16`.
- Feature detection covers cpuid, `/proc/cpuinfo` tokens, macOS
  `hw.optional.arm.FEAT_*`, and Windows-on-ARM processor feature probes.
- `gdt-cpus-sys` C feature bits are extended to match. New bits are appended,
  so existing feature bits keep their values.
- `Lp::perf_hint`: ordinal within-kind performance ranking from Linux
  `cpu_capacity`, Windows `EfficiencyClass`, or macOS perflevel order.
- Linux: capacity-based kind classification with presence-tracked thresholds
  now classifies ARM big.LITTLE and asymmetric x86.
- Windows: `set_thread_soft_affinity` (CPU Sets) + hard affinity via
  `SetThreadGroupAffinity` (multi-group detection no longer outruns control).
- macOS: ordinary `set_thread_priority(TimeCritical)` may fall back to QoS
  `USER_INTERACTIVE`; the explicit realtime consent API reports denial.
- Shared fixture corpus (`GDT_CPUS_FIXTURES` or
  `testdata/gdt-cpus/fixtures`) plus fixture-driven detection tests.
- New example `l3_domains`: cross-CCD ping-pong benchmark for L3-domain
  placement (5950X sample: 53 ns in-domain vs 191 ns cross-domain round trips,
  3.6x).
- New example `thread_priorities`: priority capability and applied-priority
  introspection.
- Reworked example `reserved_core`: placement vs priority for a latency thread.
- Reworked example `frame_jitter`: physical-vs-logical worker-pool sizing.
- Reworked example `background_budget`: reserve the render LP and choose a
  frame-safe background worker budget.
- Reworked example `audio_latency`: priority sweep against a Normal worker pool
  spawned once.
- Examples now use ASCII-only output and compute their final takeaway from the
  measured run.
- Example placement uses topology records instead of `0..n` index arithmetic.
- Removed `parallel_tasks` and the old fake-IO `streaming_pipeline` example.
- New C example `examples/c/priority`: `AppliedPriority` and realtime consent
  API over the FFI.
- gdt-cpus-sys: C ABI rebuilt around the flat model (`GdtCpusLp`,
  `GdtCpusL3Domain`, `GdtCpusCoreKind`, per-kind cache getters, hard/soft
  affinity over LP-id arrays).
- gdt-cpus-sys: socket/core tree accessors are removed.
- Linux: pure-timeshare priority ladder (nice 19/10/5/0/-5/-10/-20); named
  priority levels no longer request `SCHED_RR`.
- Linux: when fully granted, `TimeCritical` holds about a 9x CFS weight edge
  over `Highest`, which holds about a 9x edge over `Normal`.
- Linux: denied negative nice cascades to rtkit `MakeThreadHighPriority`
  (feature `rtkit`, on by default), then reports the retained current
  timeshare state if no broker helped.
- The rtkit path uses a hand-rolled minimal D-Bus client and adds no new
  dependencies.
- The D-Bus client uses bounded call deadlines and tries every supported bus
  address entry instead of failing after the first bad address.
- New `promote_thread_to_realtime(budget)` / `demote_thread_from_realtime`:
  explicit `SCHED_RR` opt-in.
- Linux realtime promotion tries direct `SCHED_RR`, then the xdg realtime
  portal, then rtkit.
- Linux brokered realtime promotion sets `RLIMIT_RTTIME` before requesting the
  grant (soft = requested budget clamped to the hard limit; hard = current hard
  limit capped by the daemon ceiling).
- Linux direct realtime promotion uses `SCHED_RESET_ON_FORK`; zero-length
  realtime budgets are rejected as invalid parameters.
- New `priority_capabilities()`: predicts the effective rank of each priority
  level under current rlimits and broker reachability.
- `set_thread_priority` / `promote_thread_to_realtime` now return
  `AppliedPriority` instead of `()`.
- Linux `promote_thread_to_realtime` denial reports the retained timeshare
  state through `AppliedPriority` instead of flattening the broker result into
  a permission string.
- New `FallbackReason` enum. Denied elevation is returned as structured data in
  `AppliedPriority::reason()`, not as a hidden log line.
- `AppliedPriority::requested()`, `effective()`, `grant()`, and `reason()`
  describe what actually stuck.
- `AppliedPriority` fields are private; use accessors or rebuild stored data
  with `AppliedPriority::from_parts(...)`, which validates contradictory
  combinations.
- `AppliedPriority` serde deserialization validates the same invariants as
  `from_parts(...)`.
- New `BrokerError` enum plus `AppliedPriority::broker_error()` for broker
  refusals (`reason() == Some(BrokerRefused)`).
- Broker refusal causes are mirrored in the C ABI as `GdtCpusBrokerError` plus
  `gdt_cpus_broker_error_description()`.
- `AppliedPriority::mechanism()` reports which OS scheduling mechanism
  was applied.
- `Mechanism { policy: MechanismPolicy, value: i8 }` carries the platform
  scheduling mechanism as typed data (`Nice`, `SchedRr`, `SchedOther`, `Qos`,
  `WinPriority`).
- `Mechanism` implements `Display` for human output such as `nice -15` or
  `QoS UserInteractive`.
- Mechanism data is mirrored in the C ABI as `GdtCpusMechanism`,
  `GdtCpusMechanismPolicy`, and `GdtCpusAppliedPriority::mechanism`.
- `gdt-cpus-sys` forwards the `rtkit` feature to `gdt-cpus`; C consumers keep
  the brokered Linux priority path by default and can opt out with
  `default-features = false`.
- `#[must_use]` added to result-carrying public API (`AppliedPriority`,
  `PriorityCaps`, `AffinityMask`, `CpuInfo::detect()`, and key priority /
  affinity query methods) so discarded priority, capability, and affinity
  results warn.

### 🐛 Bug Fixes

- L3 attributed per socket (first-wins) lost CCD topology on chiplet CPUs (#7).
- Windows ANYSIZE `GroupMask` arrays: only the first group was read.
- Linux/Windows L3 detection now only treats Unified L3 cache relations as L3
  domains.
- macOS: eight `.expect()` panic paths on absent `hw.perflevel*` keys removed.
- macOS: `FEAT_SME` no longer mapped to the SVE flag (SME != SVE).
- x86_64: CRC32 feature now set from the SSE4.2 cpuid bit.
- `set_thread_affinity` no longer triggers a hidden full detection on Linux.
- Linux: the unprivileged negative-nice fallback matched the wrong errno
  (`EPERM`; the kernel returns `EACCES`).
- Linux: demotion from a brokered RT grant preserves `SCHED_RESET_ON_FORK`
  (clearing it needs `CAP_SYS_NICE`; a plain `SCHED_OTHER` switch failed
  `EPERM` on rtkit-granted threads).
- Linux: `set_thread_priority` no longer errors on permission denial.
- Linux: denied priority requests degrade to the level the thread actually has
  and return `AppliedPriority` with `reason = NoBroker` when no broker helped.
- Linux: only genuine `setpriority` failures are returned as errors.
- Linux: removed the old sign-gated `nice(0)` fallback, which could error on
  back-to-normal/reclaim paths and demote an already elevated thread to nice 0.
- Windows: priority API failures report as system-call failures, not affinity
  failures.

### 🚜 Refactor

- Removed the never-written scheduling-policies static.
- Removed three never-constructed error variants.
- Removed direct dependencies: `thiserror`, `log`, `nix`, `mach-sys`,
  `core-foundation`.
- Removed unused dev dependencies: `env_logger`, `rand`, `fastrand`.
- Switched the `flate2` dev dependency to the `zlib-rs` backend.
- `SchedulingPolicy` is no longer public; the remaining platform-local
  scheduling-policy helpers are internal implementation details.

## [2025.12.0] - 2025-12-03

### 🚀 Features

- Implement AffinityMask
- Adds core affinity masks in CpuInfo
- Adds iterator and debug/display impls
- Add set_thread_affinity API for multi-core affinity masks
- Add union and intersection ops to AffinityMask

### 🐛 Bug Fixes

- Correct core type detection on non-hybrid CPUs
- Core affinity setting logic on Linux

### 🚜 Refactor

- Drop thread pinning on macOS

### 📚 Documentation

- Fix clippy warnings
- Add comprehensive documentation for AffinityMask
- Update platform affinity docs for AffinityMask API

### 🧪 Testing

- Add unit tests for AffinityMask

### ⚙️ Miscellaneous Tasks

- Cargo fmt
- Bump deps
- Bump version to 25.12
- Adds git-cliff configuration file
- Add CHANGELOG.md

## [2025.5.0] - 2025-05-22

### ⚙️ Miscellaneous Tasks

- Import from private repo
- Last polish before open sourcing
- Fix ffi
