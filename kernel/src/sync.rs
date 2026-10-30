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
//! And each lock's type names its place in the order locks must be taken
//! in (its `Level`, see `order`): a core holding a lock may only take locks
//! that come after it. Two cores taking two locks in opposite orders is how
//! a deadlock happens, so every lock checks, each time it is taken, that
//! this core holds no lock at its level or after it, and panics with both
//! names if it does, the first time the wrong order runs, whether or not
//! another core happens to collide (Linux's lockdep checks the same way).
//! Taking a lock twice on one core is caught too, instead of hanging.
//! A fully compile-time check (as in Fuchsia's netstack3) would need a
//! token passed into every function that locks, and Rust's allocator
//! interface, through which the heap's lock is taken, has no room for one;
//! the orders the code relies on are checked at compile time with
//! `comes_before` instead.
//!
//! The locks are `lock_api` 0.5 mutexes: `lock()` returns a guard that
//! unlocks when dropped and cannot be moved to another core.

use core::cell::UnsafeCell;
use core::marker::PhantomData;
use core::mem::MaybeUninit;
use core::sync::atomic::{AtomicBool, AtomicU8, AtomicU32, AtomicU64, Ordering};

/// A lock's place in the order locks must be taken in.
pub trait Level {
    const RANK: u32;
    const NAME: &'static str;
}

