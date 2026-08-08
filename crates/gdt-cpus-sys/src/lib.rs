#![warn(missing_docs)]
//!
//! C-style Foreign Function Interface (FFI) for the `gdt-cpus` crate.
//!
//! Mirrors the flat topology model: one [`GdtCpusLp`] record per logical
//! processor, a first-class L3-domain table (CCDs/clusters), per-KIND caches,
//! and N-ary core kinds (Performance / Efficiency / LP-Efficiency). There is
//! no socket -> core object tree - socket membership is a field on each LP.
//!
//! Conventions:
//! - Functions return `i32` error codes (`GdtCpusErrorCode`); `0` = success.
//! - Results are written through out-pointers; `NULL` out-pointers return
//!   `InvalidParameter`.
//! - Detection runs once on first use and is cached for the process lifetime;
//!   returned strings (`vendor_name`, `model_name`) stay valid forever after.
//! - Affinity masks cross the FFI as arrays of OS logical-processor ids.
//!
//! # Usage Example (Conceptual C Code)
//! ```c
//! // GdtCpusCpuInfo info;
//! // if (gdt_cpus_cpu_info(&info) == GDT_CPUS_ERROR_CODE_SUCCESS) {
//! //     printf("CPU: %s, %llu cores, %llu L3 domains\n",
//! //            info.model_name, info.core_count, info.l3_domain_count);
//! // }
//! // // Pin to the first LP of L3 domain 0:
//! // uint32_t lp;
//! // gdt_cpus_get_l3_domain_lp(0, 0, &lp);
//! // gdt_cpus_pin_thread_to_core(lp);
//! ```

#![deny(missing_docs)]

use std::ffi::CString;
use std::os::raw::c_char;
use std::sync::OnceLock;

use gdt_cpus::{AffinityMask, CoreKind, ThreadPriority};

/// `l3_domain` value meaning "this LP belongs to no detected L3 domain".
pub const GDT_CPUS_NO_L3: u32 = u32::MAX;

/// `l2_domain` value meaning "this LP belongs to no detected L2 domain".
pub const GDT_CPUS_NO_L2: u32 = u32::MAX;

struct CpuInfoContainer {
    info: gdt_cpus::CpuInfo,
    model_name_storage: CString,
    vendor_name_storage: CString,
}

static CPU_INFO_CONTAINER: OnceLock<Result<CpuInfoContainer, gdt_cpus::Error>> = OnceLock::new();

fn container() -> Result<&'static CpuInfoContainer, GdtCpusErrorCode> {
    let result = CPU_INFO_CONTAINER.get_or_init(|| {
        let info = gdt_cpus::CpuInfo::detect()?;
        let model_name_storage = CString::new(info.model_name.clone()).unwrap_or_default();
        let vendor_name_storage = CString::new(info.vendor.to_string()).unwrap_or_default();
        Ok(CpuInfoContainer {
            info,
            model_name_storage,
            vendor_name_storage,
        })
    });
    match result {
        Ok(c) => Ok(c),
        Err(e) => Err(GdtCpusErrorCode::from(e)),
    }
}

macro_rules! get_info_validate_out_or_err {
    ($out:ident) => {{
        if $out.is_null() {
            return GdtCpusErrorCode::InvalidParameter as i32;
        }
        match container() {
            Ok(c) => c,
            Err(code) => return code as i32,
        }
    }};
}

/// C-compatible enumeration for error codes returned by FFI functions.
///
/// `Success` (0) indicates no error. Negative values indicate errors.
#[repr(C)]
#[derive(Debug, Copy, Clone)]
pub enum GdtCpusErrorCode {
    /// No error.
    Success = 0,
    /// Error during CPU detection.
    Detection = -1,
    /// Invalid core ID.
    InvalidCoreId = -2,
    /// Error setting thread affinity.
    Affinity = -4,
    /// Unsupported operation on this platform.
    Unsupported = -5,
    /// Permission denied.
    PermissionDenied = -6,
    /// System call error.
    SystemCall = -8,
    /// Resource not found.
    NotFound = -9,
    /// Invalid parameter (NULL pointer, value out of range).
    InvalidParameter = -10,
    /// Index out of bounds.
    OutOfBounds = -12,
    /// Unknown error.
    Unknown = -999,
}

impl From<&gdt_cpus::Error> for GdtCpusErrorCode {
    fn from(e: &gdt_cpus::Error) -> Self {
        match e {
            gdt_cpus::Error::Detection(_) => GdtCpusErrorCode::Detection,
            gdt_cpus::Error::InvalidCoreId(_) => GdtCpusErrorCode::InvalidCoreId,
            gdt_cpus::Error::Affinity(_) => GdtCpusErrorCode::Affinity,
            gdt_cpus::Error::Unsupported(_) => GdtCpusErrorCode::Unsupported,
            gdt_cpus::Error::PermissionDenied(_) => GdtCpusErrorCode::PermissionDenied,
            gdt_cpus::Error::SystemCall(_) => GdtCpusErrorCode::SystemCall,
            gdt_cpus::Error::NotFound(_) => GdtCpusErrorCode::NotFound,
            gdt_cpus::Error::InvalidParameter(_) => GdtCpusErrorCode::InvalidParameter,
        }
    }
}

/// C-compatible enumeration for CPU vendors.
#[repr(C)]
#[derive(Debug, Copy, Clone)]
pub enum GdtCpusVendor {
    /// Intel Corporation.
    Intel = 0,
    /// Advanced Micro Devices.
    Amd = 1,
    /// ARM reference designs.
    Arm = 2,
    /// Apple Silicon.
    Apple = 3,
    /// Unknown CPU vendor.
    Unknown = 4,
    /// Vendor not recognized by the library.
    Other = 5,
    /// Qualcomm (Snapdragon, Oryon).
    Qualcomm = 6,
    /// Broadcom.
    Broadcom = 7,
    /// NVIDIA.
    Nvidia = 8,
    /// Marvell / Cavium.
    Marvell = 9,
}

impl From<gdt_cpus::Vendor> for GdtCpusVendor {
    fn from(v: gdt_cpus::Vendor) -> Self {
        match v {
            gdt_cpus::Vendor::Intel => GdtCpusVendor::Intel,
            gdt_cpus::Vendor::Amd => GdtCpusVendor::Amd,
            gdt_cpus::Vendor::Arm => GdtCpusVendor::Arm,
            gdt_cpus::Vendor::Apple => GdtCpusVendor::Apple,
            gdt_cpus::Vendor::Qualcomm => GdtCpusVendor::Qualcomm,
            gdt_cpus::Vendor::Broadcom => GdtCpusVendor::Broadcom,
            gdt_cpus::Vendor::Nvidia => GdtCpusVendor::Nvidia,
            gdt_cpus::Vendor::Marvell => GdtCpusVendor::Marvell,
            gdt_cpus::Vendor::Other => GdtCpusVendor::Other,
            gdt_cpus::Vendor::Unknown => GdtCpusVendor::Unknown,
        }
    }
}

