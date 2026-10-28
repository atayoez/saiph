//! Exception handling: the vector table, saving and restoring the
//! interrupted code's registers, and a report for anything unexpected.
//!
//! VBAR holds the address of a 2 KiB vector table: 16 slots of 128 bytes,
//! four kinds of exception (synchronous, IRQ, FIQ, SError) for each of four
//! origins (this level on SP_EL0, this level on its own stack pointer, a
//! lower level in AArch64, a lower level in AArch32). Each slot saves x0/x1,
//! puts its slot number in x0 and jumps to common code, which saves the
//! remaining registers plus ELR/SPSR/ESR/FAR as a `Frame` on the stack,
//! calls `saiph_exception`, restores everything and returns with `eret`.
//! The `_EL1` register names reach the EL2 registers under VHE.

use crate::uart;
use core::fmt::Write;

/// The interrupted code's registers, as the vector code saves them.
#[repr(C)]
pub struct Frame {
    pub x: [u64; 31],
    /// Where the exception happened, and where `eret` returns to.
    pub elr: u64,
    /// Processor state at the exception (restored by `eret`).
    pub spsr: u64,
    /// Exception syndrome: class in bits 31..26, details below.
    pub esr: u64,
    /// The faulting address, for memory faults.
    pub far: u64,
    _pad: u64,
}

core::arch::global_asm!(
    ".section .text.vectors, \"ax\"",
    ".balign 2048",
    ".global saiph_vectors",
    "saiph_vectors:",
    ".irp slot, 0,1,2,3,4,5,6,7,8,9,10,11,12,13,14,15",
    ".balign 128",
    "sub sp, sp, #{size}",
    "stp x0, x1, [sp, #0]",
    "mov x0, #\\slot",
    "b saiph_exception_common",
    ".endr",
    "",
    "saiph_exception_common:",
    "stp x2, x3, [sp, #16]",
    "stp x4, x5, [sp, #32]",
    "stp x6, x7, [sp, #48]",
    "stp x8, x9, [sp, #64]",
    "stp x10, x11, [sp, #80]",
    "stp x12, x13, [sp, #96]",
    "stp x14, x15, [sp, #112]",
    "stp x16, x17, [sp, #128]",
    "stp x18, x19, [sp, #144]",
    "stp x20, x21, [sp, #160]",
    "stp x22, x23, [sp, #176]",
    "stp x24, x25, [sp, #192]",
    "stp x26, x27, [sp, #208]",
    "stp x28, x29, [sp, #224]",
    "str x30, [sp, #240]",
    "mrs x1, ELR_EL1",
    "mrs x2, SPSR_EL1",
    "stp x1, x2, [sp, #248]",
    "mrs x1, ESR_EL1",
    "mrs x2, FAR_EL1",
    "stp x1, x2, [sp, #264]",
    "mov x1, sp",
    "bl saiph_exception", // (slot, &mut Frame)
    // The handler may change ELR (to skip an instruction) and SPSR.
    "ldp x1, x2, [sp, #248]",
    "msr ELR_EL1, x1",
    "msr SPSR_EL1, x2",
    "ldp x2, x3, [sp, #16]",
    "ldp x4, x5, [sp, #32]",
    "ldp x6, x7, [sp, #48]",
    "ldp x8, x9, [sp, #64]",
    "ldp x10, x11, [sp, #80]",
    "ldp x12, x13, [sp, #96]",
    "ldp x14, x15, [sp, #112]",
    "ldp x16, x17, [sp, #128]",
    "ldp x18, x19, [sp, #144]",
    "ldp x20, x21, [sp, #160]",
    "ldp x22, x23, [sp, #176]",
    "ldp x24, x25, [sp, #192]",
    "ldp x26, x27, [sp, #208]",
    "ldp x28, x29, [sp, #224]",
    "ldr x30, [sp, #240]",
    "ldp x0, x1, [sp, #0]",
    "add sp, sp, #{size}",
    "eret",
    size = const size_of::<Frame>(),
);

unsafe extern "C" {
    static saiph_vectors: u8;
}

/// Point VBAR at our vector table. From here on, exceptions are reported.
pub fn install() {
    // SAFETY: the table is complete and 2 KiB aligned; the ISB makes the
    // new VBAR take effect before the next instruction.
    unsafe { core::arch::asm!("msr VBAR_EL1, {}", "isb", in(reg) &raw const saiph_vectors) };
}

/// Immediate of the boot-time self-test breakpoint (`brk #SELF_TEST`).
/// Other breakpoints, such as the `brk #1` the compiler emits for traps,
/// are reported as errors.
pub const SELF_TEST: u16 = 0x0b07;

