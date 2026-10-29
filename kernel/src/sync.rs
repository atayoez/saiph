//! Locks.
//!
//! A spin lock makes a core wait until the core holding it lets go. Saiph's
//! is a ticket lock, like the numbered tickets at a counter: each core that
//! wants the lock draws the next number (`next`) and waits until "now
//! serving" (`serving`) shows it. Cores get the lock in the order they
//! asked, so the O6's slower A520 cores cannot be starved by the A720s, as
//! with a plain test-and-set lock. A waiting core sleeps in `wfe` until the
//! lock word changes, instead of reading it over and over.
//!
//! Each lock's type also says what happens to interrupts while it is held
//! (its `Guardian`, an idea from Asterinas). If an interrupt handler wanted
//! a lock its own core already holds, that core would wait for itself
//! forever; a lock that handlers use is therefore an `IrqsOff` lock, and
//! there is no way to take it with interrupts on.
//!
//! The locks are `lock_api` 0.5 mutexes: `lock()` returns a guard that
//! unlocks when dropped and cannot be moved to another core.

use core::marker::PhantomData;
use core::sync::atomic::{AtomicBool, AtomicU32, Ordering};

/// What a lock does to interrupts while it is held.
pub trait Guardian {
    /// Before the lock is taken.
    fn enter();
    /// After the lock is released.
    fn exit();
}

/// Interrupts are masked while the lock is held.
pub struct IrqsOff;

impl Guardian for IrqsOff {
    fn enter() {
        irqs::mask();
    }
    fn exit() {
        irqs::unmask();
    }
}

/// Interrupts stay as they are. Only for locks that interrupt handlers
/// never take: taking one inside a handler panics, with a message, instead
/// of perhaps waiting for itself forever.
pub struct IrqsOn;

impl Guardian for IrqsOn {
    fn enter() {
        assert!(!crate::irq::in_handler(), "an IrqsOn lock was taken in an interrupt handler");
    }
    fn exit() {}
}

/// A spin lock around a `T`; `G` says what happens to interrupts while it
/// is held.
pub type SpinLock<T, G> = lock_api::Mutex<TicketLock<G>, T>;

pub struct TicketLock<G> {
    /// The next ticket to hand out.
    next: AtomicU32,
    /// The ticket whose holder may have the lock.
    serving: AtomicU32,
    guardian: PhantomData<G>,
}

/// Holding a `TicketLock`. Not `Send`: the interrupt state it goes with is
/// this core's.
pub struct Held(PhantomData<*mut ()>);

// SAFETY: one ticket is served at a time; taking it is an acquire and
// handing on the next one a release, so the holder sees everything the
// previous holder wrote.
unsafe impl<G: Guardian> lock_api::RawMutex for TicketLock<G> {
    const INIT: Self = TicketLock { next: AtomicU32::new(0), serving: AtomicU32::new(0), guardian: PhantomData };
    type Guard = Held;

    fn lock(&self) -> Held {
        G::enter();
        let ticket = self.next.fetch_add(1, Ordering::Relaxed);
        loop {
            let now = self.serving.load(Ordering::Acquire);
            if now == ticket {
                return Held(PhantomData);
            }
            wait_for_change(&self.serving, now);
        }
    }

    fn try_lock(&self) -> Option<Held> {
        G::enter();
        // Draw a ticket only if it would be served right away.
        let now = self.serving.load(Ordering::Acquire);
        if self.next.compare_exchange(now, now.wrapping_add(1), Ordering::Relaxed, Ordering::Relaxed).is_ok() {
            return Some(Held(PhantomData));
        }
        G::exit();
        None
    }

    unsafe fn unlock(&self, _held: Held) {
        self.serving.fetch_add(1, Ordering::Release);
        G::exit();
    }

    fn is_locked(&self) -> bool {
        self.next.load(Ordering::Relaxed) != self.serving.load(Ordering::Relaxed)
    }
}

/// Sleep until `word` may have changed from `old`, the way Linux waits on
/// arm64: `ldxr` puts the word in this core's exclusive monitor, and when
/// another core writes it, the monitor clears and wakes `wfe`. Other events
/// and interrupts wake it too, so the caller checks again.
fn wait_for_change(word: &AtomicU32, old: u32) {
    // SAFETY: only reads `word`, which is a valid atomic.
    unsafe {
        core::arch::asm!(
            "sevl",
            "wfe",              // clear the event register (sevl just set it)
            "ldxr {v:w}, [{p}]",
            "cmp {v:w}, {old:w}",
            "b.ne 2f",
            "wfe",
            "2:",
            p = in(reg) word.as_ptr(),
            old = in(reg) old,
            v = out(reg) _,
            options(nostack),
        )
    };
}

/// Masking interrupts, nested: they come back on when the last lock is
/// released, and only if they were on before the first one was taken, in
/// whatever order the locks are released.
///
/// Each core keeps its own count.
pub mod irqs {
    use super::*;

    crate::cpu_local! {
        static DEPTH: AtomicU32 = AtomicU32::new(0);
        static WERE_ON: AtomicBool = AtomicBool::new(false);
    }

    pub fn mask() {
        let daif: u64;
        // SAFETY: masks IRQs (DAIF.I, bit 7) on this core; the old state is kept.
        unsafe { core::arch::asm!("mrs {}, DAIF", "msr DAIFSet, #2", out(reg) daif, options(nostack)) };
        // Interrupts are off now, so nothing else runs on this core in between.
        if DEPTH.get().fetch_add(1, Ordering::Relaxed) == 0 {
            WERE_ON.get().store(daif & (1 << 7) == 0, Ordering::Relaxed);
        }
    }

    pub fn unmask() {
        if DEPTH.get().fetch_sub(1, Ordering::Relaxed) == 1 && WERE_ON.get().load(Ordering::Relaxed) {
            // SAFETY: unmasks IRQs, as they were before the first lock. The
            // isb makes a pending interrupt arrive right here.
            unsafe { core::arch::asm!("msr DAIFClr, #2", "isb", options(nostack)) };
        }
    }
}
