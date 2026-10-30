//! Kernel stacks, each in its own range of addresses with an unmapped
//! guard page below it, so running off the end of a stack faults at once
//! instead of overwriting whatever lies below (Linux's VMAP_STACK).
//!
//! Every core has two. Kernel code runs on the kernel stack through SP_EL0
//! (PSTATE.SPSel = 0). Taking an exception switches the core to its own
//! stack pointer, SP_EL2 (SP_EL1 under KVM), which points at the exception
//! stack. So even a fault caused by a stack overflow has a working stack to
//! report it from, and interrupt handlers don't run on the interrupted
//! code's stack.
//!
//! Each core's stacks live in a 1 MiB slot from `STACKS_BASE`:
//! guard page, kernel stack, guard page, exception stack, nothing.
//! Threads (see sched.rs) get a kernel stack each, in 128 KiB slots after
//! the cores' (guard page, stack, nothing); exceptions taken while a thread
//! runs use its core's exception stack.

use crate::{memory, vm};
use paging::{Kind, PAGE_SIZE};

/// Where the stack slots start: between the heap region and the kernel.
pub const STACKS_BASE: u64 = 0xffff_e000_0000_0000;
const SLOT: u64 = 1 << 20;
const KERNEL_STACK: u64 = 64 * 1024;
const EXCEPTION_STACK: u64 = 32 * 1024;
/// Most cores there are slots for (the O6 has 12).
pub const MAX_CORES: usize = 64;
/// Threads' stack slots, after the cores'.
const THREADS_BASE: u64 = STACKS_BASE + MAX_CORES as u64 * SLOT;
const THREAD_SLOT: u64 = 128 * 1024;
pub const MAX_THREADS: usize = 4096;

/// The tops of one core's two stacks (stacks grow down).
#[derive(Clone, Copy)]
pub struct Stacks {
    pub kernel_top: u64,
    pub exception_top: u64,
}

/// Map fresh frames at `[va, va + len)`.
fn map_fresh(va: u64, len: u64) -> Result<(), vm::Error> {
    for page in (va..va + len).step_by(PAGE_SIZE as usize) {
        let frame = memory::alloc_frame().ok_or(vm::Error::Map(paging::MapError::OutOfMemory))?;
        if let Err(e) = vm::map(page, frame, PAGE_SIZE, Kind::ReadWrite) {
            memory::free_frame(frame);
            return Err(e);
        }
    }
    Ok(())
}

/// Map core `core`'s stacks (after `vm::switch_to`).
pub fn allocate(core: usize) -> Result<Stacks, vm::Error> {
    assert!(core < MAX_CORES);
    let slot = STACKS_BASE + core as u64 * SLOT;
    let kernel = slot + PAGE_SIZE; // a guard page below
    let exception = kernel + KERNEL_STACK + PAGE_SIZE; // and another
    map_fresh(kernel, KERNEL_STACK)?;
    map_fresh(exception, EXCEPTION_STACK)?;
    Ok(Stacks { kernel_top: kernel + KERNEL_STACK, exception_top: exception + EXCEPTION_STACK })
}

/// Map the kernel stack of thread `n`; returns its top.
pub fn allocate_thread(n: usize) -> Result<u64, vm::Error> {
    assert!(n < MAX_THREADS);
    let stack = THREADS_BASE + n as u64 * THREAD_SLOT + PAGE_SIZE; // a guard page below
    map_fresh(stack, KERNEL_STACK)?;
    Ok(stack + KERNEL_STACK)
}

/// Whose stack's guard page an address is in.
pub enum Guard {
    /// A core's kernel stack (`false`) or exception stack (`true`).
    Core(usize, bool),
    Thread(usize),
}

/// If `addr` is in a stack's guard page: whose stack it is.
pub fn guard_page_of(addr: u64) -> Option<Guard> {
    if let Some(offset) = addr.checked_sub(THREADS_BASE) {
        let thread = (offset / THREAD_SLOT) as usize;
        return (thread < MAX_THREADS && offset % THREAD_SLOT < PAGE_SIZE).then_some(Guard::Thread(thread));
    }
    let offset = addr.checked_sub(STACKS_BASE)?;
    let core = (offset / SLOT) as usize;
    match offset % SLOT {
        o if o < PAGE_SIZE => Some(Guard::Core(core, false)),
        o if (PAGE_SIZE + KERNEL_STACK..2 * PAGE_SIZE + KERNEL_STACK).contains(&o) => Some(Guard::Core(core, true)),
        _ => None,
    }
}

/// Move this core onto `stacks` and call `next(arg)` there, never to come
/// back: SP_EL2 (or SP_EL1) gets the exception stack, SP_EL0 the kernel
/// stack, and SPSel = 0 selects SP_EL0 for kernel code.
pub fn switch(stacks: Stacks, next: extern "C" fn(usize) -> !, arg: usize) -> ! {
    // SAFETY: both stacks are mapped (see `allocate`); nothing on the old
    // stack is used afterwards except through `arg`, which the caller keeps
    // alive (the old stack is never reused).
    unsafe {
        core::arch::asm!(
            "msr SP_EL0, {kernel}",
            "mov sp, {exception}", // SPSel is 1, so this sets SP_EL2/SP_EL1
            "msr SPSel, #0",
            "blr x9",
            "1: wfe",
            "b 1b",
            kernel = in(reg) stacks.kernel_top,
            exception = in(reg) stacks.exception_top,
            in("x0") arg,
            in("x9") next,
            options(noreturn),
        )
    }
}
