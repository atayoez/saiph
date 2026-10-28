//! PSCI, the firmware interface for powering cores and the machine on/off.

#[derive(Clone, Copy)]
pub enum Conduit {
    /// To the secure firmware at EL3 (bare metal, QEMU TCG).
    Smc,
    /// To the hypervisor (a KVM guest).
    Hvc,
}

const SYSTEM_OFF: u64 = 0x8400_0008;
const SYSTEM_RESET: u64 = 0x8400_0009;

/// Power the machine off. Does not return; if the firmware refuses, the
/// core stops instead.
pub fn system_off(conduit: Conduit) -> ! {
    call_no_return(conduit, SYSTEM_OFF)
}

/// Restart the machine (a cold reset: the firmware starts from scratch).
/// Does not return; if the firmware refuses, the core stops instead.
pub fn system_reset(conduit: Conduit) -> ! {
    call_no_return(conduit, SYSTEM_RESET)
}

fn call_no_return(conduit: Conduit, function: u64) -> ! {
    // SAFETY: SYSTEM_OFF and SYSTEM_RESET take no arguments and do not
    // return on success.
    unsafe {
        match conduit {
            Conduit::Smc => core::arch::asm!("smc #0", inout("x0") function => _, out("x1") _, out("x2") _, out("x3") _),
            Conduit::Hvc => core::arch::asm!("hvc #0", inout("x0") function => _, out("x1") _, out("x2") _, out("x3") _),
        }
    }
    loop {
        // SAFETY: wfe has no effect besides waiting.
        unsafe { core::arch::asm!("wfe") };
    }
}
