//! Just enough ACPI to find the console, the way to call PSCI, the interrupt
//! controller and the timer's interrupt.
//!
//! The RSDP points to the XSDT, a list of physical addresses of the other
//! tables. Limine maps all ACPI memory in the higher half direct map, so a
//! table's virtual address is its physical address plus the HHDM offset.

fn u16_at(d: &[u8], at: usize) -> u16 {
    d.get(at..at + 2).map_or(0, |b| u16::from_le_bytes([b[0], b[1]]))
}
fn u32_at(d: &[u8], at: usize) -> u32 {
    d.get(at..at + 4).map_or(0, |b| u32::from_le_bytes([b[0], b[1], b[2], b[3]]))
}
fn u64_at(d: &[u8], at: usize) -> u64 {
    d.get(at..at + 8).map_or(0, |b| u64::from_le_bytes([b[0], b[1], b[2], b[3], b[4], b[5], b[6], b[7]]))
}

pub struct Tables {
    xsdt: &'static [u8],
    hhdm: u64,
}

/// A table at physical address `phys`, using the length from its header.
fn table(phys: u64, hhdm: u64) -> Option<&'static [u8]> {
    if phys == 0 {
        return None;
    }
    let va = phys + hhdm;
    // SAFETY: ACPI tables are mapped in the HHDM (Limine base revision >= 4),
    // and every table header holds its total length at offset 4.
    let len = unsafe { core::ptr::read_unaligned((va + 4) as *const u32) } as usize;
    if !(36..=1 << 20).contains(&len) {
        return None;
    }
    // SAFETY: as above.
    Some(unsafe { core::slice::from_raw_parts(va as *const u8, len) })
}

impl Tables {
    /// `rsdp` is the RSDP's virtual address as Limine reports it.
    pub fn new(rsdp: u64, hhdm: u64) -> Option<Self> {
        // SAFETY: Limine hands over a mapped ACPI 2.0 RSDP (36 bytes).
        let head = unsafe { core::slice::from_raw_parts(rsdp as *const u8, 36) };
        if &head[0..8] != b"RSD PTR " || head[15] < 2 {
            return None;
        }
        Some(Tables { xsdt: table(u64_at(head, 24), hhdm)?, hhdm })
    }

    /// The first table with this signature.
    fn find(&self, signature: &[u8; 4]) -> Option<&'static [u8]> {
        (0..(self.xsdt.len() - 36) / 8)
            .filter_map(|i| table(u64_at(self.xsdt, 36 + 8 * i), self.hhdm))
            .find(|t| &t[0..4] == signature)
    }

    /// Physical address of the console UART, from the SPCR ("Serial Port
    /// Console Redirection") table, if it is a PL011 in memory space.
    pub fn spcr_uart(&self) -> Option<u64> {
        let spcr = self.find(b"SPCR")?;
        // Interface type at offset 36 (3 = Arm PL011). The base address is a
        // "generic address" at offset 40: space ID 0 means memory-mapped, and
        // the 64-bit address follows at offset 44.
        (spcr[36] == 3 && spcr[40] == 0).then(|| u64_at(spcr, 44))
    }

    /// How to call PSCI, from the FADT's "ARM boot architecture flags"
    /// (offset 129): bit 0 = PSCI available, bit 1 = use HVC instead of SMC.
    pub fn psci_conduit(&self) -> Option<crate::psci::Conduit> {
        let flags = u16_at(self.find(b"FACP")?, 129);
        if flags & 1 == 0 {
            return None;
        }
        Some(if flags & 2 != 0 { crate::psci::Conduit::Hvc } else { crate::psci::Conduit::Smc })
    }

    /// The GIC, from the MADT ("Multiple APIC Description Table"): the
    /// distributor and the ranges where the redistributors are.
    pub fn gic(&self) -> Option<Gic> {
        let madt = self.find(b"APIC")?;
        let mut gic = Gic { distributor: 0, version: 0, redistributors: [(0, 0); 4], ranges: 0 };
        // Entries follow the 44-byte header: type, length, then the body.
        let mut at = 44;
        while at + 2 <= madt.len() {
            let (kind, len) = (madt[at], madt[at + 1] as usize);
            if len < 2 {
                break;
            }
            match kind {
                0x0c => {
                    gic.distributor = u64_at(madt, at + 8);
                    gic.version = madt.get(at + 20).copied().unwrap_or(0);
                }
                0x0e if gic.ranges < gic.redistributors.len() => {
                    gic.redistributors[gic.ranges] = (u64_at(madt, at + 4), u32_at(madt, at + 12) as u64);
                    gic.ranges += 1;
                }
                _ => {}
            }
            at += len;
        }
        (gic.distributor != 0 && gic.ranges > 0).then_some(gic)
    }

    /// The virtual timer's interrupt, from the GTDT ("Generic Timer
    /// Description Table"): the EL2 virtual timer's (GTDT revision 3 and
    /// later) when the kernel runs at EL2, the EL1 one's otherwise. Each
    /// comes with flags; bit 0 set means edge-triggered.
    pub fn virtual_timer_interrupt(&self, at_el2: bool) -> Option<(u32, bool)> {
        let gtdt = self.find(b"GTDT")?;
        let at = if at_el2 { 96 } else { 64 };
        let (intid, flags) = (u32_at(gtdt, at), u32_at(gtdt, at + 4));
        (intid != 0).then_some((intid, flags & 1 != 0))
    }
}

/// Where the GIC's parts are (physical addresses).
pub struct Gic {
    pub distributor: u64,
    /// GIC version as the firmware states it (3 or 4 for a GIC-700).
    pub version: u8,
    /// (base, length) of each range of redistributors, one per core.
    pub redistributors: [(u64, u64); 4],
    pub ranges: usize,
}
