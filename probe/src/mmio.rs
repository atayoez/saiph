//! Safe-as-possible reads of device registers (MMIO).
//!
//! Reading an address the firmware never mapped raises a translation fault,
//! and the firmware then stops with a crash dump. So before every read we
//! ask the MMU with the `AT` ("address translate") instruction whether the
//! read would work, without touching the address. The answer lands in
//! PAR_EL1: bit 0 set means "not mapped"; otherwise bits 47..12 hold the
//! physical address and bits 63..56 the memory type (0x00..0x0c = Device).

use alloc::format;
use alloc::string::String;

/// Ask the MMU whether a read of `address` would succeed; returns PAR_EL1.
fn translate(address: u64) -> u64 {
    let el = (read_sysreg!("CurrentEL") >> 2) & 3;
    let par: u64;
    // SAFETY: AT only writes PAR_EL1. Interrupts are masked (and restored) so
    // nothing else can run and change PAR_EL1 between the AT and our read.
    unsafe {
        if el == 2 {
            core::arch::asm!(
                "mrs {daif}, DAIF", "msr DAIFSet, #3",
                "at S1E2R, {addr}", "isb", "mrs {par}, PAR_EL1",
                "msr DAIF, {daif}",
                addr = in(reg) address, par = out(reg) par, daif = out(reg) _,
            );
        } else {
            core::arch::asm!(
                "mrs {daif}, DAIF", "msr DAIFSet, #3",
                "at S1E1R, {addr}", "isb", "mrs {par}, PAR_EL1",
                "msr DAIF, {daif}",
                addr = in(reg) address, par = out(reg) par, daif = out(reg) _,
            );
        }
    }
    par
}

/// Read one 32-bit device register, but only if the MMU says the address is
/// mapped 1:1 as Device memory.
pub fn read32(address: u64) -> Result<u32, String> {
    let par = translate(address);
    if par & 1 != 0 {
        return Err(format!("{address:#x} is not mapped by the firmware, not read"));
    }
    let physical = par & 0x000f_ffff_ffff_f000;
    if physical != address & !0xfff {
        return Err(format!("{address:#x} is mapped to a different address ({physical:#x}), not read"));
    }
    let kind = (par >> 56) as u8;
    if kind & 0xf0 != 0 {
        return Err(format!("{address:#x} is mapped as normal memory (attributes {kind:#04x}), not read"));
    }
    // SAFETY: the MMU confirmed a 1:1 Device mapping, and callers only pass
    // addresses of read-only ID/status registers from the ACPI tables.
    Ok(unsafe { core::ptr::read_volatile(address as *const u32) })
}
