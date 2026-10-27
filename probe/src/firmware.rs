//! Groups E and F: entropy sources, firmware identity and network boot.

use crate::acpi::{Table, find, text, u16_at, u32_at, u64_at};
use crate::cpu::{Ids, field};
use crate::report::{Report, say};
use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;
use uefi::boot::{self, SearchType};
use uefi::proto::rng::Rng;
use uefi::runtime::{self, VariableVendor};
use uefi::table::cfg::ConfigTableEntry;
use uefi::{CString16, Guid, guid, system};

// ---------------------------------------------------------------------------
// Group E: entropy.

/// Draw 16 bytes from each EFI_RNG_PROTOCOL the firmware offers.
pub fn report_efi_rng(r: &mut Report) {
    let handles = boot::find_handles::<Rng>().unwrap_or_default();
    if handles.is_empty() {
        say!(r, "EFI RNG protocol: absent");
        return;
    }
    for h in handles {
        let Ok(mut rng) = boot::open_protocol_exclusive::<Rng>(h) else {
            say!(r, "EFI RNG protocol: present, but could not be opened");
            continue;
        };
        let mut list = [uefi::proto::rng::RngAlgorithmType::EMPTY_ALGORITHM; 8];
        let algorithms: Vec<String> = match rng.get_info(&mut list) {
            // ARM_RNDR has no name in the uefi crate yet; it uses CPU RNDR.
            Ok(l) => l.iter().map(|a| if a.0 == guid!("43d2fde3-9d4e-4d79-0296-a89bca780841") { "ARM_RNDR".into() } else { format!("{a:?}") }).collect(),
            Err(_) => Vec::new(),
        };
        let mut bytes = [0u8; 16];
        let sample = match rng.get_rng(None, &mut bytes) {
            Ok(()) => bytes.iter().map(|b| format!("{b:02x}")).collect::<String>(),
            Err(e) => format!("draw failed ({:?})", e.status()),
        };
        say!(r, "EFI RNG protocol: present, algorithms [{}], 16 random bytes: {sample}", algorithms.join(", "));
    }
}

#[derive(Clone, Copy, PartialEq)]
enum Conduit {
    Smc,
    Hvc,
}

/// One SMCCC call: function ID in x0, argument in x1; results come back in
/// x0..x3, and x4..x17 may be overwritten by the firmware.
fn smccc(conduit: Conduit, function: u32, arg: u64) -> i64 {
    let mut x0 = function as u64;
    // SAFETY: only read-only version queries are passed in (see callers).
    unsafe {
        match conduit {
            Conduit::Smc => core::arch::asm!("smc #0", inout("x0") x0, inout("x1") arg => _,
                out("x2") _, out("x3") _, out("x4") _, out("x5") _, out("x6") _, out("x7") _,
                out("x8") _, out("x9") _, out("x10") _, out("x11") _, out("x12") _, out("x13") _,
                out("x14") _, out("x15") _, out("x16") _, out("x17") _),
            Conduit::Hvc => core::arch::asm!("hvc #0", inout("x0") x0, inout("x1") arg => _,
                out("x2") _, out("x3") _, out("x4") _, out("x5") _, out("x6") _, out("x7") _,
                out("x8") _, out("x9") _, out("x10") _, out("x11") _, out("x12") _, out("x13") _,
                out("x14") _, out("x15") _, out("x16") _, out("x17") _),
        }
    }
    x0 as i32 as i64 // results are 32-bit for these calls; negative = error
}

fn version(v: i64) -> String {
    if v < 0 { format!("not supported ({v})") } else { format!("{}.{}", v >> 16, v & 0xffff) }
}

const PSCI_VERSION: u32 = 0x8400_0000;
const PSCI_FEATURES: u32 = 0x8400_000a;
const SMCCC_VERSION: u32 = 0x8000_0000;
const TRNG_VERSION: u32 = 0x8400_0050;

/// Ask the firmware for its PSCI, SMCCC and TRNG versions.
pub fn report_smccc(r: &mut Report, ids: &Ids, tables: &[Table]) {
    let el = (read_sysreg!("CurrentEL") >> 2) & 3;
    // FADT "ARM boot architecture flags" (offset 129): bit 0 = PSCI is
    // available, bit 1 = call it with HVC instead of SMC.
    let flags = find(tables, "FACP").map_or(0, |t| u16_at(t.data, 129));
    let el3 = field(ids.pfr0, 12) != 0;
    let conduit = if el == 2 {
        // From EL2 only SMC makes sense, and only if something answers it.
        (el3 || flags & 3 == 1).then_some(Conduit::Smc)
    } else if flags & 1 == 1 {
        Some(if flags & 2 != 0 { Conduit::Hvc } else { Conduit::Smc })
    } else {
        None
    };
    let Some(c) = conduit else {
        say!(r, "SMCCC: no safe way to call the firmware from EL{el}, skipped");
        return;
    };
    say!(r, "SMCCC calls go through {}", if c == Conduit::Smc { "SMC (to EL3 firmware)" } else { "HVC (to the hypervisor)" });
    let psci = smccc(c, PSCI_VERSION, 0);
    say!(r, "  PSCI version:  {}", version(psci));
    // The SMCCC spec says to check PSCI_FEATURES(SMCCC_VERSION) first.
    let smccc_v = if psci >= 0x1_0000 && smccc(c, PSCI_FEATURES, SMCCC_VERSION as u64) >= 0 {
        smccc(c, SMCCC_VERSION, 0)
    } else {
        0x1_0000 // no SMCCC_VERSION call means SMCCC 1.0
    };
    say!(r, "  SMCCC version: {}", version(smccc_v));
    if smccc_v >= 0x1_0001 {
        say!(r, "  TRNG version:  {}", version(smccc(c, TRNG_VERSION, 0)));
    } else {
        say!(r, "  TRNG version:  not asked (needs SMCCC 1.1 or later)");
    }
}

