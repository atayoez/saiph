//! Pointer authentication (PAC) and branch target identification (BTI).
//!
//! Every function compiled with `-Zbranch-protection=bti,pac-ret` signs its
//! return address on entry (`paciasp`) and checks it before returning
//! (`autiasp`), and every possible indirect-branch target starts with a
//! `bti` landing pad. Both are hint instructions that do nothing until:
//! - PAC: an instruction key A is loaded and SCTLR.EnIA is set;
//! - BTI: the code pages are marked guarded (GP bit). SCTLR.BT then also
//!   stops `paciasp` from counting as a landing pad for plain `br` jumps.
//!
//! The SCTLR write itself happens in `kmain` (see there for why).

use limine::request::Request;

/// Limine's Entropy feature (not in the `limine` crate yet): random 64-bit
/// values the bootloader drew from its best source, such as the EFI RNG.
#[repr(C)]
pub struct EntropyResp {
    value_count: u64,
    values: *const u64,
}

/// The request's own field is the number of values wanted.
#[used]
#[unsafe(link_section = ".requests")]
pub static ENTROPY: Request<EntropyResp, u64> =
    // SAFETY: ID and layout from the Limine protocol's "Entropy Feature".
    unsafe { Request::new_raw([0x65ea_8025_5d56_82c5, 0x9117_2407_23f4_93eb], 0, 2) };

/// Where the PAC key came from.
pub enum KeySource {
    Limine,
    /// Limine gave no entropy: a weak key derived from the timer counter.
    Counter,
}

/// splitmix64: spreads the bits of a weak seed.
fn mix(mut x: u64) -> u64 {
    x = x.wrapping_add(0x9e37_79b9_7f4a_7c15);
    x = (x ^ (x >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
    x = (x ^ (x >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
    x ^ (x >> 31)
}

/// Load the 128-bit instruction key A (used by `paciasp`/`autiasp`). Safe
/// to call while PAC is still off, since nothing uses the key yet.
pub fn load_pac_key() -> KeySource {
    let (lo, hi, source) = match ENTROPY.response() {
        Some(r) if r.value_count >= 2 && !r.values.is_null() => {
            // SAFETY: Limine provides `value_count` values at `values`.
            let v = unsafe { core::slice::from_raw_parts(r.values, 2) };
            (v[0], v[1], KeySource::Limine)
        }
        _ => {
            let t: u64;
            // SAFETY: reading the counter has no side effects.
            unsafe { core::arch::asm!("mrs {}, CNTPCT_EL0", out(reg) t) };
            (mix(t), mix(t ^ 0x5a5a_5a5a), KeySource::Counter)
        }
    };
    // APIAKeyLo_EL1 / APIAKeyHi_EL1 by encoding (the assembler only knows
    // their names with PAuth enabled for the whole program).
    // SAFETY: the key is not used until SCTLR.EnIA is set.
    unsafe {
        core::arch::asm!("msr S3_0_C2_C1_0, {lo}", "msr S3_0_C2_C1_1, {hi}", "isb", lo = in(reg) lo, hi = in(reg) hi);
    }
    source
}

unsafe extern "C" {
    static __text_start: u8;
    static __text_end: u8;
}

/// Set the GP ("guarded page") bit on the kernel's code pages in Limine's
/// upper-half page tables, so BTI checks apply there. Returns the number of
/// pages marked.
///
/// The tables hold physical addresses; Limine's direct map (HHDM) makes
/// them readable at physical address + `hhdm`. TTBR1 has the root table.
pub fn guard_kernel_code(hhdm: u64) -> usize {
    const GP: u64 = 1 << 50;
    let (start, end) = (&raw const __text_start as u64, &raw const __text_end as u64);
    let ttbr1: u64;
    // SAFETY: reading TTBR1 has no side effects.
    unsafe { core::arch::asm!("mrs {}, TTBR1_EL1", out(reg) ttbr1) };
    let root = ttbr1 & 0x0000_ffff_ffff_f000;
    let mut marked = 0;
    let mut va = start & !0xfff;
    while va < end {
        let mut table = root;
        for level in 0..4 {
            let index = (va >> (39 - 9 * level)) & 511;
            let entry = (table + hhdm + 8 * index) as *mut u64;
            // SAFETY: page-table pages are bootloader-reclaimable memory,
            // which the HHDM maps; we only add the GP bit to valid leaves.
            let d = unsafe { entry.read_volatile() };
            if d & 1 == 0 {
                break; // not mapped (cannot happen for our own code)
            }
            let is_table = level < 3 && d & 2 != 0;
            if !is_table {
                // A page (level 3) or a block (levels 1-2): set GP. Only a
                // permission-like bit changes, so no break-before-make.
                unsafe { entry.write_volatile(d | GP) };
                marked += 1;
                break;
            }
            table = d & 0x0000_ffff_ffff_f000;
        }
        va += 0x1000;
    }
    // SAFETY: make the new descriptors visible and drop cached translations.
    unsafe { core::arch::asm!("dsb ishst", "tlbi vmalle1", "dsb ish", "isb") };
    marked
}

/// `pac-test`: sign a pointer, corrupt one bit of its signature, check it.
/// With FPAC the check (`autia`) traps at once: exception class 0x1c.
pub fn pac_test() {
    // pacia x0, x1 = 0xdac10020; autia x0, x1 = 0xdac11020 (encodings, as
    // above). Bit 49 is inside the signature of a 48-bit address.
    // SAFETY: deliberately faults; the handler reports it and halts.
    unsafe {
        core::arch::asm!(
            "adr x0, .",
            "mov x1, #0x1234",
            ".inst 0xdac10020",
            "eor x0, x0, #(1 << 49)",
            ".inst 0xdac11020",
            out("x0") _, out("x1") _,
        )
    };
}

/// `bti-test`: an indirect branch to an instruction that is not a `bti`
/// landing pad. On a guarded page this traps: exception class 0x0d.
pub fn bti_test() {
    // SAFETY: deliberately faults; the handler reports it and halts.
    unsafe { core::arch::asm!("adr x0, 1f", "br x0", "1:", "nop", out("x0") _) };
}