/// Performance/efficiency classification of a CPU core.
///
/// N-ary on purpose: modern silicon ships more than two kinds (Intel
/// P + E + LP-E, capacity tiers on ARM).
#[repr(C)]
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum GdtCpusCoreKind {
    /// Performance core (Intel P-core, ARM "big", Apple P).
    Performance = 0,
    /// Efficiency core (Intel E-core, ARM "LITTLE", AMD dense, Apple E).
    Efficiency = 1,
    /// Low-power efficiency core (Intel LP-E, lowest ARM capacity tier).
    LpEfficiency = 2,
    /// Unknown core kind (never produced on homogeneous machines -
    /// the classification invariant is "homogeneous means all Performance").
    Unknown = 3,
}

impl From<CoreKind> for GdtCpusCoreKind {
    fn from(k: CoreKind) -> Self {
        match k {
            CoreKind::Performance => GdtCpusCoreKind::Performance,
            CoreKind::Efficiency => GdtCpusCoreKind::Efficiency,
            CoreKind::LpEfficiency => GdtCpusCoreKind::LpEfficiency,
            CoreKind::Unknown => GdtCpusCoreKind::Unknown,
        }
    }
}

fn kind_from_ffi(kind: GdtCpusCoreKind) -> CoreKind {
    match kind {
        GdtCpusCoreKind::Performance => CoreKind::Performance,
        GdtCpusCoreKind::Efficiency => CoreKind::Efficiency,
        GdtCpusCoreKind::LpEfficiency => CoreKind::LpEfficiency,
        GdtCpusCoreKind::Unknown => CoreKind::Unknown,
    }
}

/// Reconstructs a `#[repr(C)]` FFI enum from the raw integer a C caller passed,
/// returning `None` for any value that is not a declared discriminant.
///
/// Every public entry point accepts the enum's `i32` repr (not the enum) by value
/// and validates it here. Taking the integer is what keeps the boundary sound:
/// materializing an out-of-range enum value at the ABI edge is undefined behavior.
macro_rules! ffi_enum_from_i32 {
    ($name:ident, $ty:ty, [$($variant:ident),+ $(,)?]) => {
        fn $name(v: i32) -> Option<$ty> {
            [$(<$ty>::$variant),+].into_iter().find(|&e| e as i32 == v)
        }
    };
}

ffi_enum_from_i32!(
    error_code_from_i32,
    GdtCpusErrorCode,
    [
        Success,
        Detection,
        InvalidCoreId,
        Affinity,
        Unsupported,
        PermissionDenied,
        SystemCall,
        NotFound,
        InvalidParameter,
        OutOfBounds,
        Unknown,
    ]
);
ffi_enum_from_i32!(
    vendor_from_i32,
    GdtCpusVendor,
    [
        Intel, Amd, Arm, Apple, Unknown, Other, Qualcomm, Broadcom, Nvidia, Marvell
    ]
);
ffi_enum_from_i32!(
    core_kind_ffi_from_i32,
    GdtCpusCoreKind,
    [Performance, Efficiency, LpEfficiency, Unknown]
);
ffi_enum_from_i32!(
    thread_priority_ffi_from_i32,
    GdtCpusThreadPriority,
    [
        Background,
        Lowest,
        BelowNormal,
        Normal,
        AboveNormal,
        Highest,
        TimeCritical
    ]
);
ffi_enum_from_i32!(grant_from_i32, GdtCpusGrant, [Direct, Brokered, Realtime]);
ffi_enum_from_i32!(
    fallback_reason_from_i32,
    GdtCpusFallbackReason,
    [None, NoBroker, BrokerTimedOut, BrokerRefused, Clamped]
);
ffi_enum_from_i32!(
    broker_error_from_i32,
    GdtCpusBrokerError,
    [
        None,
        AccessDenied,
        LimitsExceeded,
        InvalidArgs,
        Failed,
        Other
    ]
);

/// Validates a C `int` core-kind argument into the internal [`CoreKind`].
fn core_kind_from_i32(v: i32) -> Option<CoreKind> {
    core_kind_ffi_from_i32(v).map(kind_from_ffi)
}

/// Validates a C `int` priority argument into the internal [`ThreadPriority`].
fn thread_priority_from_i32(v: i32) -> Option<ThreadPriority> {
    thread_priority_ffi_from_i32(v).map(Into::into)
}

/// C-compatible enumeration for CPU features on x86_64 architecture (bitmask).
#[cfg(target_arch = "x86_64")]
#[repr(C)]
#[derive(Debug, Copy, Clone)]
pub enum GdtCpusCpuFeatures {
    /// MMX support.
    MMX = 0x00000001,
    /// SSE support.
    SSE = 0x00000002,
    /// SSE2 support.
    SSE2 = 0x00000004,
    /// SSE3 support.
    SSE3 = 0x00000008,
    /// SSSE3 support.
    SSSE3 = 0x00000010,
    /// SSE4.1 support.
    SSE4_1 = 0x00000020,
    /// SSE4.2 support.
    SSE4_2 = 0x00000040,
    /// FMA3 support.
    FMA3 = 0x00000080,
    /// AVX support.
    AVX = 0x00000100,
    /// AVX2 support.
    AVX2 = 0x00000200,
    /// AVX-512 Foundation support.
    AVX512F = 0x00000400,
    /// AVX-512 Byte and Word Instructions support.
    AVX512BW = 0x00000800,
    /// AVX-512 Conflict Detection Instructions support.
    AVX512CD = 0x00001000,
    /// AVX-512 Doubleword and Quadword Instructions support.
    AVX512DQ = 0x00002000,
    /// AVX-512 Vector Length Extensions support.
    AVX512VL = 0x00004000,
    /// AES hardware acceleration support.
    AES = 0x00008000,
    /// SHA hardware acceleration support.
    SHA = 0x00010000,
    /// CRC32 hardware acceleration support.
    CRC32 = 0x00020000,
    /// POPCNT (population count) support.
    POPCNT = 0x00040000,
    /// BMI1 (bit manipulation 1: ANDN/BLSI/TZCNT) support.
    BMI1 = 0x00080000,
    /// BMI2 (bit manipulation 2: PDEP/PEXT/BZHI) support.
    BMI2 = 0x00100000,
    /// F16C (half-precision float conversion) support.
    F16C = 0x00200000,
}

