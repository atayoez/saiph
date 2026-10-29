//! The kernel's own page tables, replacing Limine's.
//!
//! Upper half (TTBR1):
//! - the kernel image at the address it was linked for, one permission per
//!   segment: code read-only + executable + BTI-guarded, read-only data,
//!   read-write data, nothing both writable and executable;
//! - the direct map of RAM at Limine's HHDM offset, so every pointer we
//!   already hold stays valid. Never executable; the kernel image's alias
//!   in it is read-only;
//! - a device window: device registers at DEVICE_WINDOW + physical address;
//! - the heap region (`heap::HEAP_BASE`), mapped page by page as the heap
//!   grows, through `map`.
//!
//! The lower half (TTBR0) is switched off afterwards; it is for user space.

use crate::{memory, mmu, screen};
use limine::memmap;
use paging::{ENTRIES, Kind, MapError, Mapper, PAGE_SIZE, TableMemory};
use spin::Mutex;

/// Where device registers appear: this offset plus their physical address.
/// Above the direct map (which ends below 0xffff_0100_0000_0000 for the O6's
/// 40-bit physical addresses) and below the kernel image.
pub const DEVICE_WINDOW: u64 = 0xffff_c000_0000_0000;

/// The tables the kernel runs on, once `switch_to` has switched to them.
static RUNNING: Mutex<Option<Mapper<KernelTables>>> = Mutex::new(None);

/// Page-table memory for the kernel: frames from the frame allocator,
/// reached through the direct map.
struct KernelTables {
    hhdm: u64,
}

impl TableMemory for KernelTables {
    fn alloc_table(&mut self) -> Option<u64> {
        let frame = memory::alloc_frame()?;
        // SAFETY: a frame we own, mapped by the direct map.
        unsafe { core::ptr::write_bytes((frame + self.hhdm) as *mut u8, 0, PAGE_SIZE as usize) };
        Some(frame)
    }
    fn table(&mut self, phys: u64) -> &mut [u64; ENTRIES] {
        // SAFETY: only called for tables this mapper allocated.
        unsafe { &mut *((phys + self.hhdm) as *mut [u64; ENTRIES]) }
    }
}

unsafe extern "C" {
    static __kernel_start: u8;
    static __text_start: u8;
    static __text_end: u8;
    static __rodata_start: u8;
    static __rodata_end: u8;
    static __data_start: u8;
    static __kernel_end: u8;
}

fn align_down(x: u64) -> u64 {
    x & !(PAGE_SIZE - 1)
}
fn align_up(x: u64) -> u64 {
    x.div_ceil(PAGE_SIZE) * PAGE_SIZE
}

/// Why `build` failed.
#[derive(Debug)]
pub enum Error {
    Map(MapError),
    /// The memory map has more regions than `build` has room for.
    TooManyRegions,
    /// `map` was called before `switch_to`.
    NotRunning,
    /// The page that switches tables could not be mapped in the lower half.
    Trampoline,
    /// The CPU cannot translate with 16 KiB pages.
    NoSixteenKiB,
    /// Limine loaded the kernel at a physical address that is not a
    /// multiple of the page size.
    KernelMisaligned,
}

impl core::fmt::Display for Error {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Error::Map(e) => write!(f, "{e:?}"),
            Error::TooManyRegions => write!(f, "the memory map has too many regions"),
            Error::NotRunning => write!(f, "the kernel's page tables are not in use yet"),
            Error::Trampoline => write!(f, "could not map the table switch at its physical address"),
            Error::NoSixteenKiB => write!(f, "this CPU has no 16 KiB pages"),
            Error::KernelMisaligned => write!(f, "the kernel's physical address is not 16 KiB-aligned"),
        }
    }
}

impl From<MapError> for Error {
    fn from(e: MapError) -> Self {
        Error::Map(e)
    }
}

/// What `build` produced.
pub struct Tables {
    pub root: u64,
    pub tables: usize,
    pub pages: usize,
    pub blocks: usize,
    pub wx_violations: usize,
    pub uart: u64,
    mapper: Mapper<KernelTables>,
    /// Physical (= lower-half virtual) address of `saiph_switch_tables`.
    trampoline: u64,
}