// ---------------------------------------------------------------------------
// Group F: firmware identity and network boot.

/// String number `n` (1-based) from the text area after an SMBIOS structure.
fn smbios_string(s: &[u8], formatted_len: usize, n: u8) -> String {
    if n == 0 {
        return String::new();
    }
    s.get(formatted_len..).unwrap_or(&[]).split(|&b| b == 0).nth(n as usize - 1).map(text).unwrap_or_default()
}

pub fn report_identity(r: &mut Report) {
    let rev = system::uefi_revision();
    say!(r, "Firmware: {} revision {:#x}, UEFI {}.{}", system::firmware_vendor(), system::firmware_revision(), rev.major(), rev.minor());

    // SMBIOS 3: a 24-byte entry point with the table length (offset 12) and
    // address (offset 16). The table is a list of structures: type, length,
    // fields, then their strings, ended by an empty string.
    let entry = system::with_config_table(|t| t.iter().find(|e| e.guid == ConfigTableEntry::SMBIOS3_GUID).map(|e| e.address as u64));
    let Some(entry) = entry else {
        say!(r, "SMBIOS: no SMBIOS 3 table");
        return;
    };
    // SAFETY: the firmware published this entry point.
    let ep = unsafe { core::slice::from_raw_parts(entry as *const u8, 24) };
    let (len, address) = (u32_at(ep, 12) as usize, u64_at(ep, 16));
    // SAFETY: as above; the entry point gives the table's address and size.
    let table = unsafe { core::slice::from_raw_parts(address as *const u8, len) };
    let mut at = 0;
    while at + 4 <= table.len() {
        let (kind, flen) = (table[at], table[at + 1] as usize);
        if flen < 4 || at + flen > table.len() {
            break;
        }
        let mut end = at + flen; // strings end with two zero bytes
        while end + 1 < table.len() && !(table[end] == 0 && table[end + 1] == 0) {
            end += 1;
        }
        let s = &table[at..end + 1];
        let st = |i: usize| smbios_string(s, flen, s.get(i).copied().unwrap_or(0));
        match kind {
            0 => say!(r, "SMBIOS BIOS:   vendor '{}', version '{}', date '{}'", st(4), st(5), st(8)),
            1 => say!(r, "SMBIOS system: '{}' '{}'", st(4), st(5)),
            2 => say!(r, "SMBIOS board:  '{}' '{}' version '{}'", st(4), st(5), st(6)),
            127 => break,
            _ => {}
        }
        at = end + 2;
    }
}

/// The description of boot option `Boot####`: after a 32-bit attribute
/// field and a 16-bit length comes the description as UCS-2 text.
fn boot_option(n: u16) -> Option<String> {
    let name = CString16::try_from(format!("Boot{n:04X}").as_str()).ok()?;
    let (data, _) = runtime::get_variable_boxed(&name, &VariableVendor::GLOBAL_VARIABLE).ok()?;
    let words: Vec<u16> = data.get(6..)?.chunks_exact(2).map(|c| u16::from_le_bytes([c[0], c[1]])).take_while(|&w| w != 0).collect();
    Some(char::decode_utf16(words).map(|c| c.unwrap_or('?')).collect())
}

fn count(guid: &Guid) -> usize {
    boot::locate_handle_buffer(SearchType::ByProtocol(guid)).map_or(0, |h| h.len())
}

pub fn report_network_boot(r: &mut Report) {
    let order = CString16::try_from("BootOrder").unwrap();
    let order: Vec<u16> = runtime::get_variable_boxed(&order, &VariableVendor::GLOBAL_VARIABLE)
        .map(|(d, _)| d.chunks_exact(2).map(|c| u16::from_le_bytes([c[0], c[1]])).collect())
        .unwrap_or_default();
    say!(r, "Boot menu entries ({}):", order.len());
    for n in order {
        let desc = boot_option(n).unwrap_or_else(|| "(unreadable)".to_string());
        let lower = desc.to_ascii_lowercase();
        let tag = if lower.contains("http") { "  <- HTTP network boot" } else if lower.contains("pxe") { "  <- PXE network boot" } else { "" };
        say!(r, "  Boot{n:04X}: {desc}{tag}");
    }
    // These protocols only exist once the firmware has started a network card.
    let nics = count(&guid!("a19832b9-ac25-11d3-9a2d-0090273fc14d")); // Simple Network
    let pxe = count(&guid!("03c4e603-ac28-11d3-9a2d-0090273fc14d")); // PXE Base Code
    let http = count(&guid!("bdc8e6af-d9bc-4379-a72a-e0c4e75dae1c")); // HTTP Service Binding
    say!(r, "Network cards started by the firmware: {nics}; PXE: {pxe}; HTTP: {http}");
    if nics == 0 {
        say!(r, "  (no network card started yet; the boot menu entries above are the better answer)");
    }
}
