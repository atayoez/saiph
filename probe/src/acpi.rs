//! Group C: ACPI tables.
//!
//! The firmware publishes the RSDP ("Root System Description Pointer") in the
//! EFI configuration table. The RSDP points to the XSDT, a list of 64-bit
//! addresses of all other tables. Every table starts with the same 36-byte
//! header: signature (4 ASCII letters), length, revision, checksum and the
//! vendor's names. DSDT and FACS are not in the XSDT; the FADT points to them.

use crate::report::{Report, say};
use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use uefi::system;
use uefi::table::cfg::ConfigTableEntry;

pub struct Table {
    pub signature: String,
    pub address: u64,
    pub data: &'static [u8],
}

pub fn u16_at(d: &[u8], at: usize) -> u16 {
    d.get(at..at + 2).map_or(0, |b| u16::from_le_bytes([b[0], b[1]]))
}
pub fn u32_at(d: &[u8], at: usize) -> u32 {
    d.get(at..at + 4).map_or(0, |b| u32::from_le_bytes([b[0], b[1], b[2], b[3]]))
}
pub fn u64_at(d: &[u8], at: usize) -> u64 {
    d.get(at..at + 8).map_or(0, |b| u64::from_le_bytes(b.try_into().unwrap()))
}

/// Printable text from a fixed-size field (non-printable bytes become '.').
pub fn text(b: &[u8]) -> String {
    b.iter().take_while(|&&c| c != 0).map(|&c| if (0x20..0x7f).contains(&c) { c as char } else { '.' }).collect()
}

/// The table at `address`, using the length from its own header.
fn table_at(address: u64) -> Option<&'static [u8]> {
    if address == 0 {
        return None;
    }
    // SAFETY: the address comes from the firmware's own table list, and UEFI
    // maps all memory 1:1 (virtual address = physical address).
    let len = unsafe { core::ptr::read_unaligned((address + 4) as *const u32) } as usize;
    if !(36..=16 << 20).contains(&len) {
        return None;
    }
    // SAFETY: as above; the firmware says the table is `len` bytes long.
    Some(unsafe { core::slice::from_raw_parts(address as *const u8, len) })
}

fn checksum_ok(d: &[u8]) -> bool {
    d.iter().fold(0u8, |sum, &b| sum.wrapping_add(b)) == 0
}

/// Find the RSDP and collect every table it leads to.
pub fn tables(r: &mut Report) -> Vec<Table> {
    let rsdp = system::with_config_table(|t| t.iter().find(|e| e.guid == ConfigTableEntry::ACPI2_GUID).map(|e| e.address as u64));
    let Some(rsdp) = rsdp else {
        say!(r, "No ACPI tables (no ACPI 2.0 RSDP in the EFI configuration table)");
        return Vec::new();
    };
    // SAFETY: an ACPI 2.0 RSDP is 36 bytes.
    let head = unsafe { core::slice::from_raw_parts(rsdp as *const u8, 36) };
    say!(r, "RSDP at {rsdp:#x}: signature '{}', OEM '{}', revision {}", text(&head[0..8]), text(&head[9..15]), head[15]);
    let xsdt_addr = u64_at(head, 24);
    let Some(xsdt) = table_at(xsdt_addr) else {
        say!(r, "XSDT at {xsdt_addr:#x} is unreadable");
        return Vec::new();
    };
    let mut list = Vec::new();
    list.push(Table { signature: text(&xsdt[0..4]), address: xsdt_addr, data: xsdt });
    for i in 0..(xsdt.len() - 36) / 8 {
        let address = u64_at(xsdt, 36 + 8 * i);
        if let Some(data) = table_at(address) {
            list.push(Table { signature: text(&data[0..4]), address, data });
        }
    }
    // The FADT (signature FACP) holds the 64-bit DSDT address at offset 140.
    if let Some(fadt) = list.iter().find(|t| t.signature == "FACP").map(|t| t.data) {
        let address = u64_at(fadt, 140);
        if let Some(data) = table_at(address) {
            list.push(Table { signature: text(&data[0..4]), address, data });
        }
    }
    list
}

pub fn report_tables(r: &mut Report, list: &[Table]) {
    say!(r, "sig   address         length  rev  OEM     OEM table  checksum  saved as");
    let mut seen: Vec<(String, u32)> = Vec::new();
    for t in list {
        // Some signatures (e.g. SSDT) appear more than once: ssdt.dat, ssdt1.dat, ...
        let n = match seen.iter_mut().find(|(s, _)| *s == t.signature) {
            Some(entry) => {
                entry.1 += 1;
                entry.1
            }
            None => {
                seen.push((t.signature.clone(), 0));
                0
            }
        };
        let base: String = t.signature.chars().map(|c| if c.is_ascii_alphanumeric() { c.to_ascii_lowercase() } else { '_' }).collect();
        let name = if n == 0 { format!("{base}.dat") } else { format!("{base}{n}.dat") };
        let saved = match r.save_file(&format!("acpi\\{name}"), t.data) {
            Ok(()) => format!("acpi\\{name}"),
            Err(e) => format!("(not saved: {e})"),
        };
        say!(r, "{:<4}  {:#012x}  {:>6}  {:>3}  {:<6}  {:<9}  {:<8}  {saved}",
            t.signature, t.address, t.data.len(), t.data[8], text(&t.data[10..16]), text(&t.data[16..24]),
            if checksum_ok(t.data) { "ok" } else { "BAD" });
    }
}

