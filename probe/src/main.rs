#![no_main]
#![no_std]

use core::arch::asm;
use core::time::Duration;
use uefi::prelude::*;
use uefi::println;

/// Extract the 4-bit ID-register field that starts at bit `lsb`.
///
/// Shifting right by `lsb` moves the field down to bits 3..0, and `& 0xf`
/// (binary 1111) clears everything above them, leaving just the field.
fn field(reg: u64, lsb: u32) -> u64 {
    (reg >> lsb) & 0xf
}

#[entry]
fn main() -> Status {
    let current_el: u64;
    // SAFETY: reading CurrentEL has no side effects.
    unsafe { asm!("mrs {}, CurrentEL", out(reg) current_el) };

    println!("Hello from the Orion probe");
    // CurrentEL keeps the level in bits 3..2, so shift by 2 and keep 2 bits.
    println!("Running at EL{}", (current_el >> 2) & 3);

    // ID_AA64PFR1_EL1 ("Processor Feature Register 1") describes optional
    // CPU features. BT is bits 3..0 and MTE is bits 11..8 (see the register's
    // page in the Arm Architecture Reference Manual).
    let pfr1: u64;
    // SAFETY: ID registers are read-only; reading them has no side effects.
    unsafe { asm!("mrs {}, ID_AA64PFR1_EL1", out(reg) pfr1) };

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

    // scripts/run-qemu.sh stops QEMU when it sees this line.
    println!("Probe finished.");

    // Leave the text on screen long enough to read it on the real board.
    boot::stall(Duration::from_secs(10));
    Status::SUCCESS
}
