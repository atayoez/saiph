//! The lower half while Limine's page tables are in use: device registers,
//! and the page that switches to our own tables.
//!
//! Limine maps the kernel and all RAM in the upper half of the address space
//! (through TTBR1), but no device registers. The lower half (TTBR0) is ours:
//! Limine already configured its walk in TCR (4 KiB pages, 48-bit
//! addresses), so we only build tables and point TTBR0 at them. Pages are
//! mapped 1:1 (virtual address = physical address).
//!
//! With 4 KiB pages and 48-bit addresses, a lookup goes through four tables
//! (levels 0..3); each level uses 9 bits of the address as an index into a
//! 512-entry table. The `_EL1` register names also work at EL2, because VHE
//! redirects them to the EL2 registers.

/// One 4 KiB page-table page: 512 eight-byte descriptors.
#[repr(C, align(4096))]
struct Table([u64; 512]);

/// The tables: [0] is the root (level 0), the rest are handed out as the
/// mappings need them. Two pages need at most seven.
static mut TABLES: [Table; 8] = [const { Table([0; 512]) }; 8];
/// Tables handed out so far (0 = TTBR0 not set up yet).
static mut USED: usize = 0;

const VALID: u64 = 1 << 0;
/// Below level 3: "points to the next table". At level 3: "maps a page".
const TABLE_OR_PAGE: u64 = 1 << 1;
/// Which MAIR slot holds the memory type (bits 4..2).
const ATTR_DEVICE: u64 = 2 << 2;
/// MAIR slot 0 is Limine's normal, write-back cached memory.
const ATTR_NORMAL: u64 = 0 << 2;
/// AP[2]: read-only.
const READ_ONLY: u64 = 1 << 7;
/// Inner shareable: kept coherent between all cores.
const INNER_SHAREABLE: u64 = 3 << 8;
/// Access flag: without it, the first access faults.
const ACCESS_FLAG: u64 = 1 << 10;
/// Never execute: not by the kernel (PXN), not by user code (UXN).
const PXN: u64 = 1 << 53;
const UXN: u64 = 1 << 54;
/// MAIR slot 2 (Limine leaves slots 2..7 unused) set to Device-nGnRnE: no
/// caching, no merging, no reordering, no early acknowledgement.
const MAIR_SLOT: u64 = 2;
const DEVICE_NGNRNE: u64 = 0x00;

pub fn read_current_el() -> u64 {
    let v: u64;
    // SAFETY: reading CurrentEL has no side effects.
    unsafe { core::arch::asm!("mrs {}, CurrentEL", out(reg) v) };
    v
}

/// Map the 4 KiB page at physical `phys` at the same virtual address, with
/// `attributes` (memory type and permissions). `kernel_phys` translates a
/// kernel virtual address to its physical address. False if the tables
/// ran out or something else is mapped there.
fn map_identity(phys: u64, attributes: u64, kernel_phys: impl Fn(u64) -> u64) -> bool {
    let va = phys & !0xfff;
    let index = |level: u32| ((va >> (39 - 9 * level)) & 511) as usize;
    let tables = &raw mut TABLES;
    let used = &raw mut USED;
    // SAFETY: this runs on one core, before anything else uses the lower
    // half; the tables are only reached through these pointers.
    unsafe {
        // Physical addresses of the tables, worked out before writing.
        let pa: [u64; 8] = core::array::from_fn(|n| kernel_phys(&raw const (*tables)[n] as u64));
        let table_pa = |n: usize| pa[n];
        let first = *used == 0;
        if first {
            *used = 1;
        }
        let mut table = 0;
        for level in 0..3 {
            let d = (*tables)[table].0[index(level)];
            table = if d & VALID == 0 {
                if *used == (*tables).len() {
                    return false;
                }
                let new = *used;
                *used += 1;
                (*tables)[table].0[index(level)] = table_pa(new) | TABLE_OR_PAGE | VALID;
                new
            } else {
                // Find which of our tables this descriptor points to.
                match (1..*used).find(|&n| table_pa(n) == d & 0x0000_ffff_ffff_f000) {
                    Some(n) => n,
                    None => return false,
                }
            };
        }
        let slot = &mut (*tables)[table].0[index(3)];
        if *slot & VALID != 0 {
            return false;
        }
        *slot = va | attributes | ACCESS_FLAG | TABLE_OR_PAGE | VALID;

        if first {
            let mut mair: u64;
            core::arch::asm!("mrs {}, MAIR_EL1", out(reg) mair);
            mair = (mair & !(0xff << (8 * MAIR_SLOT))) | (DEVICE_NGNRNE << (8 * MAIR_SLOT));
            core::arch::asm!(
                "msr MAIR_EL1, {mair}",
                "dsb ishst",        // table writes visible to the page-table walker
                "msr TTBR0_EL1, {root}",
                "isb",
                "tlbi vmalle1",     // drop any stale translations (EL2&0 regime under VHE)
                "dsb ish",
                "isb",
                mair = in(reg) mair,
                root = in(reg) table_pa(0),
            );
        } else {
            // A new entry where there was none: nothing cached to drop.
            core::arch::asm!("dsb ishst", "isb");
        }
    }
    true
}

/// Map the 4 KiB page holding physical address `phys` as Device memory at
/// the same virtual address, and return the virtual address of `phys`.
/// `kernel_phys` translates a kernel virtual address to its physical address.
pub fn map_device_page(phys: u64, kernel_phys: impl Fn(u64) -> u64) -> u64 {
    map_identity(phys, ATTR_DEVICE | UXN | PXN, kernel_phys);
    phys
}

/// Map the 4 KiB page of kernel code at physical address `phys` at the
/// same virtual address, read-only and executable. False if that failed.
pub fn map_identity_code(phys: u64, kernel_phys: impl Fn(u64) -> u64) -> bool {
    map_identity(phys, ATTR_NORMAL | INNER_SHAREABLE | READ_ONLY | UXN, kernel_phys)
}
