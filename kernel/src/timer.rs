//! The Arm generic timer: a counter in every core that has been counting up
//! at a fixed rate since power-on (CNTFRQ_EL0 ticks per second; 1 GHz on
//! the O6), plus comparators that raise an interrupt when the count passes
//! a value.
//!
//! Saiph uses the virtual timer (CNTV_*). At EL2 with VHE those register
//! names reach the EL2 virtual timer (interrupt 28 on the O6), under KVM at
//! EL1 the EL1 one (27); the GTDT says which number. Linux defaults to the
//! same timer since 2026.
//!
//! It is tickless: the timer is set only for the next deadline, and the
//! core sleeps (`wfi`) until then. Nothing wakes it up periodically.

use crate::{gic, irq, sync};
use core::sync::atomic::{AtomicBool, AtomicU32, Ordering};

/// CNTV_CTL bit 0: the timer is on (bit 1 would mask its interrupt).
const ENABLE: u64 = 1;

/// The timer's interrupt, once `init` has run (0 = none: busy-wait instead).
static INTID: AtomicU32 = AtomicU32::new(0);
/// The CPU has FEAT_ECV, so the counter can be read without a barrier.
static ECV: AtomicBool = AtomicBool::new(false);
/// How often the timer's interrupt has arrived.
static FIRED: AtomicU32 = AtomicU32::new(0);

pub fn frequency() -> u64 {
    let f: u64;
    // SAFETY: reading CNTFRQ_EL0 has no side effects.
    unsafe { core::arch::asm!("mrs {}, CNTFRQ_EL0", out(reg) f) };
    f
}

/// The current count.
pub fn now() -> u64 {
    let c: u64;
    if ECV.load(Ordering::Relaxed) {
        // SAFETY: CNTVCTSS_EL0 ("self-synchronised") cannot be read early,
        // ahead of the instructions before it, so it needs no barrier.
        unsafe { core::arch::asm!("mrs {}, S3_3_C14_C0_6", out(reg) c) };
    } else {
        // SAFETY: the isb stops the core from reading the counter early.
        unsafe { core::arch::asm!("isb", "mrs {}, CNTVCT_EL0", out(reg) c) };
    }
    c
}

/// The interrupt handler: the deadline has passed. Switch the timer off;
/// its interrupt is level-triggered and would otherwise keep coming.
fn fired() {
    // SAFETY: turns this core's virtual timer off.
    unsafe { core::arch::asm!("msr CNTV_CTL_EL0, xzr", "isb") };
    FIRED.fetch_add(1, Ordering::Relaxed);
}

/// Take the timer's interrupt `intid` from now on.
pub fn init(intid: u32, trigger: gic::Trigger) {
    let mmfr0: u64;
    // SAFETY: reading an ID register has no side effects.
    unsafe { core::arch::asm!("mrs {}, ID_AA64MMFR0_EL1", out(reg) mmfr0) };
    ECV.store(mmfr0 >> 60 != 0, Ordering::Relaxed);
    // SAFETY: the timer starts switched off.
    unsafe { core::arch::asm!("msr CNTV_CTL_EL0, xzr", "isb") };
    irq::set_handler(intid, fired);
    gic::configure(intid, trigger);
    gic::enable(intid);
    INTID.store(intid, Ordering::Relaxed);
}

/// The counter is read through CNTVCTSS_EL0 (FEAT_ECV).
pub fn self_synchronised() -> bool {
    ECV.load(Ordering::Relaxed)
}

/// How often the timer's interrupt has arrived so far.
pub fn interrupts() -> u32 {
    FIRED.load(Ordering::Relaxed)
}

/// Sleep until the counter reaches `deadline`.
pub fn sleep_until(deadline: u64) {
    if INTID.load(Ordering::Relaxed) == 0 {
        while now() < deadline {
            core::hint::spin_loop();
        }
        return;
    }
    // SAFETY: sets this core's virtual timer to fire at `deadline`.
    unsafe { core::arch::asm!("msr CNTV_CVAL_EL0, {}", "msr CNTV_CTL_EL0, {}", "isb", in(reg) deadline, in(reg) ENABLE) };
    loop {
        // Interrupts are masked around the check, so the timer's interrupt
        // cannot slip in between "not yet" and `wfi` and leave the core
        // asleep for good: `wfi` still wakes for a pending interrupt, and
        // the handler runs as soon as the mask is lifted.
        sync::irqs::mask();
        if now() >= deadline {
            sync::irqs::unmask();
            return;
        }
        // SAFETY: waits for an interrupt; changes nothing.
        unsafe { core::arch::asm!("wfi", options(nostack)) };
        sync::irqs::unmask();
    }
}

/// Sleep for `ticks` counter ticks.
pub fn sleep(ticks: u64) {
    sleep_until(now() + ticks);
}

/// Sleep 10 ms and check that the timer's interrupt is what woke the core.
pub fn self_test() -> (bool, u64) {
    let before = interrupts();
    let start = now();
    sleep(frequency() / 100);
    let took = now() - start;
    (interrupts() == before + 1 && took >= frequency() / 100, took * 1_000_000 / frequency())
}
