//! Map a device's registers so the kernel can reach them.
//!
//! Limine maps the kernel and all RAM in the upper half of the address space
//! (through TTBR1), but no device registers. The lower half (TTBR0) is ours:
//! Limine already configured its walk in TCR (4 KiB pages, 48-bit
//! addresses), so we only build a table and point TTBR0 at it. Device pages
//! are mapped 1:1 (virtual address = physical address).
//!
//! With 4 KiB pages and 48-bit addresses, a lookup goes through four tables
//! (levels 0..3); each level uses 9 bits of the address as an index into a
//! 512-entry table. The `_EL1` register names also work at EL2, because VHE
//! redirects them to the EL2 registers.

/// One 4 KiB page-table page: 512 eight-byte descriptors.
#[repr(C, align(4096))]
struct Table([u64; 512]);

/// Levels 0, 1, 2 and 3 of the lower-half page table. Enough for one
/// device page for now.
static mut TABLES: [Table; 4] = [const { Table([0; 512]) }; 4];

const VALID: u64 = 1 << 0;
/// Below level 3: "points to the next table". At level 3: "maps a page".
const TABLE_OR_PAGE: u64 = 1 << 1;
/// Which MAIR slot holds the memory type (bits 4..2).
const ATTR_DEVICE: u64 = 2 << 2;
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

/// Map the 4 KiB page holding physical address `phys` as Device memory at
/// the same virtual address, and return that address. `kernel_phys`
/// translates a kernel virtual address to its physical address.
pub fn map_device_page(phys: u64, kernel_phys: impl Fn(u64) -> u64) -> u64 {
    let va = phys;
    let index = |level: u32| ((va >> (39 - 9 * level)) & 511) as usize;
    let tables = &raw mut TABLES;
    // SAFETY: this runs once, on one core, before anything else uses the
    // lower half; the tables are only reached through this pointer.
    unsafe {
        // Physical addresses of the four tables, worked out before writing.
        let pa: [u64; 4] = core::array::from_fn(|level| kernel_phys(&raw const (*tables)[level] as u64));
        let pa = |level: usize| pa[level];
        (*tables)[0].0[index(0)] = pa(1) | TABLE_OR_PAGE | VALID;
        (*tables)[1].0[index(1)] = pa(2) | TABLE_OR_PAGE | VALID;
        (*tables)[2].0[index(2)] = pa(3) | TABLE_OR_PAGE | VALID;
        (*tables)[3].0[index(3)] = (phys & !0xfff) | UXN | PXN | ACCESS_FLAG | ATTR_DEVICE | TABLE_OR_PAGE | VALID;

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
            root = in(reg) pa(0),
        );
    }
    va + (phys & 0xfff)
}
