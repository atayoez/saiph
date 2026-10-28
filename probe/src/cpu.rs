//! Groups A and B: CPU identification registers and cache geometry.

use crate::report::{Report, say};

/// Extract the 4-bit ID-register field that starts at bit `lsb`.
///
/// Shifting right by `lsb` moves the field down to bits 3..0, and `& 0xf`
/// (binary 1111) clears everything above them, leaving just the field.
pub fn field(reg: u64, lsb: u32) -> u64 {
    (reg >> lsb) & 0xf
}

/// Raw values of the registers this module decodes. The layout is shared
/// with the assembly routine below: 13 registers, then CCSIDR for cache
/// levels 1..7 as [data or unified, instruction].
#[repr(C)]
#[derive(Clone, Copy, Default, PartialEq)]
pub struct Ids {
    pub midr: u64,
    pub mpidr: u64,
    pub pfr0: u64,
    pub pfr1: u64,
    pub isar0: u64,
    pub isar1: u64,
    pub isar2: u64,
    pub mmfr0: u64,
    pub mmfr1: u64,
    pub mmfr2: u64,
    pub dfr0: u64,
    pub zfr0: u64,
    pub clidr: u64,
    pub ccsidr: [[u64; 2]; 7],
}

// Reads every register in `Ids` on the core it runs on. It uses no stack,
// no floating point and no firmware calls, so it can also run on the other
// cores (group G). CSSELR_EL1 selects which cache CCSIDR_EL1 describes:
// bits 3..1 are the level minus one and bit 0 picks the instruction cache.
// CSSELR only affects this core and is put back at the end; interrupts are
// masked meanwhile.
core::arch::global_asm!(
    ".global orion_probe_read_ids",
    ".p2align 2",
    "orion_probe_read_ids:",
    "mrs x1, MIDR_EL1",         "str x1, [x0, #0]",
    "mrs x1, MPIDR_EL1",        "str x1, [x0, #8]",
    "mrs x1, ID_AA64PFR0_EL1",  "str x1, [x0, #16]",
    "mrs x1, ID_AA64PFR1_EL1",  "str x1, [x0, #24]",
    "mrs x1, ID_AA64ISAR0_EL1", "str x1, [x0, #32]",
    "mrs x1, ID_AA64ISAR1_EL1", "str x1, [x0, #40]",
    "mrs x1, ID_AA64ISAR2_EL1", "str x1, [x0, #48]",
    "mrs x1, ID_AA64MMFR0_EL1", "str x1, [x0, #56]",
    "mrs x1, ID_AA64MMFR1_EL1", "str x1, [x0, #64]",
    "mrs x1, ID_AA64MMFR2_EL1", "str x1, [x0, #72]",
    "mrs x1, ID_AA64DFR0_EL1",  "str x1, [x0, #80]",
    // ID_AA64ZFR0_EL1 by its encoding: the assembler only knows the name
    // when SVE is enabled for the whole program.
    "mrs x1, S3_0_C0_C4_4",     "str x1, [x0, #88]",
    "mrs x1, CLIDR_EL1",        "str x1, [x0, #96]",
    "mrs x5, DAIF",
    "msr DAIFSet, #3",
    "mrs x4, CSSELR_EL1",
    "add x2, x0, #104",
    "mov x3, #0",
    "1:",
    "msr CSSELR_EL1, x3", "isb", "mrs x1, CCSIDR_EL1", "str x1, [x2], #8",
    "orr x6, x3, #1",
    "msr CSSELR_EL1, x6", "isb", "mrs x1, CCSIDR_EL1", "str x1, [x2], #8",
    "add x3, x3, #2",
    "cmp x3, #14",
    "b.lo 1b",
    "msr CSSELR_EL1, x4",
    "isb",
    "msr DAIF, x5",
    "dsb sy",
    "ret",
);

unsafe extern "efiapi" {
    pub fn orion_probe_read_ids(ids: *mut Ids);
}

impl Ids {
    /// Read the registers of the core this runs on.
    pub fn read() -> Self {
        let mut ids = Ids::default();
        // SAFETY: the routine only reads ID registers (and restores the
        // cache selector) and writes into `ids`, whose layout it matches.
        unsafe { orion_probe_read_ids(&mut ids) };
        ids.tidy();
        ids
    }