/// C-compatible enumeration for CPU features on aarch64 architecture (bitmask).
#[cfg(target_arch = "aarch64")]
#[repr(C)]
#[derive(Debug, Copy, Clone)]
pub enum GdtCpusCpuFeatures {
    /// NEON (Advanced SIMD) support.
    NEON = 0x00000001,
    /// SVE (Scalable Vector Extension) support.
    SVE = 0x00000002,
    /// AES hardware acceleration support.
    AES = 0x00000004,
    /// SHA hardware acceleration support (SHA1/SHA256/SHA512).
    SHA = 0x00000008,
    /// CRC32 hardware acceleration support.
    CRC32 = 0x00000010,
    /// FP16 (half-precision arithmetic, FEAT_FP16) support.
    FP16 = 0x00000020,
    /// DotProd (int8 dot product, FEAT_DotProd) support.
    DOTPROD = 0x00000040,
    /// I8MM (int8 matrix multiply, FEAT_I8MM) support.
    I8MM = 0x00000080,
    /// BF16 (bfloat16, FEAT_BF16) support.
    BF16 = 0x00000100,
    /// SVE2 (Scalable Vector Extension 2) support.
    SVE2 = 0x00000200,
    /// LSE (Large System Extensions atomics, FEAT_LSE) support.
    LSE = 0x00000400,
    /// JSCVT (JavaScript conversion instruction, FEAT_JSCVT) support.
    JSCVT = 0x00000800,
    /// LRCPC (Load-Acquire RCpc instructions, FEAT_LRCPC) support.
    LRCPC = 0x00001000,
    /// PMULL (polynomial multiply, FEAT_PMULL) support.
    PMULL = 0x00002000,
    /// RDM (rounding doubling multiply-add, FEAT_RDM) support.
    RDM = 0x00004000,
    /// FHM (half-precision multiply-add, FEAT_FHM) support.
    FHM = 0x00008000,
    /// FCMA (floating-point complex multiply-add, FEAT_FCMA) support.
    FCMA = 0x00010000,
    /// LSE2 (Large System Extensions 2 atomics, FEAT_LSE2) support.
    LSE2 = 0x00020000,
    /// LRCPC2 (immediate-offset RCpc load-acquire, FEAT_LRCPC2) support.
    LRCPC2 = 0x00040000,
    /// SM3 cryptographic hash instructions (FEAT_SM3) support.
    SM3 = 0x00080000,
    /// SM4 cryptographic cipher instructions (FEAT_SM4) support.
    SM4 = 0x00100000,
    /// SVE AES instructions (FEAT_SVE_AES) support.
    SVEAES = 0x00200000,
    /// SVE PMULL instructions (FEAT_SVE_PMULL128) support.
    SVEPMULL = 0x00400000,
    /// SVE bit permutation instructions (FEAT_SVE_BitPerm) support.
    SVEBITPERM = 0x00800000,
    /// SVE SHA3 instructions (FEAT_SVE_SHA3) support.
    SVESHA3 = 0x01000000,
    /// SVE SM4 instructions (FEAT_SVE_SM4) support.
    SVESM4 = 0x02000000,
    /// SVE I8MM instructions (FEAT_SVE_I8MM) support.
    SVEI8MM = 0x04000000,
    /// SVE BF16 instructions (FEAT_SVE_BF16) support.
    SVEBF16 = 0x08000000,
}

/// Thread priority levels (7 portable levels mapped onto each OS scheduler).
///
/// Linux: a pure timeshare-nice ladder (19/10/5/0/-5/-10/-20); negative nice is
/// negotiated through rtkit when the direct syscall is denied. Pass the
/// `GdtCpusAppliedPriority` out-param of `gdt_cpus_set_thread_priority` to learn
/// what actually stuck. macOS: QoS classes (real-time via the consent API,
/// `gdt_cpus_promote_thread_to_realtime`). Windows:
/// `THREAD_PRIORITY_IDLE..TIME_CRITICAL`.
#[repr(C)]
#[derive(Debug, Copy, Clone)]
pub enum GdtCpusThreadPriority {
    /// Background noise (cloud saves, achievement sync).
    Background = 0,
    /// Analytics, telemetry.
    Lowest = 1,
    /// Async workers, AI planning.
    BelowNormal = 2,
    /// Asset loading/streaming.
    Normal = 3,
    /// Game logic helpers.
    AboveNormal = 4,
    /// Render/audio-adjacent workers.
    Highest = 5,
    /// Audio mixer thread territory. Linux uses the strongest timeshare slot;
    /// explicit real-time is `gdt_cpus_promote_thread_to_realtime`.
    TimeCritical = 6,
}

impl From<GdtCpusThreadPriority> for ThreadPriority {
    fn from(p: GdtCpusThreadPriority) -> Self {
        match p {
            GdtCpusThreadPriority::Background => ThreadPriority::Background,
            GdtCpusThreadPriority::Lowest => ThreadPriority::Lowest,
            GdtCpusThreadPriority::BelowNormal => ThreadPriority::BelowNormal,
            GdtCpusThreadPriority::Normal => ThreadPriority::Normal,
            GdtCpusThreadPriority::AboveNormal => ThreadPriority::AboveNormal,
            GdtCpusThreadPriority::Highest => ThreadPriority::Highest,
            GdtCpusThreadPriority::TimeCritical => ThreadPriority::TimeCritical,
        }
    }
}

impl From<ThreadPriority> for GdtCpusThreadPriority {
    fn from(p: ThreadPriority) -> Self {
        match p {
            ThreadPriority::Background => GdtCpusThreadPriority::Background,
            ThreadPriority::Lowest => GdtCpusThreadPriority::Lowest,
            ThreadPriority::BelowNormal => GdtCpusThreadPriority::BelowNormal,
            ThreadPriority::Normal => GdtCpusThreadPriority::Normal,
            ThreadPriority::AboveNormal => GdtCpusThreadPriority::AboveNormal,
            ThreadPriority::Highest => GdtCpusThreadPriority::Highest,
            ThreadPriority::TimeCritical => GdtCpusThreadPriority::TimeCritical,
        }
    }
}

/// How a thread-priority request was satisfied - which tier it landed in,
/// orthogonal to whether it fell short (that is [`GdtCpusFallbackReason`]).
#[repr(C)]
#[derive(Debug, Copy, Clone)]
pub enum GdtCpusGrant {
    /// Applied directly by the OS scheduler.
    Direct = 0,
    /// Negotiated through a privilege broker (Linux rtkit / xdg portal).
    Brokered = 1,
    /// A real-time policy was engaged (macOS SCHED_RR, or the consent API).
    Realtime = 2,
}

impl From<gdt_cpus::Grant> for GdtCpusGrant {
    fn from(g: gdt_cpus::Grant) -> Self {
        match g {
            gdt_cpus::Grant::Direct => GdtCpusGrant::Direct,
            gdt_cpus::Grant::Brokered => GdtCpusGrant::Brokered,
            gdt_cpus::Grant::Realtime => GdtCpusGrant::Realtime,
        }
    }
}

/// Why a priority request didn't get a clean, direct grant of what was asked.
///
/// `None` (0) is the no-news sentinel: you got the requested level. The rest
/// answer "my engine feels wonky on this box - what did my priority actually
/// do?" as DATA, never a hidden log line. `NoBroker`/`BrokerTimedOut`/
/// `BrokerRefused` mean you fell back to `Normal`; `Clamped` kept the level but
/// at the broker's ceiling (rtkit's `MinNiceLevel`, default -15).
#[repr(C)]
#[derive(Debug, Copy, Clone)]
pub enum GdtCpusFallbackReason {
    /// Clean grant - exactly what was requested. No fallback.
    None = 0,
    /// Direct denied and no broker available. Fell back to `Normal`.
    NoBroker = 1,
    /// Broker reached but timed out (transient). Fell back to `Normal`.
    BrokerTimedOut = 2,
    /// Broker reached and refused (policy / rate limit). Fell back to `Normal`.
    BrokerRefused = 3,
    /// Broker granted but weaker than asked (hit its ceiling). Kept the level.
    Clamped = 4,
}

impl From<gdt_cpus::FallbackReason> for GdtCpusFallbackReason {
    fn from(r: gdt_cpus::FallbackReason) -> Self {
        match r {
            gdt_cpus::FallbackReason::NoBroker => GdtCpusFallbackReason::NoBroker,
            gdt_cpus::FallbackReason::BrokerTimedOut => GdtCpusFallbackReason::BrokerTimedOut,
            // The C ABI flattens the refusal detail into the sibling
            // `broker_error` field, so the payload is dropped here on purpose.
            gdt_cpus::FallbackReason::BrokerRefused(_) => GdtCpusFallbackReason::BrokerRefused,
            gdt_cpus::FallbackReason::Clamped => GdtCpusFallbackReason::Clamped,
        }
    }
}

