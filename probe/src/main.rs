#![no_main]
#![no_std]

use core::arch::asm;
use core::time::Duration;
use uefi::prelude::*;
use uefi::println;

#[entry]
fn main() -> Status {
    let current_el: u64;
    // SAFETY: reading CurrentEL has no side effects.
    unsafe { asm!("mrs {}, CurrentEL", out(reg) current_el) };

    println!("Hello from the Orion probe");
    println!("Running at EL{}", (current_el >> 2) & 3);

    // Leave the text on screen long enough to read it on the real board.
    boot::stall(Duration::from_secs(10));
    Status::SUCCESS
}
