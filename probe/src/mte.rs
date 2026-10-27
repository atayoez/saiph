//! Group H: does MTE tag storage work at this exception level?
//!
//! Tags only work on memory mapped with the "Tagged" type (MAIR value 0xf0)
//! and with SCTLR_ELx.ATA (bit 43) set. UEFI uses neither, so the test sets
//! both up for one page the probe allocated, writes and reads two tags, and
//! restores every register and page-table entry it touched.

use crate::cpu::{Ids, field};
use crate::report::{Report, say};
use alloc::format;
use alloc::string::String;
use core::arch::asm;
use uefi::boot::{self, AllocateType, MemoryType};

const TAGGED: u64 = 0xf0; // MAIR memory type: Tagged Normal, write-back cacheable

/// The current exception level's translation registers.
struct Regs {
    el: u64,
    e2h: bool,
    sctlr: u64,
    tcr: u64,
    mair: u64,
    ttbr0: u64,
}

fn regs() -> Regs {
    let el = (read_sysreg!("CurrentEL") >> 2) & 3;
    if el == 2 {
        Regs {
            el,
            e2h: (read_sysreg!("HCR_EL2") >> 34) & 1 == 1,
            sctlr: read_sysreg!("SCTLR_EL2"),
            tcr: read_sysreg!("TCR_EL2"),
            mair: read_sysreg!("MAIR_EL2"),
            ttbr0: read_sysreg!("TTBR0_EL2"),
        }
    } else {
        Regs {
            el,
            e2h: false,
            sctlr: read_sysreg!("SCTLR_EL1"),
            tcr: read_sysreg!("TCR_EL1"),
            mair: read_sysreg!("MAIR_EL1"),
            ttbr0: read_sysreg!("TTBR0_EL1"),
        }
    }
}

fn set_sctlr(el: u64, v: u64) {
    // SAFETY: callers only flip the ATA bit and restore the old value.
    unsafe {
        if el == 2 {
            asm!("msr SCTLR_EL2, {}", "isb", in(reg) v);
        } else {
            asm!("msr SCTLR_EL1, {}", "isb", in(reg) v);
        }
    }
}

fn set_mair(el: u64, v: u64) {
    // SAFETY: callers only change a slot no page-table entry uses.
    unsafe {
        if el == 2 {
            asm!("msr MAIR_EL2, {}", "isb", in(reg) v);
        } else {
            asm!("msr MAIR_EL1, {}", "isb", in(reg) v);
        }
    }
}

/// Forget any cached translation of `va` (TLB = the MMU's translation cache).
fn flush_tlb(el: u64, va: u64) {
    let page = va >> 12;
    // SAFETY: TLB maintenance only drops cached translations.
    unsafe {
        if el == 2 {
            asm!("dsb ishst", "tlbi vae2is, {}", "dsb ish", "isb", in(reg) page);
        } else {
            asm!("dsb ishst", "tlbi vaae1is, {}", "dsb ish", "isb", in(reg) page);
        }
    }
}

/// Ask the MMU (with AT) whether `va` is mapped for writing; returns PAR_EL1.
fn translate_for_write(el: u64, va: u64) -> u64 {
    let par: u64;
    // SAFETY: AT only writes PAR_EL1; interrupts are masked meanwhile.
    unsafe {
        if el == 2 {
            asm!("mrs {d}, DAIF", "msr DAIFSet, #3", "at S1E2W, {va}", "isb", "mrs {p}, PAR_EL1", "msr DAIF, {d}",
                va = in(reg) va, p = out(reg) par, d = out(reg) _);
        } else {
            asm!("mrs {d}, DAIF", "msr DAIFSet, #3", "at S1E1W, {va}", "isb", "mrs {p}, PAR_EL1", "msr DAIF, {d}",
                va = in(reg) va, p = out(reg) par, d = out(reg) _);
        }
    }
    par
}

// ---------------------------------------------------------------------------
// Reading the firmware's page tables (4 KiB pages only).

struct Walk {
    root: u64,
    start: i32, // first level; -1 exists only with 52-bit (LPA2) tables
    va_bits: u32,
    lpa2: bool,
}

/// Address bits a descriptor at `level` translates: 12 + 9 per level below 3.
fn shift(level: i32) -> u32 {
    (12 + 9 * (3 - level)) as u32
}