    /// Clear CCSIDR values of caches that CLIDR says do not exist: for a
    /// missing cache the CPU returns an unspecified value.
    pub fn tidy(&mut self) {
        for level in 0..7 {
            let kind = (self.clidr >> (3 * level)) & 7;
            if !(2..=4).contains(&kind) {
                self.ccsidr[level][0] = 0;
            }
            if kind != 1 && kind != 3 {
                self.ccsidr[level][1] = 0;
            }
        }
    }
}

/// Core name from MIDR_EL1: implementer in bits 31..24, part number in 15..4.
pub fn core_name(midr: u64) -> &'static str {
    match ((midr >> 24) & 0xff, (midr >> 4) & 0xfff) {
        (0x41, 0xd80) => "Cortex-A520",
        (0x41, 0xd81) => "Cortex-A720",
        (0x00, 0x051) => "QEMU max",
        _ => "unknown core",
    }
}

fn pauth_level(v: u64) -> &'static str {
    match v {
        0 => "not implemented",
        1 => "basic PAuth",
        2 => "PAuth with EPAC",
        3 => "PAuth2",
        4 => "PAuth2 + FPAC",
        5 => "PAuth2 + FPAC + FPACCOMBINE",
        _ => "newer than FPACCOMBINE",
    }
}

fn yes(v: u64) -> &'static str {
    if v >= 1 { "present" } else { "absent" }
}