/// Build the new tables (does not switch to them yet).
/// `guard_code`: mark the code pages BTI-guarded (only for a kernel built
/// with landing pads, see `protect::built_with_branch_protection`).
pub fn build(hhdm: u64, virt_base: u64, phys_base: u64, uart_phys: u64, guard_code: bool) -> Result<Tables, Error> {
    // ID_AA64MMFR0.TGran16, bits 23..20: 0 = no 16 KiB pages.
    let mmfr0: u64;
    // SAFETY: reading an ID register has no side effects.
    unsafe { core::arch::asm!("mrs {}, ID_AA64MMFR0_EL1", out(reg) mmfr0) };
    if (mmfr0 >> 20) & 0xf == 0 {
        return Err(Error::NoSixteenKiB);
    }
    if !phys_base.is_multiple_of(PAGE_SIZE) {
        return Err(Error::KernelMisaligned);
    }
    let mut m = Mapper::new(KernelTables { hhdm }).ok_or(MapError::OutOfMemory)?;
    let phys = |va: u64| va - virt_base + phys_base;

    // The kernel image, segment by segment. Each segment starts on a 16 KiB
    // page boundary (see linker.ld), so rounding a segment's end up never
    // reaches into the next one.
    let symbols = |s: &u8| s as *const u8 as u64;
    // SAFETY: linker symbols; only their addresses are used.
    let (start, text, text_end, rodata, rodata_end, data, end) = unsafe {
        (symbols(&__kernel_start), symbols(&__text_start), symbols(&__text_end), symbols(&__rodata_start),
            symbols(&__rodata_end), symbols(&__data_start), symbols(&__kernel_end))
    };
    for (from, to, kind) in [
        (start, text, Kind::ReadWrite), // Limine requests and responses
        (text, align_up(text_end), if guard_code { Kind::Code } else { Kind::UnguardedCode }),
        (rodata, align_up(rodata_end), Kind::ReadOnly),
        (data, align_up(end), Kind::ReadWrite),
    ] {
        if to > from {
            m.map(from, phys(from), to - from, kind)?;
        }
    }

    // The direct map: the same memory types Limine maps there, plus the
    // screens in use (in case the memory map does not list them). Ranges
    // are rounded out to whole pages, sorted, and merged when they touch and
    // have the same kind (so 32 MiB blocks can span region boundaries). A
    // page claimed by two ranges keeps the kind of the one mapped first.
    let memory = memory::entries().iter().filter_map(|e| {
        let kind = match e.type_ {
            memmap::MEMMAP_USABLE
            | memmap::MEMMAP_BOOTLOADER_RECLAIMABLE
            | memmap::MEMMAP_ACPI_RECLAIMABLE
            | memmap::MEMMAP_ACPI_NVS
            | memmap::MEMMAP_MAPPED_RESERVED => Kind::ReadWrite,
            memmap::MEMMAP_EXECUTABLE_AND_MODULES => Kind::ReadOnly,
            memmap::MEMMAP_FRAMEBUFFER => Kind::Framebuffer,
            _ => return None,
        };
        Some((e.base, e.length, kind))
    });
    let screens = screen::regions(hhdm).map(|(base, len)| (base, len, Kind::Framebuffer));
    let mut ranges: [(u64, u64, Kind); 256] = [(0, 0, Kind::ReadOnly); 256];
    let mut n = 0;
    for (base, len, kind) in memory.chain(screens) {
        if n == ranges.len() {
            return Err(Error::TooManyRegions);
        }
        ranges[n] = (align_down(base), align_up(base + len), kind);
        n += 1;
    }
    let ranges = &mut ranges[..n];
    ranges.sort_unstable_by_key(|r| r.0);
    let mut mapped_to = 0; // everything below this is already mapped
    let mut i = 0;
    while i < ranges.len() {
        let (from, mut to, kind) = ranges[i];
        let from = from.max(mapped_to);
        let mut j = i + 1;
        while j < ranges.len() && ranges[j].2 == kind && ranges[j].0 <= to {
            to = to.max(ranges[j].1);
            j += 1;
        }
        if from < to {
            m.map(from + hhdm, from, to - from, kind)?;
            mapped_to = to;
        }
        i = j;
    }

    // The console UART in the device window.
    let uart_page = align_down(uart_phys);
    m.map(DEVICE_WINDOW + uart_page, uart_page, PAGE_SIZE, Kind::Device)?;

    // Count what was built and check W^X over every mapping.
    let (mut pages, mut blocks, mut wx) = (0, 0, 0);
    m.for_each_leaf(|_, _, size, d| {
        if size == PAGE_SIZE { pages += 1 } else { blocks += 1 }
        if paging::writable_and_executable(d) {
            wx += 1;
        }
    });
    // The switch runs from its physical address, mapped in the lower half.
    let trampoline = phys(saiph_switch_tables as *const () as u64);
    if !mmu::map_identity_code(trampoline, phys) {
        return Err(Error::Trampoline);
    }
    Ok(Tables { root: m.root, tables: m.tables, pages, blocks, wx_violations: wx, uart: DEVICE_WINDOW + uart_phys, mapper: m, trampoline })
}

