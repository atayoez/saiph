//! The Arm generic timer: a counter in every core that has been counting up
//! at a fixed rate since power-on. CNTFRQ_EL0 holds that rate in ticks per
//! second; CNTVCT_EL0 is the current count.

pub fn frequency() -> u64 {
    let f: u64;
    // SAFETY: reading CNTFRQ_EL0 has no side effects.
    unsafe { core::arch::asm!("mrs {}, CNTFRQ_EL0", out(reg) f) };
    f
}

fn count() -> u64 {
    let c: u64;
    // SAFETY: reading the counter has no side effects; the isb stops the
    // core from reading it early, ahead of the instructions before it.
    unsafe { core::arch::asm!("isb", "mrs {}, CNTVCT_EL0", out(reg) c) };
    c
}

/// Busy-wait for `secs` seconds.
pub fn wait_seconds(secs: u64) {
    let (start, ticks) = (count(), secs * frequency());
    while count() - start < ticks {
        core::hint::spin_loop();
    }
}