/// The typed reason a broker REFUSED a grant - the C mirror of Rust's
/// `BrokerError`, carried by `GdtCpusAppliedPriority::broker_error`.
///
/// `None` (0) is the sentinel: no broker refusal (the grant succeeded, or it
/// failed some other way - see `reason`). The rest are set only when
/// `reason == BrokerRefused`. Branch on `AccessDenied` (policy / no session -
/// give up) vs `LimitsExceeded` (rate limit - back off and retry).
#[repr(C)]
#[derive(Debug, Copy, Clone)]
pub enum GdtCpusBrokerError {
    /// No broker refusal (clean grant, or a non-refusal failure).
    None = 0,
    /// Policy denied - polkit, or no active/seated login session. Give up.
    AccessDenied = 1,
    /// Broker rate limit hit. Transient - back off and retry.
    LimitsExceeded = 2,
    /// Broker rejected the arguments (bad priority / version skew).
    InvalidArgs = 3,
    /// Generic daemon-side failure.
    Failed = 4,
    /// An error name this version doesn't map (cause is in the rtkit journal).
    Other = 5,
}

impl From<gdt_cpus::BrokerError> for GdtCpusBrokerError {
    fn from(e: gdt_cpus::BrokerError) -> Self {
        match e {
            gdt_cpus::BrokerError::AccessDenied => GdtCpusBrokerError::AccessDenied,
            gdt_cpus::BrokerError::LimitsExceeded => GdtCpusBrokerError::LimitsExceeded,
            gdt_cpus::BrokerError::InvalidArgs => GdtCpusBrokerError::InvalidArgs,
            gdt_cpus::BrokerError::Failed => GdtCpusBrokerError::Failed,
            // `BrokerError` is `#[non_exhaustive]`: any name added later maps here.
            _ => GdtCpusBrokerError::Other,
        }
    }
}

/// Which OS scheduler API set a thread's priority - the C mirror of Rust's
/// `MechanismPolicy`. Says how to read `GdtCpusMechanism::value`.
#[repr(C)]
#[derive(Debug, Copy, Clone)]
pub enum GdtCpusMechanismPolicy {
    /// Linux `SCHED_OTHER` setpriority - `value` is the nice (-20..19).
    Nice = 0,
    /// `SCHED_RR` real-time - `value` is the RR priority (Linux RT, macOS TimeCritical).
    SchedRr = 1,
    /// POSIX `SCHED_OTHER` sched_priority band - `value` is the band (macOS QoS opt-out).
    SchedOther = 2,
    /// macOS QoS - `value` is a QoS class ordinal (0 background .. 4 user_interactive).
    Qos = 3,
    /// Windows `SetThreadPriority` - `value` is the THREAD_PRIORITY constant (-15..15).
    WinPriority = 4,
}

impl From<gdt_cpus::MechanismPolicy> for GdtCpusMechanismPolicy {
    fn from(p: gdt_cpus::MechanismPolicy) -> Self {
        match p {
            gdt_cpus::MechanismPolicy::Nice => GdtCpusMechanismPolicy::Nice,
            gdt_cpus::MechanismPolicy::SchedRr => GdtCpusMechanismPolicy::SchedRr,
            gdt_cpus::MechanismPolicy::SchedOther => GdtCpusMechanismPolicy::SchedOther,
            gdt_cpus::MechanismPolicy::Qos => GdtCpusMechanismPolicy::Qos,
            gdt_cpus::MechanismPolicy::WinPriority => GdtCpusMechanismPolicy::WinPriority,
        }
    }
}

/// The concrete OS scheduling mechanism a request landed on - the C mirror of
/// Rust's `Mechanism`. `value` is read per `policy` (nice / RR priority / QoS
/// class ordinal / sched_priority band / THREAD_PRIORITY constant).
#[repr(C)]
#[derive(Debug, Copy, Clone)]
pub struct GdtCpusMechanism {
    /// Which OS scheduler API set the priority.
    pub policy: GdtCpusMechanismPolicy,
    /// The applied parameter, interpreted per `policy`.
    pub value: i8,
}

impl From<gdt_cpus::Mechanism> for GdtCpusMechanism {
    fn from(m: gdt_cpus::Mechanism) -> Self {
        GdtCpusMechanism {
            policy: m.policy.into(),
            value: m.value,
        }
    }
}

/// What a thread-priority request actually produced - the C mirror of Rust's
/// `AppliedPriority`.
///
/// A "successful" set on Linux can still mean you silently landed on `Normal`,
/// so for audio/render threads branch on `grant`/`reason`, not just the return
/// code. `reason == None` is a clean grant; `effective` differs from `requested`
/// only on a true fallback (a `Clamped` keeps the level, just weaker).
#[repr(C)]
#[derive(Debug, Copy, Clone)]
pub struct GdtCpusAppliedPriority {
    /// The level the caller requested.
    pub requested: GdtCpusThreadPriority,
    /// The level actually in effect (weaker than `requested` = a fallback).
    pub effective: GdtCpusThreadPriority,
    /// How the request was satisfied.
    pub grant: GdtCpusGrant,
    /// Why it fell short, or `None` for a clean grant.
    pub reason: GdtCpusFallbackReason,
    /// The typed broker-refusal reason - set (non-`None`) only when
    /// `reason == BrokerRefused`.
    pub broker_error: GdtCpusBrokerError,
    /// The concrete OS scheduling mechanism the request landed on (typed data).
    pub mechanism: GdtCpusMechanism,
}

impl From<&gdt_cpus::AppliedPriority> for GdtCpusAppliedPriority {
    fn from(a: &gdt_cpus::AppliedPriority) -> Self {
        GdtCpusAppliedPriority {
            requested: a.requested().into(),
            effective: a.effective().into(),
            grant: a.grant().into(),
            reason: a.reason().map_or(GdtCpusFallbackReason::None, Into::into),
            broker_error: a
                .broker_error()
                .map_or(GdtCpusBrokerError::None, Into::into),
            mechanism: a.mechanism().into(),
        }
    }
}

/// What each priority level will resolve to under this process's privileges -
/// the C mirror of Rust's `PriorityCaps`. A planning hint (rtkit can withdraw
/// cooperation later), not a contract.
#[repr(C)]
#[derive(Debug, Copy, Clone)]
pub struct GdtCpusPriorityCaps {
    /// Effective strength rank per level, indexed by `GdtCpusThreadPriority`
    /// (`Background` = 0 … `TimeCritical` = 6); higher = stronger. Two levels
    /// with the same rank are indistinguishable on this box.
    pub effective_rank: [u8; 7],
    /// Number of effectively distinct levels (7 = the full ladder works; fewer
    /// means the top collapsed, e.g. unprivileged Linux without rtkit).
    pub distinct_levels: u8,
}

/// Size, line size and sharing degree of one cache instance.
///
/// `size_bytes == 0` means "not detected". `shared_by` is the number of
/// logical processors sharing ONE instance of this cache (2 = core-private
/// with SMT; >2 = cluster-shared, e.g. Intel E-core L2).
#[repr(C)]
#[derive(Debug, Copy, Clone)]
pub struct GdtCpusCacheInfo {
    /// Total size in bytes (0 = not detected).
    pub size_bytes: u64,
    /// Cache line size in bytes (typically 64).
    pub line_bytes: u32,
    /// Logical processors sharing one instance of this cache.
    pub shared_by: u32,
}

