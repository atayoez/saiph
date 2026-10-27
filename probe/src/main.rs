#![no_main]
#![no_std]

use core::time::Duration;
use uefi::prelude::*;
use uefi::println;

/// Read a system register with `mrs`.
///
/// This is a macro, not a function, because the register name is part of
/// the instruction text, which the assembler needs at compile time.
macro_rules! read_sysreg {
    ($name:literal) => {{
        let value: u64;
        // SAFETY: the registers read here are read-only ID and status
        // registers; reading them has no side effects.
        unsafe { core::arch::asm!(concat!("mrs {}, ", $name), out(reg) value) };
        value
    }};
}

/// Extract the 4-bit ID-register field that starts at bit `lsb`.
///
/// Shifting right by `lsb` moves the field down to bits 3..0, and `& 0xf`
/// (binary 1111) clears everything above them, leaving just the field.
fn field(reg: u64, lsb: u32) -> u64 {
    (reg >> lsb) & 0xf
}

/// Meaning of an address-authentication field (APA, API or APA3).
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

#[entry]
fn main() -> Status {
    println!("Hello from the Orion probe");
    // CurrentEL keeps the level in bits 3..2, so shift by 2 and keep 2 bits.
    println!("Running at EL{}", (read_sysreg!("CurrentEL") >> 2) & 3);

    // ID_AA64PFR1_EL1 ("Processor Feature Register 1"): BT is bits 3..0 and
    // MTE is bits 11..8 (see the register's page in the Arm Architecture
    // Reference Manual).
    let pfr1 = read_sysreg!("ID_AA64PFR1_EL1");
    println!("ID_AA64PFR1_EL1 = {pfr1:#018x}");
    let bt = field(pfr1, 0);
    println!("  BTI: {} (BT={bt})", if bt >= 1 { "present" } else { "absent" });
    let mte = field(pfr1, 8);
    let mte_desc = match mte {
        0 => "absent",
        1 => "instructions only, no tag checking (MTE)",
        2 => "full tag checking (MTE2)",
        3 => "full tag checking plus asymmetric mode (MTE3)",
        _ => "newer than MTE3",
    };
    println!("  MTE: {mte_desc} (MTE={mte})");

    // Pointer authentication. A CPU implements one signing algorithm for
    // addresses: QARMA5 (ISAR1.APA, bits 7..4), a vendor-defined one
    // (ISAR1.API, bits 11..8) or QARMA3 (ISAR2.APA3, bits 15..12). The
    // "generic" fields (GPA bits 27..24, GPI bits 31..28, GPA3 bits 11..8)
    // say whether the PACGA instruction, which signs arbitrary data, exists.
    let isar1 = read_sysreg!("ID_AA64ISAR1_EL1");
    let isar2 = read_sysreg!("ID_AA64ISAR2_EL1");
    println!("ID_AA64ISAR1_EL1 = {isar1:#018x}");
    println!("ID_AA64ISAR2_EL1 = {isar2:#018x}");
    let (apa, api, apa3) = (field(isar1, 4), field(isar1, 8), field(isar2, 12));
    println!("  PAuth, QARMA5 algorithm (APA):  {} (APA={apa})", pauth_level(apa));
    println!("  PAuth, vendor algorithm (API):  {} (API={api})", pauth_level(api));
    println!("  PAuth, QARMA3 algorithm (APA3): {} (APA3={apa3})", pauth_level(apa3));
    let (gpa, gpi, gpa3) = (field(isar1, 24), field(isar1, 28), field(isar2, 8));
    let generic = match (gpa, gpi, gpa3) {
        (0, 0, 0) => "not implemented",
        (_, _, 1..) => "QARMA3",
        (1.., _, _) => "QARMA5",
        _ => "vendor algorithm",
    };
    println!("  Generic signing (PACGA):        {generic} (GPA={gpa} GPI={gpi} GPA3={gpa3})");

    // scripts/run-qemu.sh stops QEMU when it sees this line.
    println!("Probe finished.");

    // Leave the text on screen long enough to read it on the real board.
    boot::stall(Duration::from_secs(10));
    Status::SUCCESS
}
