//! The kernel's own page tables, replacing Limine's.
//!
//! Upper half (TTBR1):
//! - the kernel image at the address it was linked for, one permission per
//!   segment: code read-only + executable + BTI-guarded, read-only data,
//!   read-write data, nothing both writable and executable;
//! - the direct map of RAM at Limine's HHDM offset, so every pointer we
//!   already hold stays valid. Never executable; the kernel image's alias
//!   in it is read-only;
//! - a device window: device registers at DEVICE_WINDOW + physical address.
//!
//! The lower half (TTBR0) is switched off afterwards; it is for user space.

use crate::memory;
use frames::FrameAllocator;
use limine::memmap;
use paging::{Kind, MapError, Mapper, PAGE_SIZE, TableMemory};

/// Where device registers appear: this offset plus their physical address.
/// Above the direct map (which ends below 0xffff_0100_0000_0000 for the O6's
/// 40-bit physical addresses) and below the kernel image.
pub const DEVICE_WINDOW: u64 = 0xffff_c000_0000_0000;

/// Page-table memory for the kernel: frames from the allocator, reached
/// through the direct map.
struct KernelTables<'a, 'b> {
    frames: &'a mut FrameAllocator<'b>,
    hhdm: u64,
}

impl TableMemory for KernelTables<'_, '_> {
    fn alloc_table(&mut self) -> Option<u64> {
        let frame = self.frames.alloc()?;
        // SAFETY: a frame we own, mapped by the direct map.
        unsafe { core::ptr::write_bytes((frame + self.hhdm) as *mut u8, 0, PAGE_SIZE as usize) };
        Some(frame)
    }
    fn table(&mut self, phys: u64) -> &mut [u64; 512] {
        // SAFETY: only called for tables this mapper allocated.
        unsafe { &mut *((phys + self.hhdm) as *mut [u64; 512]) }
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

/// What `build` produced.
pub struct Tables {
    pub root: u64,
    pub tables: usize,
    pub pages: usize,
    pub blocks: usize,
    pub wx_violations: usize,
    pub uart: u64,
}

/// Build the new tables (does not switch to them yet).
pub fn build(frames: &mut FrameAllocator<'_>, hhdm: u64, virt_base: u64, phys_base: u64, uart_phys: u64) -> Result<Tables, MapError> {
    let mem = KernelTables { frames, hhdm };
    let mut m = Mapper::new(mem).ok_or(MapError::OutOfMemory)?;
    let phys = |va: u64| va - virt_base + phys_base;

    // The kernel image, segment by segment. Each segment starts on a page
    // boundary (see linker.ld), so rounding a segment's end up never
    // reaches into the next one.
    let symbols = |s: &u8| s as *const u8 as u64;
    // SAFETY: linker symbols; only their addresses are used.
    let (start, text, text_end, rodata, rodata_end, data, end) = unsafe {
        (symbols(&__kernel_start), symbols(&__text_start), symbols(&__text_end), symbols(&__rodata_start),
            symbols(&__rodata_end), symbols(&__data_start), symbols(&__kernel_end))
    };
    for (from, to, kind) in [
        (start, text, Kind::ReadWrite), // Limine requests and responses
        (text, align_up(text_end), Kind::Code),
        (rodata, align_up(rodata_end), Kind::ReadOnly),
        (data, align_up(end), Kind::ReadWrite),
    ] {
        if to > from {
            m.map(from, phys(from), to - from, kind)?;
        }
    }

    // The direct map: the same memory types Limine maps there. Ranges are
    // rounded out to whole pages, sorted, and merged when they touch and
    // have the same kind (so 2 MiB blocks can span region boundaries). A
    // page claimed by two ranges keeps the kind of the one mapped first.
    let mut ranges: [(u64, u64, Kind); 128] = [(0, 0, Kind::ReadOnly); 128];
    let mut n = 0;
    for e in memory::entries() {
        let kind = match e.type_ {
            memmap::MEMMAP_USABLE
            | memmap::MEMMAP_BOOTLOADER_RECLAIMABLE
            | memmap::MEMMAP_ACPI_RECLAIMABLE
            | memmap::MEMMAP_ACPI_NVS
            | memmap::MEMMAP_MAPPED_RESERVED => Kind::ReadWrite,
            memmap::MEMMAP_EXECUTABLE_AND_MODULES => Kind::ReadOnly,
            memmap::MEMMAP_FRAMEBUFFER => Kind::Framebuffer,
            _ => continue,
        };
        if n == ranges.len() {
            break;
        }
        ranges[n] = (align_down(e.base), align_up(e.base + e.length), kind);
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
    Ok(Tables { root: m.root, tables: m.tables, pages, blocks, wx_violations: wx, uart: DEVICE_WINDOW + uart_phys })
}

/// Switch the upper half to the tables at `root`.
///
/// The new tables map the running code, stack and data at the same
/// addresses, so execution simply continues. Old and new tables may map
/// the same address with different block sizes for a moment; the O6's
/// cores (FEAT_BBM level 2) resolve that without a "TLB conflict" fault.
pub fn switch_to(root: u64) {
    // SAFETY: see above; the TLBI drops all of Limine's cached translations.
    unsafe {
        core::arch::asm!(
            "dsb ishst",
            "msr TTBR1_EL1, {root}",
            "isb",
            "tlbi vmalle1",
            "dsb ish",
            "isb",
            root = in(reg) root,
        )
    };
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