impl From<&gdt_cpus::CacheInfo> for GdtCpusCacheInfo {
    fn from(c: &gdt_cpus::CacheInfo) -> Self {
        GdtCpusCacheInfo {
            size_bytes: c.size_bytes,
            line_bytes: c.line_bytes as u32,
            shared_by: c.shared_by as u32,
        }
    }
}

/// One record per ONLINE logical processor - the flat topology's atom.
#[repr(C)]
#[derive(Debug, Copy, Clone)]
pub struct GdtCpusLp {
    /// OS logical-processor id. Affinity addresses THESE ids.
    pub os_id: u32,
    /// Dense library-assigned physical core index, `0..core_count`.
    pub core: u32,
    /// Dense socket index.
    pub socket: u32,
    /// Index into the L3-domain table, or `GDT_CPUS_NO_L3`.
    pub l3_domain: u32,
    /// Index into the L2-domain table, or `GDT_CPUS_NO_L2`.
    pub l2_domain: u32,
    /// OS NUMA node id (0 on single-node systems and macOS).
    pub numa_node: u32,
    /// Performance/efficiency classification of this LP's physical core.
    pub kind: GdtCpusCoreKind,
    /// 0 = first SMT sibling on its physical core.
    pub smt_index: u32,
    /// Relative performance hint - ordinal and machine-local: higher = faster
    /// core on THIS machine, equal = indistinguishable, scale differs per OS
    /// (Linux cpu_capacity 0-1024, Windows EfficiencyClass, macOS perflevel
    /// order). 0 = no finer signal than `kind`. Use to pick the best cores
    /// within a kind (ARM prime-vs-mid, Intel favored cores).
    pub perf_hint: u32,
    /// Raw ARM MIDR part number of this core's microarch (e.g. 0x0d0b =
    /// Cortex-A76), read per-core from /proc/cpuinfo. 0 when absent (x86, or not
    /// reported). With the chip vendor (= MIDR implementer) it names the
    /// microarchitecture; no part->name table is shipped. NOT a kind signal.
    pub cpu_part: u32,
}

/// A set of cores sharing one L3 cache instance (a CCD on chiplet AMD,
/// a cluster on hybrid Intel). Enumerate its LPs with
/// [`gdt_cpus_get_l3_domain_lp`].
#[repr(C)]
#[derive(Debug, Copy, Clone)]
pub struct GdtCpusL3Domain {
    /// Size of this L3 instance in bytes.
    pub size_bytes: u64,
    /// Physical cores in this domain (SMT siblings counted once).
    pub core_count: u32,
    /// Logical processors in this domain.
    pub lp_count: u32,
}

/// A set of cores sharing one L2 cache instance (the finest "these cores are
/// closest" grouping - a core and its SMT siblings, or an efficiency-core
/// cluster). Enumerate its LPs with [`gdt_cpus_get_l2_domain_lp`].
#[repr(C)]
#[derive(Debug, Copy, Clone)]
pub struct GdtCpusL2Domain {
    /// Size of this L2 instance in bytes.
    pub size_bytes: u64,
    /// Physical cores in this domain (SMT siblings counted once).
    pub core_count: u32,
    /// Logical processors in this domain.
    pub lp_count: u32,
    /// Index of the parent L3 domain these cores share, or `GDT_CPUS_NO_L3`.
    pub l3_domain: u32,
}

/// Top-level CPU summary. Per-LP records, L3 domains and per-kind caches are
/// read through the accessor functions.
#[repr(C)]
pub struct GdtCpusCpuInfo {
    /// Detected CPU vendor.
    pub vendor: GdtCpusVendor,
    /// Null-terminated vendor name; valid for the process lifetime.
    pub vendor_name: *const c_char,
    /// Null-terminated model name; valid for the process lifetime.
    pub model_name: *const c_char,
    /// Bitmask of CPU features (see `GdtCpusCpuFeatures` for the bits).
    pub features: u32,
    /// Number of online logical processors.
    pub lp_count: u64,
    /// Number of physical cores (SMT siblings counted once).
    pub core_count: u64,
    /// Number of CPU sockets.
    pub socket_count: u64,
    /// Number of NUMA nodes (1 on single-node systems).
    pub numa_node_count: u64,
    /// Number of L3 cache domains (CCDs/clusters).
    pub l3_domain_count: u64,
    /// Number of L2 cache domains.
    pub l2_domain_count: u64,
    /// Physical cores classified as Performance.
    pub performance_cores: u64,
    /// Physical cores classified as Efficiency.
    pub efficiency_cores: u64,
    /// Physical cores classified as Low-Power Efficiency.
    pub lp_efficiency_cores: u64,
}

// ---------------------------------------------------------------------------
// Description helpers
// ---------------------------------------------------------------------------

/// Returns a static, null-terminated description of an error code.
#[unsafe(no_mangle)]
pub extern "C" fn gdt_cpus_error_code_description(error_code: i32) -> *const c_char {
    let Some(error_code) = error_code_from_i32(error_code) else {
        return std::ptr::null();
    };
    let s: &'static [u8] = match error_code {
        GdtCpusErrorCode::Success => b"Success\0",
        GdtCpusErrorCode::Detection => b"CPU detection error\0",
        GdtCpusErrorCode::InvalidCoreId => b"Invalid core ID\0",
        GdtCpusErrorCode::Affinity => b"Thread affinity error\0",
        GdtCpusErrorCode::Unsupported => b"Unsupported operation\0",
        GdtCpusErrorCode::PermissionDenied => b"Permission denied\0",
        GdtCpusErrorCode::SystemCall => b"System call error\0",
        GdtCpusErrorCode::NotFound => b"Not found\0",
        GdtCpusErrorCode::InvalidParameter => b"Invalid parameter\0",
        GdtCpusErrorCode::OutOfBounds => b"Index out of bounds\0",
        GdtCpusErrorCode::Unknown => b"Unknown error\0",
    };
    s.as_ptr() as *const c_char
}

/// Returns a static, null-terminated description of a vendor.
#[unsafe(no_mangle)]
pub extern "C" fn gdt_cpus_vendor_description(vendor: i32) -> *const c_char {
    let Some(vendor) = vendor_from_i32(vendor) else {
        return std::ptr::null();
    };
    let s: &'static [u8] = match vendor {
        GdtCpusVendor::Intel => b"Intel\0",
        GdtCpusVendor::Amd => b"AMD\0",
        GdtCpusVendor::Arm => b"ARM\0",
        GdtCpusVendor::Apple => b"Apple\0",
        GdtCpusVendor::Qualcomm => b"Qualcomm\0",
        GdtCpusVendor::Broadcom => b"Broadcom\0",
        GdtCpusVendor::Nvidia => b"NVIDIA\0",
        GdtCpusVendor::Marvell => b"Marvell\0",
        GdtCpusVendor::Other => b"Other\0",
        GdtCpusVendor::Unknown => b"Unknown\0",
    };
    s.as_ptr() as *const c_char
}