pub fn report_ids(r: &mut Report, ids: &Ids) {
    let m = ids.midr;
    say!(r, "MIDR_EL1  = {m:#018x}  {} r{}p{}", core_name(m), (m >> 20) & 0xf, m & 0xf);
    let a = ids.mpidr;
    say!(r, "MPIDR_EL1 = {a:#018x}  core address {}.{}.{}.{}", (a >> 32) & 0xff, (a >> 16) & 0xff, (a >> 8) & 0xff, a & 0xff);
    for (name, v) in [
        ("ID_AA64PFR0_EL1 ", ids.pfr0),
        ("ID_AA64PFR1_EL1 ", ids.pfr1),
        ("ID_AA64ISAR0_EL1", ids.isar0),
        ("ID_AA64ISAR1_EL1", ids.isar1),
        ("ID_AA64ISAR2_EL1", ids.isar2),
        ("ID_AA64MMFR0_EL1", ids.mmfr0),
        ("ID_AA64MMFR1_EL1", ids.mmfr1),
        ("ID_AA64MMFR2_EL1", ids.mmfr2),
        ("ID_AA64DFR0_EL1 ", ids.dfr0),
        ("ID_AA64ZFR0_EL1 ", ids.zfr0),
    ] {
        say!(r, "{name} = {v:#018x}");
    }

    // Security features.
    let bt = field(ids.pfr1, 0); // PFR1 bits 3..0
    say!(r, "  BTI:                 {} (BT={bt})", yes(bt));
    let mte = field(ids.pfr1, 8); // PFR1 bits 11..8
    let mte_desc = match mte {
        0 => "absent",
        1 => "instructions only, no tag checking (MTE)",
        2 => "full tag checking (MTE2)",
        3 => "full tag checking plus asymmetric mode (MTE3)",
        _ => "newer than MTE3",
    };
    say!(r, "  MTE:                 {mte_desc} (MTE={mte})");
    // One signing algorithm per CPU: QARMA5 (ISAR1 bits 7..4), vendor (ISAR1
    // bits 11..8) or QARMA3 (ISAR2 bits 15..12).
    let (apa, api, apa3) = (field(ids.isar1, 4), field(ids.isar1, 8), field(ids.isar2, 12));
    let pauth = match (apa, api, apa3) {
        (1.., _, _) => alloc::format!("QARMA5, {}", pauth_level(apa)),
        (_, 1.., _) => alloc::format!("vendor algorithm, {}", pauth_level(api)),
        (_, _, 1..) => alloc::format!("QARMA3, {}", pauth_level(apa3)),
        _ => alloc::format!("absent"),
    };
    say!(r, "  PAuth:               {pauth} (APA={apa} API={api} APA3={apa3})");
    let gcs = field(ids.pfr1, 44); // PFR1 bits 47..44
    say!(r, "  GCS (shadow stack):  {} (GCS={gcs})", yes(gcs));

    // Memory addressing (MMFR0).
    let pa_code = field(ids.mmfr0, 0); // bits 3..0
    let pa_bits = [32, 36, 40, 42, 44, 48, 52, 56].get(pa_code as usize).copied().unwrap_or(0);
    say!(r, "  Physical addresses:  {pa_bits} bits (PARange={pa_code})");
    // TGran4 (bits 31..28): 0 = yes, 1 = yes with 52-bit (LPA2), 0xf = no.
    // TGran16 (bits 23..20): 0 = no, 1 = yes, 2 = yes with 52-bit.
    // TGran64 (bits 27..24): 0 = yes, 0xf = no.
    let (tg4, tg16, tg64) = (field(ids.mmfr0, 28), field(ids.mmfr0, 20), field(ids.mmfr0, 24));
    let ok = |b: bool| if b { "yes" } else { "no" };
    say!(r, "  Page sizes:          4K={} 16K={} 64K={} (TGran4={tg4} TGran16={tg16} TGran64={tg64})", ok(tg4 != 0xf), ok(tg16 != 0), ok(tg64 != 0xf));
    say!(r, "  LPA2 (52-bit, 4K):   {}", if tg4 == 1 || tg16 == 2 { "present" } else { "absent" });

    // Virtualization (MMFR1, MMFR2) and exception levels (PFR0).
    let vh = field(ids.mmfr1, 8); // MMFR1 bits 11..8
    say!(r, "  VHE:                 {} (VH={vh})", yes(vh));
    let nv = field(ids.mmfr2, 24); // MMFR2 bits 27..24
    let nv_desc = match nv {
        0 => "absent",
        1 => "NV",
        2 => "NV2",
        _ => "newer than NV2",
    };
    say!(r, "  Nested virt:         {nv_desc} (NV={nv})");
    let (el2, el3) = (field(ids.pfr0, 8), field(ids.pfr0, 12)); // PFR0 bits 11..8, 15..12
    say!(r, "  EL2 / EL3 exist:     {} / {}", yes(el2), yes(el3));

    // Reliability and resource partitioning: major version in PFR0, minor in PFR1.
    let (ras, ras_frac) = (field(ids.pfr0, 28), field(ids.pfr1, 12)); // PFR0 31..28, PFR1 15..12
    let ras_desc = match (ras, ras_frac) {
        (0, _) => "absent",
        (1, 0) => "RAS v1",
        (1, _) | (2, _) => "RAS v1.1",
        (3, _) => "RAS v2",
        _ => "newer than RAS v2",
    };
    say!(r, "  RAS:                 {ras_desc} (RAS={ras} RAS_frac={ras_frac})");
    let (mpam, mpam_frac) = (field(ids.pfr0, 40), field(ids.pfr1, 16)); // PFR0 43..40, PFR1 19..16
    if mpam == 0 && mpam_frac == 0 {
        say!(r, "  MPAM:                absent");
    } else {
        say!(r, "  MPAM:                v{mpam}.{mpam_frac} (MPAM={mpam} MPAM_frac={mpam_frac})");
    }

    // Vector and other instruction-set features.
    let sve = field(ids.pfr0, 32); // PFR0 bits 35..32
    let sve_desc = match (sve, field(ids.zfr0, 0)) {
        (0, _) => "absent",
        (_, 0) => "SVE",
        (_, 1) => "SVE2",
        (_, 2) => "SVE2.1",
        _ => "newer than SVE2.1",
    };
    say!(r, "  SVE:                 {sve_desc}");
    let sme = field(ids.pfr1, 24); // PFR1 bits 27..24
    say!(r, "  SME:                 {} (SME={sme})", yes(sme));
    let rndr = field(ids.isar0, 60); // ISAR0 bits 63..60
    say!(r, "  RNDR (random insn):  {} (RNDR={rndr})", yes(rndr));
    let atomic = field(ids.isar0, 20); // ISAR0 bits 23..20
    say!(r, "  LSE atomics:         {} (Atomic={atomic})", if atomic >= 2 { "present" } else { "absent" });
    let gic = field(ids.pfr0, 24); // PFR0 bits 27..24
    let gic_desc = match gic {
        0 => "absent",
        1 => "GICv3 / GICv4.0",
        3 => "GICv4.1",
        _ => "unknown",
    };
    say!(r, "  GIC CPU interface:   {gic_desc} (GIC={gic})");

    // Debug and profiling (DFR0).
    let pmu = field(ids.dfr0, 8); // bits 11..8
    say!(r, "  PMU version code:    {pmu} (1=PMUv3, 4..9 = PMUv3.1..v3.9)");
    let spe = field(ids.dfr0, 32); // bits 35..32
    say!(r, "  SPE (profiling):     {} (PMSVer={spe})", yes(spe));
    let trbe = field(ids.dfr0, 44); // bits 47..44
    say!(r, "  TRBE (trace buffer): {} (TraceBuffer={trbe})", yes(trbe));
}