/// Execute the self-test breakpoint; returns once the handler has skipped it.
pub fn self_test() {
    // SAFETY: the handler resumes after this instruction.
    unsafe { core::arch::asm!("brk #{}", const SELF_TEST) };
}

fn origin(slot: u64) -> (&'static str, &'static str) {
    let kind = ["synchronous exception", "IRQ", "FIQ", "SError"][(slot & 3) as usize];
    let from = ["this level, on SP_EL0", "this level", "a lower level (AArch64)", "a lower level (AArch32)"][(slot >> 2) as usize];
    (kind, from)
}

/// Human-readable exception class (ESR bits 31..26).
fn class(ec: u64) -> &'static str {
    match ec {
        0x00 => "undefined instruction (or unknown reason)",
        0x01 => "WFI/WFE trapped",
        0x07 => "FP/SIMD/SVE access trapped",
        0x0d => "BTI failure (branch to a non-landing-pad instruction)",
        0x0e => "illegal execution state",
        0x15 => "SVC (system call)",
        0x16 => "HVC",
        0x17 => "SMC",
        0x18 => "system register access trapped",
        0x19 => "SVE access trapped",
        0x1c => "pointer authentication failure (FPAC)",
        0x20 | 0x21 => "instruction abort (fetching code)",
        0x22 => "PC alignment fault",
        0x24 | 0x25 => "data abort (memory access)",
        0x26 => "SP alignment fault",
        0x2c => "floating-point exception",
        0x2f => "SError",
        0x30..=0x35 => "debug event (breakpoint, step or watchpoint)",
        0x3c => "BRK instruction",
        _ => "other",
    }
}

/// For instruction and data aborts: the fault status code (ESR bits 5..0).
fn fault_status(fsc: u64) -> alloc_free::Text {
    let level = fsc & 3;
    match fsc {
        0x04..=0x07 => alloc_free::Text::with_level("translation fault (address not mapped)", level),
        0x08..=0x0b => alloc_free::Text::with_level("access flag fault", level),
        0x0c..=0x0f => alloc_free::Text::with_level("permission fault", level),
        0x10 => alloc_free::Text::plain("synchronous external abort"),
        0x11 => alloc_free::Text::plain("MTE tag check fault"),
        0x21 => alloc_free::Text::plain("alignment fault"),
        _ => alloc_free::Text::plain("other fault"),
    }
}

/// Tiny helper so the report needs no heap.
mod alloc_free {
    pub struct Text {
        pub what: &'static str,
        pub level: Option<u64>,
    }
    impl Text {
        pub fn plain(what: &'static str) -> Self {
            Text { what, level: None }
        }
        pub fn with_level(what: &'static str, level: u64) -> Self {
            Text { what, level: Some(level) }
        }
    }
    impl core::fmt::Display for Text {
        fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
            match self.level {
                Some(l) => write!(f, "{}, page-table level {l}", self.what),
                None => f.write_str(self.what),
            }
        }
    }
}

/// Called by the vector code for every exception.
#[unsafe(no_mangle)]
extern "C" fn saiph_exception(slot: u64, frame: &mut Frame) {
    let ec = (frame.esr >> 26) & 0x3f;
    let synchronous = slot & 3 == 0;
    if synchronous && ec == 0x3c && frame.esr & 0xffff == SELF_TEST as u64 {
        if let Some(mut out) = uart::console() {
            let _ = writeln!(out, "  exception self-test: brk #{SELF_TEST:#x} at {:#x} handled, resuming", frame.elr);
        }
        frame.elr += 4; // continue after the brk instruction
        return;
    }

    if let Some(mut out) = uart::console() {
        let (kind, from) = origin(slot);
        let _ = writeln!(out, "\n*** unexpected {kind} from {from}");
        let _ = writeln!(out, "    cause: {} (ESR={:#x}, class {ec:#04x})", class(ec), frame.esr);
        if synchronous && matches!(ec, 0x20 | 0x21 | 0x24 | 0x25) {
            let write = ec >= 0x24 && frame.esr & (1 << 6) != 0; // WnR bit
            let _ = writeln!(out, "    {}, {} address {:#x}", fault_status(frame.esr & 0x3f),
                if ec <= 0x21 { "fetching" } else if write { "writing" } else { "reading" }, frame.far);
        }
        let _ = writeln!(out, "    at ELR={:#x}  SPSR={:#x}", frame.elr, frame.spsr);
        for row in 0..8 {
            let _ = write!(out, "   ");
            for col in 0..4 {
                let n = row * 4 + col;
                if n < 31 {
                    let _ = write!(out, " x{n:<2}={:#018x}", frame.x[n]);
                }
            }
            let _ = writeln!(out);
        }
        let _ = writeln!(out, "    halting.");
    }
    crate::halt()
}