/// Returns a static, null-terminated description of a core kind.
#[unsafe(no_mangle)]
pub extern "C" fn gdt_cpus_core_kind_description(kind: i32) -> *const c_char {
    let Some(kind) = core_kind_ffi_from_i32(kind) else {
        return std::ptr::null();
    };
    let s: &'static [u8] = match kind {
        GdtCpusCoreKind::Performance => b"Performance\0",
        GdtCpusCoreKind::Efficiency => b"Efficiency\0",
        GdtCpusCoreKind::LpEfficiency => b"LP-Efficiency\0",
        GdtCpusCoreKind::Unknown => b"Unknown\0",
    };
    s.as_ptr() as *const c_char
}

/// Returns a static, null-terminated description of a thread priority level.
#[unsafe(no_mangle)]
pub extern "C" fn gdt_cpus_thread_priority_description(priority: i32) -> *const c_char {
    let Some(priority) = thread_priority_ffi_from_i32(priority) else {
        return std::ptr::null();
    };
    let s: &'static [u8] = match priority {
        GdtCpusThreadPriority::Background => b"Background\0",
        GdtCpusThreadPriority::Lowest => b"Lowest\0",
        GdtCpusThreadPriority::BelowNormal => b"BelowNormal\0",
        GdtCpusThreadPriority::Normal => b"Normal\0",
        GdtCpusThreadPriority::AboveNormal => b"AboveNormal\0",
        GdtCpusThreadPriority::Highest => b"Highest\0",
        GdtCpusThreadPriority::TimeCritical => b"TimeCritical\0",
    };
    s.as_ptr() as *const c_char
}

// ---------------------------------------------------------------------------
// Detection
// ---------------------------------------------------------------------------

/// Fills `out_info` with the CPU summary. The C wrapper owns a process-local
/// snapshot for pointer stability.
///
/// # Safety
/// `out_info` must point to a valid `GdtCpusCpuInfo`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn gdt_cpus_cpu_info(out_info: *mut GdtCpusCpuInfo) -> i32 {
    let c = get_info_validate_out_or_err!(out_info);
    let info = &c.info;
    unsafe {
        *out_info = GdtCpusCpuInfo {
            vendor: info.vendor.into(),
            vendor_name: c.vendor_name_storage.as_ptr(),
            model_name: c.model_name_storage.as_ptr(),
            features: info.features.bits(),
            lp_count: info.lps.len() as u64,
            core_count: info.core_count as u64,
            socket_count: info.socket_count as u64,
            numa_node_count: info.numa_node_count as u64,
            l3_domain_count: info.l3_domains.len() as u64,
            l2_domain_count: info.l2_domains.len() as u64,
            performance_cores: info.num_performance_cores() as u64,
            efficiency_cores: info.num_efficiency_cores() as u64,
            lp_efficiency_cores: info.num_lp_efficiency_cores() as u64,
        };
    }
    GdtCpusErrorCode::Success as i32
}

/// Writes `true` if more than one core kind is present.
///
/// # Safety
/// `out_is_hybrid` must point to a valid `bool`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn gdt_cpus_is_hybrid(out_is_hybrid: *mut bool) -> i32 {
    let c = get_info_validate_out_or_err!(out_is_hybrid);
    unsafe { *out_is_hybrid = c.info.is_hybrid() };
    GdtCpusErrorCode::Success as i32
}

/// Fills `out_lp` with the logical-processor record at `index`
/// (`0..lp_count`, detection order - sorted by OS id).
///
/// # Safety
/// `out_lp` must point to a valid `GdtCpusLp`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn gdt_cpus_get_lp(index: u64, out_lp: *mut GdtCpusLp) -> i32 {
    let c = get_info_validate_out_or_err!(out_lp);
    let Some(lp) = c.info.lps.get(index as usize) else {
        return GdtCpusErrorCode::OutOfBounds as i32;
    };
    unsafe {
        *out_lp = GdtCpusLp {
            os_id: lp.os_id as u32,
            core: lp.core as u32,
            socket: lp.socket as u32,
            l3_domain: if lp.l3_domain == gdt_cpus::Lp::NO_L3 {
                GDT_CPUS_NO_L3
            } else {
                lp.l3_domain as u32
            },
            l2_domain: if lp.l2_domain == gdt_cpus::Lp::NO_L2 {
                GDT_CPUS_NO_L2
            } else {
                lp.l2_domain as u32
            },
            numa_node: lp.numa_node as u32,
            kind: lp.kind.into(),
            smt_index: lp.smt_index as u32,
            perf_hint: lp.perf_hint as u32,
            cpu_part: lp.cpu_part as u32,
        };
    }
    GdtCpusErrorCode::Success as i32
}

/// Fills `out_domain` with the L3 domain at `index` (`0..l3_domain_count`).
///
/// # Safety
/// `out_domain` must point to a valid `GdtCpusL3Domain`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn gdt_cpus_get_l3_domain(
    index: u64,
    out_domain: *mut GdtCpusL3Domain,
) -> i32 {
    let c = get_info_validate_out_or_err!(out_domain);
    let Some(d) = c.info.l3_domains.get(index as usize) else {
        return GdtCpusErrorCode::OutOfBounds as i32;
    };
    unsafe {
        *out_domain = GdtCpusL3Domain {
            size_bytes: d.size_bytes,
            core_count: d.core_count as u32,
            lp_count: d.mask.count() as u32,
        };
    }
    GdtCpusErrorCode::Success as i32
}

/// Writes the OS id of the `lp_index`-th logical processor (ascending) of L3
/// domain `domain_index`. Use with `GdtCpusL3Domain::lp_count` to enumerate a
/// domain's LPs - e.g. to build a per-CCD affinity set.
///
/// # Safety
/// `out_os_id` must point to a valid `u32`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn gdt_cpus_get_l3_domain_lp(
    domain_index: u64,
    lp_index: u64,
    out_os_id: *mut u32,
) -> i32 {
    let c = get_info_validate_out_or_err!(out_os_id);
    let Some(d) = c.info.l3_domains.get(domain_index as usize) else {
        return GdtCpusErrorCode::OutOfBounds as i32;
    };
    let Some(os_id) = d.mask.iter().nth(lp_index as usize) else {
        return GdtCpusErrorCode::OutOfBounds as i32;
    };
    unsafe { *out_os_id = os_id as u32 };
    GdtCpusErrorCode::Success as i32
}

/// Fills `out_domain` with the L2 domain at `index` (`0..l2_domain_count`).
///
/// # Safety
/// `out_domain` must point to a valid `GdtCpusL2Domain`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn gdt_cpus_get_l2_domain(
    index: u64,
    out_domain: *mut GdtCpusL2Domain,
) -> i32 {
    let c = get_info_validate_out_or_err!(out_domain);
    let Some(d) = c.info.l2_domains.get(index as usize) else {
        return GdtCpusErrorCode::OutOfBounds as i32;
    };
    unsafe {
        *out_domain = GdtCpusL2Domain {
            size_bytes: d.size_bytes,
            core_count: d.core_count as u32,
            lp_count: d.mask.count() as u32,
            l3_domain: if d.l3_domain == gdt_cpus::Lp::NO_L3 {
                GDT_CPUS_NO_L3
            } else {
                d.l3_domain as u32
            },
        };
    }
    GdtCpusErrorCode::Success as i32
}