fn walk_setup(g: &Regs) -> Result<Walk, String> {
    // TCR: T0SZ (bits 5..0) gives the address size as 64 - T0SZ; TG0 (bits
    // 15..14) the page size (0 = 4 KiB); DS turns on 52-bit descriptors and
    // sits at bit 32 in the EL2-only layout (E2H=0) and at bit 59 otherwise.
    if (g.tcr >> 14) & 3 != 0 {
        return Err(String::from("the firmware does not use 4 KiB pages"));
    }
    let lpa2 = if g.el == 2 && !g.e2h { (g.tcr >> 32) & 1 } else { (g.tcr >> 59) & 1 } == 1;
    let va_bits = 64 - (g.tcr & 0x3f) as u32;
    let start = match va_bits {
        49.. => -1,
        40..=48 => 0,
        31..=39 => 1,
        _ => 2,
    };
    // TTBR0 holds the root table address in bits 47..1; with 52-bit tables
    // bits 5..2 hold address bits 51..48.
    let mut root = g.ttbr0 & 0x0000_ffff_ffff_ffc0;
    if lpa2 {
        root |= ((g.ttbr0 >> 2) & 0xf) << 48;
    }
    Ok(Walk { root, start, va_bits, lpa2 })
}

/// Address held in a table or block/page descriptor.
fn output(w: &Walk, d: u64) -> u64 {
    if w.lpa2 { (d & 0x0003_ffff_ffff_f000) | (((d >> 8) & 3) << 50) } else { d & 0x0000_ffff_ffff_f000 }
}

fn entries(w: &Walk, level: i32) -> u64 {
    if level == w.start { 1 << (w.va_bits - shift(level)) } else { 512 }
}

/// Descriptor that maps `va`: (level, address of the descriptor, value).
fn find_descriptor(w: &Walk, va: u64) -> Result<(i32, u64, u64), String> {
    let (mut table, mut level) = (w.root, w.start);
    loop {
        let index = (va >> shift(level)) & (entries(w, level) - 1);
        let at = table + 8 * index;
        // SAFETY: page tables are ordinary memory, mapped 1:1 by UEFI.
        let d = unsafe { core::ptr::read_volatile(at as *const u64) };
        if d & 1 == 0 {
            return Err(format!("{va:#x} is not mapped"));
        }
        // Bit 1 set below level 3: a pointer to the next table.
        if level < 3 && d & 2 != 0 {
            table = output(w, d);
            level += 1;
            continue;
        }
        return Ok((level, at, d));
    }
}

/// Which of the 8 MAIR slots any mapping uses (AttrIndx, descriptor bits 4..2).
fn used_slots(w: &Walk) -> u8 {
    fn visit(w: &Walk, table: u64, level: i32, used: &mut u8, budget: &mut u32) {
        for i in 0..entries(w, level) {
            // SAFETY: as in find_descriptor.
            let d = unsafe { core::ptr::read_volatile((table + 8 * i) as *const u64) };
            if d & 1 == 0 {
                continue;
            }
            if level < 3 && d & 2 != 0 {
                if *budget > 0 {
                    *budget -= 1;
                    visit(w, output(w, d), level + 1, used, budget);
                }
            } else {
                *used |= 1 << ((d >> 2) & 7);
            }
        }
    }
    let mut used = 0;
    let mut budget = 20_000; // tables to visit at most
    visit(w, w.root, w.start, &mut used, &mut budget);
    used
}

// ---------------------------------------------------------------------------
// The tag instructions. `stg` stores the tag held in bits 59..56 of its
// first register for the 16-byte block at the second; `ldg` loads it back.

#[target_feature(enable = "mte")]
unsafe fn store_tag(address: u64, tag: u64) {
    let tagged = address | (tag << 56);
    // SAFETY: `address` is in a page the probe owns, mapped as Tagged.
    unsafe { asm!("stg {t}, [{a}]", "dsb sy", t = in(reg) tagged, a = in(reg) address) };
}

#[target_feature(enable = "mte")]
unsafe fn load_tag(address: u64) -> u64 {
    let mut v = address;
    // SAFETY: as above.
    unsafe { asm!("ldg {v}, [{a}]", v = inout(reg) v, a = in(reg) address) };
    (v >> 56) & 0xf
}

// ---------------------------------------------------------------------------

