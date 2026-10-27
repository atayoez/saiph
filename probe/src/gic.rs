//! Group D, part 1: which interrupt controller (GIC) this is.
//!
//! The GIC's distributor (GICD), per-CPU redistributors (GICR) and ITS have
//! ID registers at fixed offsets. IIDR says who made it and which product;
//! PIDR2 says the architecture version; GICR_TYPER.RVPEID says GICv4.1.

use crate::acpi::Madt;
use crate::mmio::read32;
use crate::report::{Report, say};

/// Product name from an IIDR: implementer in bits 11..0 (0x43b = Arm),
/// product ID in bits 31..24.
fn product(iidr: u32) -> &'static str {
    if iidr & 0xfff != 0x43b {
        return "not an Arm design";
    }
    match iidr >> 24 {
        0x00 => "Arm GIC-500 (QEMU's emulated GIC also reports this)",
        0x02 => "Arm GIC-600",
        0x04 => "Arm GIC-700",
        0x4b => "KVM's emulated GIC",
        _ => "unknown Arm GIC",
    }
}

/// "rNpM" revision from an IIDR: variant in bits 19..16, revision in 15..12.
fn revision(iidr: u32) -> alloc::string::String {
    alloc::format!("r{}p{}", (iidr >> 16) & 0xf, (iidr >> 12) & 0xf)
}

pub fn report(r: &mut Report, m: &Madt) {
    let Some((gicd, _)) = m.gicd else {
        say!(r, "No GIC distributor in the MADT");
        return;
    };
    let read = |r: &mut Report, address: u64| match read32(address) {
        Ok(v) => Some(v),
        Err(e) => {
            say!(r, "  {e}");
            None
        }
    };

    let Some(iidr) = read(r, gicd + 0x0008) else { return };
    say!(r, "GICD_IIDR  = {iidr:#010x}  {} {}", product(iidr), revision(iidr));
    let Some(pidr2) = read(r, gicd + 0xffe8) else { return };
    let arch = (pidr2 >> 4) & 0xf; // ArchRev, bits 7..4
    say!(r, "GICD_PIDR2 = {pidr2:#010x}  architecture GICv{arch}");
    if let Some(typer) = read(r, gicd + 0x0004) {
        // ITLinesNumber (bits 4..0): the highest shared interrupt number is
        // 32 * (N + 1) - 1. LPIS (bit 17): message-based interrupts.
        // DVIS (bit 18): direct injection of virtual interrupts (GICv4).
        let max = 32 * ((typer & 0x1f) + 1) - 1;
        say!(r, "GICD_TYPER = {typer:#010x}  interrupts up to {max}, LPIs {}, direct virtual injection {}",
            if typer >> 17 & 1 == 1 { "yes" } else { "no" }, if typer >> 18 & 1 == 1 { "yes" } else { "no" });
    }
    if let Some(typer2) = read(r, gicd + 0x000c) {
        say!(r, "GICD_TYPER2 = {typer2:#010x}");
    }

    // The first redistributor frame. GICR_TYPER bit 1 (VLPIS): virtual LPIs;
    // bit 7 (RVPEID): the GICv4.1 way of tracking virtual CPUs.
    let mut v41 = None;
    if let Some(&(gicr, _)) = m.gicr.first() {
        if let (Some(iidr), Some(typer)) = (read(r, gicr + 0x0004), read(r, gicr + 0x0008)) {
            say!(r, "GICR_IIDR  = {iidr:#010x}  {} {}", product(iidr), revision(iidr));
            say!(r, "GICR_TYPER = {typer:#010x}  (low half)  virtual LPIs {}, RVPEID {}",
                if typer >> 1 & 1 == 1 { "yes" } else { "no" }, typer >> 7 & 1);
            v41 = Some(typer >> 7 & 1 == 1);
        }
    }

    // ITS: GITS_TYPER bit 1 (Virtual) means it can deliver virtual interrupts.
    for &its in &m.its {
        if let (Some(iidr), Some(typer)) = (read(r, its + 0x0004), read(r, its + 0x0008)) {
            say!(r, "GITS_IIDR  = {iidr:#010x}  {} {}  (ITS at {its:#x})", product(iidr), revision(iidr));
            say!(r, "GITS_TYPER = {typer:#010x}  (low half)  virtual interrupts {}", if typer >> 1 & 1 == 1 { "yes" } else { "no" });
        }
    }

    let version = match (arch, v41) {
        (4, Some(true)) => "GICv4.1",
        (4, _) => "GICv4.0",
        (3, _) => "GICv3",
        _ => "unknown",
    };
    say!(r, "  => {} implementing {version}", product(iidr));
}