/// Writes the OS id of the `lp_index`-th logical processor (ascending) of L2
/// domain `domain_index`. Use with `GdtCpusL2Domain::lp_count` to enumerate a
/// domain's LPs - e.g. to pin cooperating threads to one L2.
///
/// # Safety
/// `out_os_id` must point to a valid `u32`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn gdt_cpus_get_l2_domain_lp(
    domain_index: u64,
    lp_index: u64,
    out_os_id: *mut u32,
) -> i32 {
    let c = get_info_validate_out_or_err!(out_os_id);
    let Some(d) = c.info.l2_domains.get(domain_index as usize) else {
        return GdtCpusErrorCode::OutOfBounds as i32;
    };
    let Some(os_id) = d.mask.iter().nth(lp_index as usize) else {
        return GdtCpusErrorCode::OutOfBounds as i32;
    };
    unsafe { *out_os_id = os_id as u32 };
    GdtCpusErrorCode::Success as i32
}

/// Fills `out_cache` with the L1 data cache of the given core kind
/// (`size_bytes == 0` = not detected / kind not present).
///
/// # Safety
/// `out_cache` must point to a valid `GdtCpusCacheInfo`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn gdt_cpus_get_l1d_cache(
    kind: i32,
    out_cache: *mut GdtCpusCacheInfo,
) -> i32 {
    let c = get_info_validate_out_or_err!(out_cache);
    let Some(kind) = core_kind_from_i32(kind) else {
        return GdtCpusErrorCode::InvalidParameter as i32;
    };
    unsafe { *out_cache = (&c.info.l1d[kind.index()]).into() };
    GdtCpusErrorCode::Success as i32
}

/// Fills `out_cache` with the L1 instruction cache of the given core kind.
///
/// # Safety
/// `out_cache` must point to a valid `GdtCpusCacheInfo`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn gdt_cpus_get_l1i_cache(
    kind: i32,
    out_cache: *mut GdtCpusCacheInfo,
) -> i32 {
    let c = get_info_validate_out_or_err!(out_cache);
    let Some(kind) = core_kind_from_i32(kind) else {
        return GdtCpusErrorCode::InvalidParameter as i32;
    };
    unsafe { *out_cache = (&c.info.l1i[kind.index()]).into() };
    GdtCpusErrorCode::Success as i32
}

/// Fills `out_cache` with the L2 cache of the given core kind.
///
/// # Safety
/// `out_cache` must point to a valid `GdtCpusCacheInfo`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn gdt_cpus_get_l2_cache(kind: i32, out_cache: *mut GdtCpusCacheInfo) -> i32 {
    let c = get_info_validate_out_or_err!(out_cache);
    let Some(kind) = core_kind_from_i32(kind) else {
        return GdtCpusErrorCode::InvalidParameter as i32;
    };
    unsafe { *out_cache = (&c.info.l2[kind.index()]).into() };
    GdtCpusErrorCode::Success as i32
}

/// Writes the number of physical cores of the given kind.
///
/// # Safety
/// `out_count` must point to a valid `u64`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn gdt_cpus_num_cores_of_kind(kind: i32, out_count: *mut u64) -> i32 {
    let c = get_info_validate_out_or_err!(out_count);
    let Some(kind) = core_kind_from_i32(kind) else {
        return GdtCpusErrorCode::InvalidParameter as i32;
    };
    unsafe {
        *out_count = c.info.kind_core_counts[kind.index()] as u64;
    }
    GdtCpusErrorCode::Success as i32
}

// ---------------------------------------------------------------------------
// Thread control
// ---------------------------------------------------------------------------

fn mask_from_ffi(lp_ids: *const u32, count: u64) -> Result<AffinityMask, GdtCpusErrorCode> {
    if lp_ids.is_null() || count == 0 {
        return Err(GdtCpusErrorCode::InvalidParameter);
    }
    let ids = unsafe { std::slice::from_raw_parts(lp_ids, count as usize) };
    let mut mask = AffinityMask::empty();
    for &id in ids {
        // Reject out-of-range ids explicitly: AffinityMask caps at
        // MAX_LP_COUNT, so a bogus id is a clean InvalidParameter rather than a
        // silently dropped core (and never an allocation - the mask is fixed).
        if id as usize >= AffinityMask::MAX_LP_COUNT {
            return Err(GdtCpusErrorCode::InvalidParameter);
        }
        mask.add(id as usize);
    }
    Ok(mask)
}

/// Pins the current thread to a single logical processor (OS LP id).
///
/// macOS: returns `Unsupported` - Apple Silicon ignores affinity; use
/// `gdt_cpus_set_thread_priority` (QoS) for placement there.
#[unsafe(no_mangle)]
pub extern "C" fn gdt_cpus_pin_thread_to_core(logical_core_id: u64) -> i32 {
    match gdt_cpus::pin_thread_to_core(logical_core_id as usize) {
        Ok(_) => GdtCpusErrorCode::Success as i32,
        Err(e) => GdtCpusErrorCode::from(&e) as i32,
    }
}

/// Sets the current thread's HARD affinity to the given OS LP ids.
///
/// Windows: the set must stay within one 64-LP processor group (OS rule);
/// use soft affinity for cross-group placement. macOS: `Unsupported`.
///
/// # Safety
/// `lp_ids` must point to `count` valid `u32` values.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn gdt_cpus_set_thread_affinity(lp_ids: *const u32, count: u64) -> i32 {
    let mask = match mask_from_ffi(lp_ids, count) {
        Ok(m) => m,
        Err(code) => return code as i32,
    };
    match gdt_cpus::set_thread_affinity(&mask) {
        Ok(_) => GdtCpusErrorCode::Success as i32,
        Err(e) => GdtCpusErrorCode::from(&e) as i32,
    }
}

/// Sets the current thread's SOFT affinity (Windows CPU Sets) to the given
/// OS LP ids - the scheduler PREFERS these LPs but may migrate under
/// contention. Returns `Unsupported` on every other platform.
///
/// # Safety
/// `lp_ids` must point to `count` valid `u32` values.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn gdt_cpus_set_thread_soft_affinity(lp_ids: *const u32, count: u64) -> i32 {
    let mask = match mask_from_ffi(lp_ids, count) {
        Ok(m) => m,
        Err(code) => return code as i32,
    };
    match gdt_cpus::set_thread_soft_affinity(&mask) {
        Ok(_) => GdtCpusErrorCode::Success as i32,
        Err(e) => GdtCpusErrorCode::from(&e) as i32,
    }
}

/// Sets the current thread's priority, reporting what actually stuck.
///
/// `out_applied` is optional (may be NULL): pass it to learn whether the request
/// got a clean grant or silently fell back (see [`GdtCpusAppliedPriority`]). The
/// return code is `Success` whenever the OS accepted *something* - the fallback
/// detail lives in `out_applied->reason`, NOT the return code, so an audio thread
/// that needs to know it landed on `Normal` must read the out-param.
///
/// # Safety
/// `out_applied`, if non-NULL, must point to a valid `GdtCpusAppliedPriority`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn gdt_cpus_set_thread_priority(
    priority: i32,
    out_applied: *mut GdtCpusAppliedPriority,
) -> i32 {
    let Some(priority) = thread_priority_from_i32(priority) else {
        return GdtCpusErrorCode::InvalidParameter as i32;
    };
    match gdt_cpus::set_thread_priority(priority) {
        Ok(applied) => {
            if !out_applied.is_null() {
                unsafe { *out_applied = GdtCpusAppliedPriority::from(&applied) };
            }
            GdtCpusErrorCode::Success as i32
        }
        Err(e) => GdtCpusErrorCode::from(&e) as i32,
    }
}