pub fn report(r: &mut Report, ids: &Ids) {
    let mte = field(ids.pfr1, 8);
    if mte < 2 {
        say!(r, "Skipped: MTE={mte}, tag storage needs MTE2 or later");
        return;
    }
    let g = regs();
    let el = g.el;
    // TCF (SCTLR bits 41..40): what happens on a tag mismatch. Leave the test
    // alone if the firmware already turned tag checking on.
    if (g.sctlr >> 40) & 3 != 0 {
        say!(r, "Skipped: tag checking is already enabled (SCTLR_EL{el}.TCF)");
        return;
    }
    let w = match walk_setup(&g) {
        Ok(w) => w,
        Err(e) => {
            say!(r, "Skipped: {e}");
            return;
        }
    };

    // 4 MiB of memory contains a 2 MiB-aligned 2 MiB window that is entirely
    // ours, so even a 2 MiB block mapping can be changed without touching
    // anyone else's memory.
    let Ok(block) = boot::allocate_pages(AllocateType::AnyPages, MemoryType::LOADER_DATA, 1024) else {
        say!(r, "Skipped: could not allocate memory for the test");
        return;
    };
    let page = (block.as_ptr() as u64 + 0x1f_ffff) & !0x1f_ffff;
    let result = run_test(r, &g, &w, page);
    // SAFETY: allocated above; every change was undone in run_test.
    let _ = unsafe { boot::free_pages(block, 1024) };
    match result {
        Ok(s) | Err(s) => say!(r, "  => {s}"),
    }
}

fn run_test(r: &mut Report, g: &Regs, w: &Walk, page: u64) -> Result<String, String> {
    let el = g.el;
    let (level, at, old) = find_descriptor(w, page).map_err(|e| format!("Skipped: {e}"))?;
    say!(r, "Test page {page:#x}: level-{level} {} descriptor {old:#018x}", if level == 3 { "page" } else { "block" });
    if level < 2 {
        return Err(String::from("Skipped: mapped by a 1 GiB block, which would need splitting"));
    }
    if translate_for_write(el, at) & 1 != 0 {
        return Err(String::from("Skipped: the firmware's page tables are read-only"));
    }
    let slot = match (0..8).find(|&i| (g.mair >> (8 * i)) & 0xff == TAGGED) {
        Some(i) => i,
        None => {
            let used = used_slots(w);
            (0..8).rev().find(|&i| used & (1 << i) == 0).ok_or_else(|| String::from("Skipped: all 8 MAIR slots are in use"))?
        }
    };
    let new_mair = (g.mair & !(0xff << (8 * slot))) | (TAGGED << (8 * slot));
    let new = (old & !(7 << 2)) | ((slot as u64) << 2);
    say!(r, "Using MAIR slot {slot} for Tagged memory; SCTLR_EL{el}.ATA on for the test");

    let write = |value: u64| {
        // SAFETY: `at` is the descriptor of memory only the probe uses.
        unsafe { asm!("str {v}, [{a}]", "dc civac, {a}", "dsb ish", v = in(reg) value, a = in(reg) at) };
    };
    let daif: u64;
    // SAFETY: mask interrupts so nothing runs while the mapping changes.
    unsafe { asm!("mrs {}, DAIF", "msr DAIFSet, #3", out(reg) daif) };
    set_mair(el, new_mair);
    write(0); // break ...
    flush_tlb(el, page);
    write(new); // ... before make
    flush_tlb(el, page);
    set_sctlr(el, g.sctlr | 1 << 43);

    // SAFETY: the page is ours and now mapped as Tagged with tag access on.
    let (a, b) = unsafe {
        store_tag(page, 0xa);
        store_tag(page + 16, 0x5);
        let tags = (load_tag(page), load_tag(page + 16));
        store_tag(page, 0);
        store_tag(page + 16, 0);
        tags
    };

    set_sctlr(el, g.sctlr);
    write(0);
    flush_tlb(el, page);
    write(old);
    flush_tlb(el, page);
    set_mair(el, g.mair);
    // SAFETY: restore the interrupt mask.
    unsafe { asm!("msr DAIF, {}", in(reg) daif) };

    say!(r, "Wrote tags 0xa and 0x5, read back {a:#x} and {b:#x}; everything restored");
    if (a, b) == (0xa, 0x5) {
        Ok(format!("MTE tag storage works at EL{el}"))
    } else {
        Err(String::from("tags did not stick: tag storage is not working here"))
    }
}
