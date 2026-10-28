//! Just enough ACPI to find the console and the way to call PSCI.
//!
//! The RSDP points to the XSDT, a list of physical addresses of the other
//! tables. Limine maps all ACPI memory in the higher half direct map, so a
//! table's virtual address is its physical address plus the HHDM offset.

fn u16_at(d: &[u8], at: usize) -> u16 {
    d.get(at..at + 2).map_or(0, |b| u16::from_le_bytes([b[0], b[1]]))
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
}