/// Promotes the current thread to real-time with an explicit CPU budget - the
/// consent API. `out_applied` reports whether the request reached real-time or
/// kept the current timeshare priority with a structured fallback reason.
///
/// `budget_us` is the longest stretch (microseconds) the thread promises to
/// compute between blocking calls; Linux uses it for the `RLIMIT_RTTIME` leash
/// (ignored on Windows/macOS). `out_applied` is optional.
///
/// # Safety
/// `out_applied`, if non-NULL, must point to a valid `GdtCpusAppliedPriority`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn gdt_cpus_promote_thread_to_realtime(
    budget_us: u64,
    out_applied: *mut GdtCpusAppliedPriority,
) -> i32 {
    match gdt_cpus::promote_thread_to_realtime(std::time::Duration::from_micros(budget_us)) {
        Ok(applied) => {
            if !out_applied.is_null() {
                unsafe { *out_applied = GdtCpusAppliedPriority::from(&applied) };
            }
            GdtCpusErrorCode::Success as i32
        }
        Err(e) => GdtCpusErrorCode::from(&e) as i32,
    }
}

/// Demotes the current thread out of real-time, back to normal scheduling.
#[unsafe(no_mangle)]
pub extern "C" fn gdt_cpus_demote_thread_from_realtime() -> i32 {
    match gdt_cpus::demote_thread_from_realtime() {
        Ok(()) => GdtCpusErrorCode::Success as i32,
        Err(e) => GdtCpusErrorCode::from(&e) as i32,
    }
}

/// Fills `out_caps` with what each priority level will resolve to on this box.
/// Touches no thread state; a cheap startup pre-flight (see
/// [`GdtCpusPriorityCaps`]).
///
/// # Safety
/// `out_caps` must point to a valid `GdtCpusPriorityCaps`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn gdt_cpus_priority_capabilities(out_caps: *mut GdtCpusPriorityCaps) -> i32 {
    if out_caps.is_null() {
        return GdtCpusErrorCode::InvalidParameter as i32;
    }
    let caps = gdt_cpus::priority_capabilities();
    unsafe {
        *out_caps = GdtCpusPriorityCaps {
            effective_rank: caps.effective_rank,
            distinct_levels: caps.distinct_levels(),
        };
    }
    GdtCpusErrorCode::Success as i32
}

/// Returns a static, null-terminated name for a grant tier.
#[unsafe(no_mangle)]
pub extern "C" fn gdt_cpus_grant_description(grant: i32) -> *const c_char {
    let Some(grant) = grant_from_i32(grant) else {
        return std::ptr::null();
    };
    let s: &'static [u8] = match grant {
        GdtCpusGrant::Direct => b"direct\0",
        GdtCpusGrant::Brokered => b"brokered\0",
        GdtCpusGrant::Realtime => b"realtime\0",
    };
    s.as_ptr() as *const c_char
}

/// Returns a static, null-terminated description of a fallback reason.
#[unsafe(no_mangle)]
pub extern "C" fn gdt_cpus_fallback_reason_description(reason: i32) -> *const c_char {
    let Some(reason) = fallback_reason_from_i32(reason) else {
        return std::ptr::null();
    };
    let s: &'static [u8] = match reason {
        GdtCpusFallbackReason::None => b"clean grant\0",
        GdtCpusFallbackReason::NoBroker => b"no broker\0",
        GdtCpusFallbackReason::BrokerTimedOut => b"broker timed out\0",
        GdtCpusFallbackReason::BrokerRefused => b"broker refused\0",
        GdtCpusFallbackReason::Clamped => b"clamped to broker ceiling\0",
    };
    s.as_ptr() as *const c_char
}

/// Returns a static, null-terminated description of a broker-refusal reason.
#[unsafe(no_mangle)]
pub extern "C" fn gdt_cpus_broker_error_description(broker_error: i32) -> *const c_char {
    let Some(broker_error) = broker_error_from_i32(broker_error) else {
        return std::ptr::null();
    };
    let s: &'static [u8] = match broker_error {
        GdtCpusBrokerError::None => b"none\0",
        GdtCpusBrokerError::AccessDenied => b"access denied\0",
        GdtCpusBrokerError::LimitsExceeded => b"limits exceeded\0",
        GdtCpusBrokerError::InvalidArgs => b"invalid args\0",
        GdtCpusBrokerError::Failed => b"failed\0",
        GdtCpusBrokerError::Other => b"other\0",
    };
    s.as_ptr() as *const c_char
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ffi_rejects_null_out_pointers() {
        assert_eq!(
            unsafe { gdt_cpus_cpu_info(std::ptr::null_mut()) },
            GdtCpusErrorCode::InvalidParameter as i32
        );
        assert_eq!(
            unsafe { gdt_cpus_get_lp(0, std::ptr::null_mut()) },
            GdtCpusErrorCode::InvalidParameter as i32
        );
        assert_eq!(
            unsafe { gdt_cpus_priority_capabilities(std::ptr::null_mut()) },
            GdtCpusErrorCode::InvalidParameter as i32
        );
    }

    #[test]
    fn ffi_rejects_invalid_priority_value() {
        assert_eq!(
            unsafe { gdt_cpus_set_thread_priority(99, std::ptr::null_mut()) },
            GdtCpusErrorCode::InvalidParameter as i32
        );
    }

    #[test]
    fn ffi_cpu_info_smoke() {
        let mut info: GdtCpusCpuInfo = unsafe { std::mem::zeroed() };
        assert_eq!(
            unsafe { gdt_cpus_cpu_info(&mut info) },
            GdtCpusErrorCode::Success as i32
        );
        assert!(info.lp_count > 0);
        assert!(info.core_count > 0);
        assert!(!info.model_name.is_null());
    }

    #[test]
    fn applied_priority_conversion_preserves_structured_fields() {
        let applied = gdt_cpus::AppliedPriority::from_parts(
            gdt_cpus::ThreadPriority::TimeCritical,
            gdt_cpus::ThreadPriority::TimeCritical,
            gdt_cpus::Grant::Brokered,
            Some(gdt_cpus::FallbackReason::Clamped),
            gdt_cpus::Mechanism {
                policy: gdt_cpus::MechanismPolicy::Nice,
                value: -15,
            },
        );

        let ffi = GdtCpusAppliedPriority::from(&applied);
        assert_eq!(
            ffi.requested as i32,
            GdtCpusThreadPriority::TimeCritical as i32
        );
        assert_eq!(
            ffi.effective as i32,
            GdtCpusThreadPriority::TimeCritical as i32
        );
        assert_eq!(ffi.grant as i32, GdtCpusGrant::Brokered as i32);
        assert_eq!(ffi.reason as i32, GdtCpusFallbackReason::Clamped as i32);
        assert_eq!(ffi.broker_error as i32, GdtCpusBrokerError::None as i32);
        assert_eq!(
            ffi.mechanism.policy as i32,
            GdtCpusMechanismPolicy::Nice as i32
        );
        assert_eq!(ffi.mechanism.value, -15);
    }
}
