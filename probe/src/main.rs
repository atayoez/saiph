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
mod report;

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

    if r.file_error.is_none() {
        println!("\nReport saved to \\EFI\\orion-probe\\report.txt");
    }
    // scripts/run-qemu.sh stops QEMU when it sees this line.
    println!("Probe finished.");

    // Leave the text on screen long enough to read it on the real board.
    boot::stall(Duration::from_secs(10));
    Status::SUCCESS
}