// Switching the upper half from Limine's tables to ours, break-before-make
// style. Simply loading TTBR1 is not safe: for a moment the TLB could hold
// an old and a new translation of the same address with different block
// sizes, and the architecture allows a "TLB conflict" fault then (only
// cores with FEAT_BBML3, a 2025 extension, promise not to; the O6's have
// BBM level 2). So, like Linux, this code runs from its identity mapping in
// the lower half, where nothing changes: it turns translation through TTBR1
// off (TCR.EPD1), drops every cached translation, then installs the new
// tables and the new TCR, and returns to the caller in the upper half.
// It must not touch the stack or anything else in the upper half.
// The new TCR also switches the upper half from Limine's 4 KiB pages to
// our 16 KiB ones, which is only allowed while TTBR1 is not in use.
//
// saiph_switch_tables(root: x0, tcr: x1)
core::arch::global_asm!(
    ".section .text.trampoline, \"ax\"",
    ".balign 4096",
    ".global saiph_switch_tables",
    "saiph_switch_tables:",
    "bti c",
    "mrs x2, TCR_EL1",
    "orr x2, x2, #(1 << 23)", // EPD1: no more walks through TTBR1
    "msr TCR_EL1, x2",
    "isb",
    "tlbi vmalle1",           // forget every cached translation
    "dsb ish",
    "isb",
    "msr TTBR1_EL1, x0",
    "msr TCR_EL1, x1",        // the new TCR, with EPD1 clear again
    "isb",
    "tlbi vmalle1",           // in case TCR fields were cached
    "dsb ish",
    "isb",
    "ret",
);

unsafe extern "C" {
    fn saiph_switch_tables(root: u64, tcr: u64);
}

/// TCR bits: EPD1 (no walks through TTBR1) and TG1 (the upper half's page
/// size: 0b01 = 16 KiB, 0b10 = 4 KiB).
const EPD1: u64 = 1 << 23;
const TG1: u64 = 0b11 << 30;
const TG1_16K: u64 = 0b01 << 30;

/// Switch the upper half to the tables `build` made, with 16 KiB pages,
/// and keep them for `map`. The new tables map the running code, stack and
/// data at the same addresses, so execution simply continues.
pub fn switch_to(tables: Tables) {
    let tcr: u64;
    // SAFETY: reading TCR has no side effects.
    unsafe { core::arch::asm!("mrs {}, TCR_EL1", out(reg) tcr) };
    let tcr = (tcr & !(EPD1 | TG1)) | TG1_16K;
    // SAFETY: `trampoline` is saiph_switch_tables at its identity mapping
    // (see `build`); the tables are complete; the DSB makes them visible
    // to the table walker first.
    unsafe {
        core::arch::asm!("dsb ishst");
        let switch: unsafe extern "C" fn(u64, u64) = core::mem::transmute(tables.trampoline as usize);
        switch(tables.root, tcr);
    }
    *RUNNING.lock() = Some(tables.mapper);
}

/// Map `len` bytes at virtual `va` to physical `pa` in the tables the
/// kernel is running on, where nothing is mapped yet.
pub fn map(va: u64, pa: u64, len: u64, kind: Kind) -> Result<(), Error> {
    let mut running = RUNNING.lock();
    let m = running.as_mut().ok_or(Error::NotRunning)?;
    m.map(va, pa, len, kind)?;
    // The new descriptors replace invalid ones, and the TLB never keeps
    // invalid ones, so there is nothing to invalidate. The barriers make
    // the writes visible to the MMU before anything uses the addresses.
    // SAFETY: barriers only.
    unsafe { core::arch::asm!("dsb ishst", "isb") };
    Ok(())
}

/// Stop translating lower-half addresses (TCR.EPD0): any access there now
/// faults. The lower half is reserved for user space.
pub fn disable_lower_half() {
    // SAFETY: nothing uses the lower half any more (the console moved to
    // the device window first).
    unsafe {
        core::arch::asm!(
            "mrs {t}, TCR_EL1",
            "orr {t}, {t}, #(1 << 7)",
            "msr TCR_EL1, {t}",
            "isb",
            "tlbi vmalle1",
            "dsb ish",
            "isb",
            t = out(reg) _,
        )
    };
}