pub fn find<'a>(list: &'a [Table], signature: &str) -> Option<&'a Table> {
    list.iter().find(|t| t.signature == signature)
}

/// One CPU as the MADT describes it.
pub struct Cpu {
    pub mpidr: u64,
    pub efficiency: u8,
    pub enabled: bool,
}

/// What the MADT says about the interrupt controller.
#[derive(Default)]
pub struct Madt {
    /// Distributor (GICD) base address and the GIC version the firmware states.
    pub gicd: Option<(u64, u8)>,
    /// Redistributor (GICR) regions: base and length.
    pub gicr: Vec<(u64, u32)>,
    /// Interrupt Translation Services (ITS) base addresses.
    pub its: Vec<u64>,
    /// One entry per CPU.
    pub cpus: Vec<Cpu>,
}

/// Parse the MADT (signature "APIC"). After its 44-byte header come
/// variable-length entries, each starting with a type byte and a length byte.
pub fn madt(list: &[Table]) -> Option<Madt> {
    let d = find(list, "APIC")?.data;
    let mut m = Madt::default();
    let mut at = 44;
    while at + 2 <= d.len() {
        let (kind, len) = (d[at], d[at + 1] as usize);
        if len < 2 || at + len > d.len() {
            break;
        }
        let e = &d[at..at + len];
        match kind {
            // GIC CPU interface: flags at 12 (bit 0 = enabled), MPIDR at 68,
            // power-efficiency class at 76.
            0x0b => m.cpus.push(Cpu { mpidr: u64_at(e, 68), efficiency: e.get(76).copied().unwrap_or(0), enabled: u32_at(e, 12) & 1 == 1 }),
            0x0c => m.gicd = Some((u64_at(e, 8), e[20])),                          // GIC distributor
            0x0e => m.gicr.push((u64_at(e, 4), u32_at(e, 12))),                    // redistributor region
            0x0f => m.its.push(u64_at(e, 8)),                                      // ITS
            _ => {}
        }
        at += len;
    }
    Some(m)
}

pub fn report_madt(r: &mut Report, m: &Madt) {
    if let Some((base, version)) = m.gicd {
        say!(r, "GIC distributor (GICD) at {base:#x}, version stated by firmware: {version}");
    }
    for (base, len) in &m.gicr {
        say!(r, "GIC redistributors (GICR) at {base:#x}, {} KiB", len / 1024);
    }
    for base in &m.its {
        say!(r, "GIC ITS at {base:#x}");
    }
    let mut classes: Vec<u8> = m.cpus.iter().map(|c| c.efficiency).collect();
    classes.sort_unstable();
    classes.dedup();
    say!(r, "{} CPUs; power-efficiency classes {classes:?}", m.cpus.len());
    for c in &m.cpus {
        say!(r, "  CPU MPIDR {:#011x}  efficiency class {}{}", c.mpidr, c.efficiency, if c.enabled { "" } else { "  (disabled)" });
    }
}

/// The debug serial port from the DBG2 table: (port subtype, base address).
/// Subtype 0x0003 is an Arm PL011 UART, 0x000e an Arm SBSA generic UART.
pub fn dbg2_uart(list: &[Table]) -> Option<(u16, u64)> {
    let d = find(list, "DBG2")?.data;
    let at = u32_at(d, 36) as usize; // offset of the first device entry
    let e = d.get(at..)?;
    let subtype = u16_at(e, 14);
    let regs = u16_at(e, 18) as usize; // offset of its register addresses
    Some((subtype, u64_at(e, regs + 4)))
}

/// Base addresses of the SMMUv3 units listed in the IORT. After its header
/// the IORT has a node count (offset 36) and the offset of the first node
/// (offset 40); each node starts with a type byte and a 16-bit length, and
/// SMMUv3 nodes (type 4) keep their base address at offset 16.
pub fn iort_smmus(list: &[Table]) -> Vec<u64> {
    let Some(d) = find(list, "IORT").map(|t| t.data) else { return Vec::new() };
    let mut out = Vec::new();
    let mut at = u32_at(d, 40) as usize;
    for _ in 0..u32_at(d, 36) {
        let len = u16_at(d, at + 1) as usize;
        if len < 16 || at + len > d.len() {
            break;
        }
        if d[at] == 4 {
            out.push(u64_at(d, at + 16));
        }
        at += len;
    }
    out
}
