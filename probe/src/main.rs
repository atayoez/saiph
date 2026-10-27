#![no_main]
#![no_std]

extern crate alloc;

use alloc::string::String;
use core::time::Duration;
use uefi::fs::FileSystem;
use uefi::prelude::*;
use uefi::{cstr16, println};

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

/// Print a line on the console and keep a copy in `$report` for report.txt.
macro_rules! say {
    ($report:expr, $($arg:tt)*) => {{
        let line = alloc::format!($($arg)*);
        println!("{line}");
        $report.push_str(&line);
        $report.push('\n');
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

/// Write `text` to \EFI\orion-probe\report.txt on the volume this app was
/// loaded from, replacing the report of any earlier run.
fn save_report(text: &str) -> Result<(), String> {
    let volume = boot::get_image_file_system(boot::image_handle())
        .map_err(|e| alloc::format!("no file system on the boot volume ({:?})", e.status()))?;
    let mut fs = FileSystem::new(volume);
    fs.create_dir_all(cstr16!("\\EFI\\orion-probe")).map_err(|e| alloc::format!("{e:?}"))?;
    fs.write(cstr16!("\\EFI\\orion-probe\\report.txt"), text.as_bytes()).map_err(|e| alloc::format!("{e:?}"))?;
    Ok(())
}

#[entry]
fn main() -> Status {
    let mut report = String::new();
    say!(report, "Hello from the Orion probe");
    // CurrentEL keeps the level in bits 3..2, so shift by 2 and keep 2 bits.
    say!(report, "Running at EL{}", (read_sysreg!("CurrentEL") >> 2) & 3);

    // ID_AA64PFR1_EL1 ("Processor Feature Register 1"): BT is bits 3..0 and
    // MTE is bits 11..8 (see the register's page in the Arm Architecture
    // Reference Manual).
    let pfr1 = read_sysreg!("ID_AA64PFR1_EL1");
    say!(report, "ID_AA64PFR1_EL1 = {pfr1:#018x}");
    let bt = field(pfr1, 0);
    say!(report, "  BTI: {} (BT={bt})", if bt >= 1 { "present" } else { "absent" });
    let mte = field(pfr1, 8);
    let mte_desc = match mte {
        0 => "absent",
        1 => "instructions only, no tag checking (MTE)",
        2 => "full tag checking (MTE2)",
        3 => "full tag checking plus asymmetric mode (MTE3)",
        _ => "newer than MTE3",
    };
    say!(report, "  MTE: {mte_desc} (MTE={mte})");

    // Pointer authentication. A CPU implements one signing algorithm for
    // addresses: QARMA5 (ISAR1.APA, bits 7..4), a vendor-defined one
    // (ISAR1.API, bits 11..8) or QARMA3 (ISAR2.APA3, bits 15..12). The
    // "generic" fields (GPA bits 27..24, GPI bits 31..28, GPA3 bits 11..8)
    // say whether the PACGA instruction, which signs arbitrary data, exists.
    let isar1 = read_sysreg!("ID_AA64ISAR1_EL1");
    let isar2 = read_sysreg!("ID_AA64ISAR2_EL1");
    say!(report, "ID_AA64ISAR1_EL1 = {isar1:#018x}");
    say!(report, "ID_AA64ISAR2_EL1 = {isar2:#018x}");
    let (apa, api, apa3) = (field(isar1, 4), field(isar1, 8), field(isar2, 12));
    say!(report, "  PAuth, QARMA5 algorithm (APA):  {} (APA={apa})", pauth_level(apa));
    say!(report, "  PAuth, vendor algorithm (API):  {} (API={api})", pauth_level(api));
    say!(report, "  PAuth, QARMA3 algorithm (APA3): {} (APA3={apa3})", pauth_level(apa3));
    let (gpa, gpi, gpa3) = (field(isar1, 24), field(isar1, 28), field(isar2, 8));
    let generic = match (gpa, gpi, gpa3) {
        (0, 0, 0) => "not implemented",
        (_, _, 1..) => "QARMA3",
        (1.., _, _) => "QARMA5",
        _ => "vendor algorithm",
    };
    say!(report, "  Generic signing (PACGA):        {generic} (GPA={gpa} GPI={gpi} GPA3={gpa3})");

    match save_report(&report) {
        Ok(()) => println!("Report saved to \\EFI\\orion-probe\\report.txt"),
        Err(e) => println!("Could not save the report: {e}"),
    }

    // scripts/run-qemu.sh stops QEMU when it sees this line.
    println!("Probe finished.");

    // Leave the text on screen long enough to read it on the real board.
    boot::stall(Duration::from_secs(10));
    Status::SUCCESS
}