macro_rules! levels {
    ($($(#[$doc:meta])* $name:ident = $rank:literal;)*) => {
        $(
            $(#[$doc])*
            pub struct $name;
            impl super::Level for $name {
                const RANK: u32 = $rank;
                const NAME: &'static str = stringify!($name);
            }
        )*

        /// The name of the level with this rank.
        pub fn name(rank: u32) -> &'static str {
            match rank {
                $($rank => stringify!($name),)*
                _ => "?",
            }
        }
    };
}

/// Every lock's level, in the order they may be taken: a core holding a
/// lock may only take locks further down this list.
pub mod order {
    levels! {
        /// A core's run queue. It may grow (allocate) while locked, so it
        /// comes before the heap.
        RunQueue = 1;
        /// The kernel heap. Growing it maps pages.
        Heap = 2;
        /// The page tables the kernel runs on.
        PageTables = 3;
        /// Calls into the UEFI firmware.
        Firmware = 4;
        /// The GIC's configuration registers.
        GicConfig = 5;
        /// Locks that only self-tests use.
        Test = 6;
        /// The console comes last, so anything may print while holding a lock.
        Console = 7;
    }
}

/// Fails to compile unless level `A` comes before level `B`. For code that
/// takes a `B` lock while holding an `A` lock: `const _: () = comes_before::<A, B>();`
pub const fn comes_before<A: Level, B: Level>() {
    assert!(A::RANK < B::RANK, "lock order: this code takes these two locks in the wrong order");
}

crate::cpu_local! {
    /// The levels of the locks this core holds, one bit per rank.
    static HELD: AtomicU64 = AtomicU64::new(0);
}

/// Before taking a lock of level `L`: panic if this core holds a lock at
/// `L`'s level or after it.
fn check_order<L: Level>() {
    let held = HELD.get().load(Ordering::Relaxed);
    if held >> L::RANK != 0 {
        let latest = 63 - held.leading_zeros();
        panic!("lock order: taking a {} lock while holding a {} lock (see sync::order)", L::NAME, order::name(latest));
    }
}

/// Entering an interrupt handler: it starts with no locks held (it is a
/// context of its own; only `IrqsOn` locks can be held by the code it
/// interrupted, and handlers never take those). Returns what to restore.
pub fn enter_handler() -> u64 {
    HELD.get().swap(0, Ordering::Relaxed)
}

/// Leaving an interrupt handler: back to the interrupted code's locks.
pub fn leave_handler(held: u64) {
    HELD.get().store(held, Ordering::Relaxed);
}

/// This core holds a lock (of any kind).
pub fn holding_locks() -> bool {
    HELD.get().load(Ordering::Relaxed) != 0
}

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
/// is held, `L` where it stands in the lock order.
pub type SpinLock<T, G, L> = lock_api::Mutex<TicketLock<G, L>, T>;

pub struct TicketLock<G, L> {
    /// The next ticket to hand out.
    next: AtomicU32,
    /// The ticket whose holder may have the lock.
    serving: AtomicU32,
    guardian: PhantomData<(G, L)>,
}

/// Holding a `TicketLock`. Not `Send`: the interrupt state it goes with is
/// this core's.
pub struct Held(PhantomData<*mut ()>);

// SAFETY: one ticket is served at a time; taking it is an acquire and
// handing on the next one a release, so the holder sees everything the
// previous holder wrote.
unsafe impl<G: Guardian, L: Level> lock_api::RawMutex for TicketLock<G, L> {
    const INIT: Self = TicketLock { next: AtomicU32::new(0), serving: AtomicU32::new(0), guardian: PhantomData };
    type Guard = Held;

    fn lock(&self) -> Held {
        G::enter();
        check_order::<L>();
        let ticket = self.next.fetch_add(1, Ordering::Relaxed);
        loop {
            let now = self.serving.load(Ordering::Acquire);
            if now == ticket {
                HELD.get().fetch_or(1 << L::RANK, Ordering::Relaxed);
                return Held(PhantomData);
            }
            wait_for_change(&self.serving, now);
        }
    }

    fn try_lock(&self) -> Option<Held> {
        G::enter();
        // Draw a ticket only if it would be served right away.
        let now = self.serving.load(Ordering::Acquire);
        // (No order check: a try never waits, so it cannot deadlock.)
        if self.next.compare_exchange(now, now.wrapping_add(1), Ordering::Relaxed, Ordering::Relaxed).is_ok() {
            HELD.get().fetch_or(1 << L::RANK, Ordering::Relaxed);
            return Some(Held(PhantomData));
        }
        G::exit();
        None
    }

    unsafe fn unlock(&self, _held: Held) {
        HELD.get().fetch_and(!(1 << L::RANK), Ordering::Relaxed);
        self.serving.fetch_add(1, Ordering::Release);
        G::exit();
    }

    fn is_locked(&self) -> bool {
        self.next.load(Ordering::Relaxed) != self.serving.load(Ordering::Relaxed)
    }
}

impl<G: Guardian, L: Level> TicketLock<G, L> {
    pub const fn new() -> Self {
        <Self as lock_api::RawMutex>::INIT
    }

    /// Take the lock without a guard, for a lock that one thread takes
    /// and another releases (the scheduler's, across a thread switch).
    pub fn raw_lock(&self) {
        // The guard state carries nothing; `raw_unlock` makes a new one.
        let _held = <Self as lock_api::RawMutex>::lock(self);
    }

    /// Release a lock taken with `raw_lock`, on the same core.
    ///
    /// # Safety
    /// This core must hold the lock.
    pub unsafe fn raw_unlock(&self) {
        // SAFETY: the caller holds the lock.
        unsafe { <Self as lock_api::RawMutex>::unlock(self, Held(PhantomData)) }
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

    /// How many `mask` calls are not yet undone on this core.
    pub fn depth() -> u32 {
        DEPTH.get().load(Ordering::Relaxed)
    }

    pub fn unmask() {
        if DEPTH.get().fetch_sub(1, Ordering::Relaxed) == 1 && WERE_ON.get().load(Ordering::Relaxed) {
            // SAFETY: unmasks IRQs, as they were before the first lock. The
            // isb makes a pending interrupt arrive right here.
            unsafe { core::arch::asm!("msr DAIFClr, #2", "isb", options(nostack)) };
        }
    }
}

/// A value that is set once and then read by every core without a lock.
/// Setting it publishes the value with a release store; `get` sees it
/// through an acquire load, so a reader gets either nothing or all of it.
pub struct Once<T> {
    state: AtomicU8,
    value: UnsafeCell<MaybeUninit<T>>,
}

const EMPTY: u8 = 0;
const WRITING: u8 = 1;
const READY: u8 = 2;

// SAFETY: the value is written once, before READY is published, and only
// read (shared) after that.
unsafe impl<T: Send + Sync> Sync for Once<T> {}

impl<T> Once<T> {
    pub const fn new() -> Self {
        Once { state: AtomicU8::new(EMPTY), value: UnsafeCell::new(MaybeUninit::uninit()) }
    }

    /// Set the value. Gives it back if a value was set (or is being set).
    pub fn set(&self, value: T) -> Result<(), T> {
        if self.state.compare_exchange(EMPTY, WRITING, Ordering::Acquire, Ordering::Relaxed).is_err() {
            return Err(value);
        }
        // SAFETY: winning the exchange makes this the only writer, and no
        // reader looks before READY.
        unsafe { (*self.value.get()).write(value) };
        self.state.store(READY, Ordering::Release);
        Ok(())
    }

    /// The value, once it is set.
    pub fn get(&self) -> Option<&T> {
        // SAFETY: READY means the value was written, and it never changes again.
        (self.state.load(Ordering::Acquire) == READY).then(|| unsafe { (*self.value.get()).assume_init_ref() })
    }
}
