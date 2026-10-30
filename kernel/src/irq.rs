//! Interrupts: what happens when the GIC signals one.
//!
//! The exception vector for "IRQ" calls `handle`. It asks the GIC which
//! interrupt it is, runs the handler registered for it, and tells the GIC
//! it is done, until none is left. Taking the exception masks further
//! interrupts on this core (PSTATE.I), and `eret` restores the mask the
//! interrupted code had, so handlers are never interrupted themselves.
//!
//! Handlers run on the core's exception stack (see stacks.rs), in the
//! middle of whatever the core was doing. So they do little.

use crate::gic;
use crate::sync::{IrqsOff, SpinLock};
use core::fmt::Write;
use core::sync::atomic::{AtomicBool, AtomicU32, AtomicUsize, Ordering};

/// A handler for each private interrupt (SGIs and PPIs): the address of a
/// `fn()`, 0 for none. Each entry is one value, so atomics are enough and
/// taking an interrupt involves no lock.
static HANDLERS: [AtomicUsize; 32] = [const { AtomicUsize::new(0) }; 32];
crate::cpu_local! {
    /// How many handlers are running on this core.
    static IN_HANDLER: AtomicU32 = AtomicU32::new(0);
}
/// Interrupts that arrived without a handler, and the last one's number.
static UNEXPECTED: AtomicU32 = AtomicU32::new(0);
static LAST_UNEXPECTED: AtomicU32 = AtomicU32::new(0);

/// True inside an interrupt handler.
pub fn in_handler() -> bool {
    IN_HANDLER.get().load(Ordering::Relaxed) != 0
}

/// Run `handler` whenever interrupt `intid` (0-31) arrives.
pub fn set_handler(intid: u32, handler: fn()) {
    if let Some(slot) = HANDLERS.get(intid as usize) {
        slot.store(handler as usize, Ordering::Release);
    }
}

/// Let interrupts in on this core (clear PSTATE.I).
pub fn unmask() {
    // SAFETY: the vector table and the GIC are set up before this is called.
    unsafe { core::arch::asm!("msr DAIFClr, #2", "isb", options(nostack)) };
}

/// Called by the exception vector for every IRQ.
pub fn handle() {
    IN_HANDLER.get().fetch_add(1, Ordering::Relaxed);
    loop {
        let intid = gic::acknowledge();
        if (1020..1024).contains(&intid) {
            break; // nothing (more) pending
        }
        let handler = HANDLERS.get(intid as usize).map_or(0, |slot| slot.load(Ordering::Acquire));
        if handler != 0 {
            // SAFETY: nonzero entries are `fn()` addresses (`set_handler`).
            let handler = unsafe { core::mem::transmute::<usize, fn()>(handler) };
            handler();
        } else {
            // Nobody wants it: switch it off so it cannot come back.
            gic::disable(intid);
            UNEXPECTED.fetch_add(1, Ordering::Relaxed);
            LAST_UNEXPECTED.store(intid, Ordering::Relaxed);
        }
        gic::end_of_interrupt(intid);
    }
    IN_HANDLER.get().fetch_sub(1, Ordering::Relaxed);
}

/// Interrupts that arrived without a handler: (count, last one's number).
pub fn unexpected() -> (u32, u32) {
    (UNEXPECTED.load(Ordering::Relaxed), LAST_UNEXPECTED.load(Ordering::Relaxed))
}

static SGI_ARRIVED: AtomicBool = AtomicBool::new(false);

/// The SGI the self-test sends.
const TEST_SGI: u32 = 0;

/// With interrupts unmasked: send this core an SGI and wait for its handler,
/// then check that holding an `IrqsOff` lock masks interrupts and releasing
/// it unmasks them again.
pub fn self_test(out: &mut impl Write) -> bool {
    set_handler(TEST_SGI, || SGI_ARRIVED.store(true, Ordering::Relaxed));
    gic::enable(TEST_SGI);
    gic::send_sgi_to_self(TEST_SGI);
    // It should arrive at once; give it a million tries.
    let mut tries = 0;
    while !SGI_ARRIVED.load(Ordering::Relaxed) && tries < 1_000_000 {
        core::hint::spin_loop();
        tries += 1;
    }
    let delivered = SGI_ARRIVED.load(Ordering::Relaxed);

    let masked = || {
        let daif: u64;
        // SAFETY: reading DAIF has no side effects.
        unsafe { core::arch::asm!("mrs {}, DAIF", out(reg) daif) };
        daif & (1 << 7) != 0
    };
    let lock = SpinLock::<(), IrqsOff>::new(());
    let guard = lock.lock();
    let while_held = masked();
    drop(guard);
    let locks_ok = while_held && !masked();

    let _ = writeln!(out, "  interrupts self-test: SGI {TEST_SGI} sent to this core {}; IrqsOff locks {}",
        if delivered { "and handled" } else { "but NEVER handled" },
        if locks_ok { "mask interrupts while held" } else { "do NOT mask interrupts correctly" });
    delivered && locks_ok
}