/// (bytes, ways, sets, line bytes) of one cache from its CCSIDR value.
/// FEAT_CCIDX (MMFR2 bits 23..20) widens the ways and sets fields.
pub fn cache_size(c: u64, ccidx: bool) -> (u64, u64, u64, u64) {
    let line = 16u64 << (c & 7); // bits 2..0: log2(bytes) - 4
    let (ways, sets) = if ccidx {
        (((c >> 3) & 0x1f_ffff) + 1, ((c >> 32) & 0xff_ffff) + 1)
    } else {
        (((c >> 3) & 0x3ff) + 1, ((c >> 13) & 0x7fff) + 1)
    };
    (line * ways * sets, ways, sets, line)
}

/// "64K", "12M" or "64B".
pub fn size(bytes: u64) -> alloc::string::String {
    match bytes {
        b if b >= 1 << 20 && b % (1 << 20) == 0 => alloc::format!("{}M", b >> 20),
        b if b >= 1 << 10 => alloc::format!("{}K", b >> 10),
        b => alloc::format!("{b}B"),
    }
}

/// One short line per core, e.g. "L1 64K/64K, L2 512K, L3 12M".
pub fn cache_summary(ids: &Ids) -> alloc::string::String {
    let ccidx = field(ids.mmfr2, 20) != 0;
    let mut parts = alloc::vec::Vec::new();
    for level in 0..7 {
        let kind = (ids.clidr >> (3 * level)) & 7;
        if kind == 0 {
            break;
        }
        let d = size(cache_size(ids.ccsidr[level][0], ccidx).0);
        let i = size(cache_size(ids.ccsidr[level][1], ccidx).0);
        parts.push(match kind {
            1 => alloc::format!("L{} {i}(I)", level + 1),
            3 => alloc::format!("L{} {d}/{i}", level + 1),
            _ => alloc::format!("L{} {d}", level + 1),
        });
    }
    parts.join(", ")
}

pub fn report_caches(r: &mut Report, ids: &Ids) {
    say!(r, "CLIDR_EL1 = {:#018x}", ids.clidr);
    let ccidx = field(ids.mmfr2, 20) != 0;
    for level in 1..=7u64 {
        // CLIDR holds a 3-bit type per level: 1 = instruction only,
        // 2 = data only, 3 = separate instruction and data, 4 = unified.
        let kind = (ids.clidr >> (3 * (level - 1))) & 7;
        let caches: &[(&str, bool)] = match kind {
            0 => break,
            1 => &[("instruction", true)],
            2 => &[("data", false)],
            3 => &[("data", false), ("instruction", true)],
            4 => &[("unified", false)],
            _ => &[],
        };
        for &(name, instruction) in caches {
            let c = ids.ccsidr[level as usize - 1][instruction as usize];
            let (bytes, ways, sets, line) = cache_size(c, ccidx);
            say!(r, "  L{level} {name:<11} {:>6}  {ways:>2}-way  {sets:>5} sets  {line} B lines  (CCSIDR={c:#x})", size(bytes));
        }
    }
}

