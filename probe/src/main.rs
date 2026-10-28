#![no_main]
#![no_std]

extern crate alloc;

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

mod acpi;
mod cpu;
mod firmware;
mod gic;
mod mmio;
mod mte;
mod percore;
mod report;
mod smmu;

use core::time::Duration;
use report::{Report, say};
use uefi::prelude::*;
use uefi::println;

#[entry]
fn main() -> Status {
    let mut r = Report::new();
    say!(r, "Orion probe");
    if let Some(e) = r.file_error.clone() {
        say!(r, "(no report file: {e})");
    }
    // CurrentEL keeps the level in bits 3..2, so shift by 2 and keep 2 bits.
    say!(r, "Running at EL{}", (read_sysreg!("CurrentEL") >> 2) & 3);
    let ids = cpu::Ids::read();

    r.section("A. CPU features (boot core)");
    cpu::report_ids(&mut r, &ids);
    r.flush();

    r.section("B. Caches (boot core)");
    cpu::report_caches(&mut r, &ids);
    r.flush();

    r.section("C. ACPI tables");
    let tables = acpi::tables(&mut r);
    acpi::report_tables(&mut r, &tables);
    let madt = acpi::madt(&tables);
    if let Some(m) = &madt {
        acpi::report_madt(&mut r, m);
    }
    let uart = acpi::dbg2_uart(&tables);
    if let Some((subtype, base)) = uart {
        let kind = match subtype {
            0x0003 => "Arm PL011",
            0x000e => "Arm SBSA generic UART",
            _ => "other serial port",
        };
        say!(r, "Debug UART (DBG2): {kind} at {base:#x}");
    }
    let smmus = acpi::iort_smmus(&tables);
    if smmus.is_empty() {
        say!(r, "SMMUv3 units (IORT): none");
    }
    for base in &smmus {
        say!(r, "SMMUv3 unit (IORT) at {base:#x}");
    }
    r.flush();

    r.section("D. Interrupt controller and SMMU");
    if let Some(m) = &madt {
        gic::report(&mut r, m);
    }
    for &base in &smmus {
        smmu::report(&mut r, base);
    }
    r.flush();

    r.section("E. Entropy");
    let rndr = cpu::field(ids.isar0, 60);
    say!(r, "RNDR instruction: {}", if rndr >= 1 { "present" } else { "absent" });
    firmware::report_efi_rng(&mut r);
    firmware::report_smccc(&mut r, &ids, &tables);
    r.flush();

    r.section("F. Firmware and network boot");
    firmware::report_identity(&mut r);
    firmware::report_network_boot(&mut r);
    r.flush();

    r.section("G. All cores");
    percore::report(&mut r, &ids, madt.as_ref(), &tables);
    r.flush();

    // Last, because it is the only probe that changes CPU memory settings.
    r.section("H. MTE tag storage");
    r.flush();
    mte::report(&mut r, &ids);
    r.flush();

    if r.file_error.is_none() {
        println!("\nReport saved to \\EFI\\orion-probe\\report.txt");
    }
    // scripts/run-qemu.sh stops QEMU when it sees this line.
    println!("Probe finished.");

    // Leave the text on screen long enough to read it on the real board.
    boot::stall(Duration::from_secs(10));
    Status::SUCCESS
}
