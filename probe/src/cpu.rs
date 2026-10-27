//! Groups A and B: CPU identification registers and cache geometry.

use crate::report::{Report, say};

/// Extract the 4-bit ID-register field that starts at bit `lsb`.
///
/// Shifting right by `lsb` moves the field down to bits 3..0, and `& 0xf`
/// (binary 1111) clears everything above them, leaving just the field.
pub fn field(reg: u64, lsb: u32) -> u64 {
    (reg >> lsb) & 0xf
}

/// Raw values of the registers this module decodes.
#[derive(Clone, Copy, Default)]
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
}

impl Ids {
    pub fn read() -> Self {
        Ids {
            midr: read_sysreg!("MIDR_EL1"),
            mpidr: read_sysreg!("MPIDR_EL1"),
            pfr0: read_sysreg!("ID_AA64PFR0_EL1"),
            pfr1: read_sysreg!("ID_AA64PFR1_EL1"),
            isar0: read_sysreg!("ID_AA64ISAR0_EL1"),
            isar1: read_sysreg!("ID_AA64ISAR1_EL1"),
            isar2: read_sysreg!("ID_AA64ISAR2_EL1"),
            mmfr0: read_sysreg!("ID_AA64MMFR0_EL1"),
            mmfr1: read_sysreg!("ID_AA64MMFR1_EL1"),
            mmfr2: read_sysreg!("ID_AA64MMFR2_EL1"),
            dfr0: read_sysreg!("ID_AA64DFR0_EL1"),
            // ID_AA64ZFR0_EL1 by its encoding: the assembler only knows the
            // name when SVE is enabled for the whole program.
            zfr0: read_sysreg!("S3_0_C0_C4_4"),
            clidr: read_sysreg!("CLIDR_EL1"),
        }
    }
}

/// Core name from MIDR_EL1: implementer in bits 31..24, part number in 15..4.
pub fn core_name(midr: u64) -> &'static str {
    match ((midr >> 24) & 0xff, (midr >> 4) & 0xfff) {
        (0x41, 0xd80) => "Cortex-A520",
        (0x41, 0xd81) => "Cortex-A720",
        (0x00, 0x051) => "QEMU 'max' (emulated)",
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

/// Read the size of one cache. CSSELR_EL1 selects which cache CCSIDR_EL1
/// describes: bits 3..1 are the level minus one, bit 0 picks the
/// instruction cache. CSSELR only affects this CPU and is restored afterwards.
fn ccsidr(level: u64, instruction: bool) -> u64 {
    let select = ((level - 1) << 1) | instruction as u64;
    let old = read_sysreg!("CSSELR_EL1");
    let value: u64;
    // SAFETY: selecting a cache to describe has no effect beyond which value
    // CCSIDR_EL1 shows; the old selection is put back.
    unsafe {
        core::arch::asm!(
            "msr CSSELR_EL1, {sel}",
            "isb",
            "mrs {val}, CCSIDR_EL1",
            "msr CSSELR_EL1, {old}",
            "isb",
            sel = in(reg) select,
            old = in(reg) old,
            val = out(reg) value,
        )
    };
    value
}

pub fn report_caches(r: &mut Report, ids: &Ids) {
    say!(r, "CLIDR_EL1 = {:#018x}", ids.clidr);
    // FEAT_CCIDX (MMFR2 bits 23..20) widens the CCSIDR fields.
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
            let c = ccsidr(level, instruction);
            let line = 16u64 << (c & 7); // bits 2..0: log2(bytes) - 4
            let (ways, sets) = if ccidx {
                (((c >> 3) & 0x1f_ffff) + 1, ((c >> 32) & 0xff_ffff) + 1)
            } else {
                (((c >> 3) & 0x3ff) + 1, ((c >> 13) & 0x7fff) + 1)
            };
            let kib = line * ways * sets / 1024;
            say!(r, "  L{level} {name:<11} {kib:>6} KiB  {ways:>2}-way  {sets:>5} sets  {line} B lines  (CCSIDR={c:#x})");
        }
    }
}