/// Names of the sixteen 4-bit fields (bits 3..0 first) of the ID registers
/// in `Ids`, from the Arm Architecture Reference Manual. "-" = reserved.
pub fn field_names(register: &str) -> [&'static str; 16] {
    match register {
        "PFR0" => ["EL0", "EL1", "EL2", "EL3", "FP", "AdvSIMD", "GIC", "RAS", "SVE", "SEL2", "MPAM", "AMU", "DIT", "RME", "CSV2", "CSV3"],
        "PFR1" => ["BT", "SSBS", "MTE", "RAS_frac", "MPAM_frac", "-", "SME", "RNDR_trap", "CSV2_frac", "NMI", "MTE_frac", "GCS", "THE", "MTEX", "DF2", "PFAR"],
        "ISAR0" => ["-", "AES", "SHA1", "SHA2", "CRC32", "Atomic", "TME", "RDM", "SHA3", "SM3", "SM4", "DP", "FHM", "TS", "TLB", "RNDR"],
        "ISAR1" => ["DPB", "APA", "API", "JSCVT", "FCMA", "LRCPC", "GPA", "GPI", "FRINTTS", "SB", "SPECRES", "BF16", "DGH", "I8MM", "XS", "LS64"],
        "ISAR2" => ["WFxT", "RPRES", "GPA3", "APA3", "MOPS", "BC", "PAC_frac", "CLRBHB", "SYSREG_128", "SYSINSTR_128", "PRFMSLC", "-", "RPRFM", "CSSC", "LUT", "ATS1A"],
        "MMFR0" => ["PARange", "ASIDBits", "BigEnd", "SNSMem", "BigEndEL0", "TGran16", "TGran64", "TGran4", "TGran16_2", "TGran64_2", "TGran4_2", "ExS", "-", "-", "FGT", "ECV"],
        "MMFR1" => ["HAFDBS", "VMIDBits", "VH", "HPDS", "LO", "PAN", "SpecSEI", "XNX", "TWED", "ETS", "HCX", "AFP", "nTLBPA", "TIDCP1", "CMOW", "ECBHB"],
        "MMFR2" => ["CnP", "UAO", "LSM", "IESB", "VARange", "CCIDX", "NV", "ST", "AT", "IDS", "FWB", "-", "TTL", "BBM", "EVT", "E0PD"],
        "DFR0" => ["DebugVer", "TraceVer", "PMUVer", "BRPs", "PMSS", "WRPs", "SEBEP", "CTX_CMPs", "PMSVer", "DoubleLock", "TraceFilt", "TraceBuffer", "MTPMU", "BRBE", "ExtTrcBuff", "HPMN0"],
        "ZFR0" => ["SVEver", "AES", "-", "-", "BitPerm", "BF16", "B16B16", "-", "SHA3", "-", "SM4", "I8MM", "-", "F32MM", "F64MM", "-"],
        _ => ["?"; 16],
    }
}

/// One-line meaning of a field, for the ones that are likely to differ
/// between the O6's two core types (memory-model and debug features).
pub fn field_meaning(name: &str) -> &'static str {
    match name {
        "HAFDBS" => "hardware updates of the page-table Access flag and Dirty state",
        "VMIDBits" => "size of VM IDs in the TLB (0 = 8 bits, 2 = 16 bits)",
        "VH" => "VHE: the kernel can run at EL2",
        "HPDS" => "hierarchical permission disables in page tables",
        "LO" => "LORegions (limited-ordering memory regions)",
        "PAN" => "Privileged Access Never (kernel cannot touch user memory by accident)",
        "SpecSEI" => "SError on speculative reads",
        "XNX" => "separate execute-never for EL0 and EL1 at stage 2",
        "TWED" => "configurable delay before trapping WFE",
        "ETS" => "enhanced translation synchronization",
        "HCX" => "the HCRX_EL2 register exists",
        "AFP" => "alternate floating-point behaviour controls",
        "nTLBPA" => "how the TLB caches intermediate page-table entries",
        "TIDCP1" => "traps of IMPLEMENTATION DEFINED registers at EL0",
        "CMOW" => "cache maintenance needs write permission",
        "ECBHB" => "hardware mitigation of branch-history (Spectre-BHB) attacks",
        "DebugVer" => "debug architecture version",
        "TraceVer" => "trace unit with a system-register interface",
        "PMUVer" => "performance monitor (PMU) version",
        "BRPs" => "number of hardware breakpoints, minus 1",
        "PMSS" => "PMU snapshot extension",
        "WRPs" => "number of hardware watchpoints, minus 1",
        "SEBEP" => "synchronous-exception-based event profiling",
        "CTX_CMPs" => "number of context-aware breakpoints, minus 1",
        "PMSVer" => "Statistical Profiling Extension (SPE) version",
        "DoubleLock" => "OS double-lock (0xf = not implemented)",
        "TraceFilt" => "trace filtering (TRF)",
        "TraceBuffer" => "trace buffer extension (TRBE)",
        "MTPMU" => "multi-threaded PMU events",
        "BRBE" => "branch record buffer (BRBE)",
        "ExtTrcBuff" => "external trace buffer access",
        "HPMN0" => "all PMU counters can be reserved for EL2",
        _ => "see the register's page in the Arm ARM",
    }
}
