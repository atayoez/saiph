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
const CPU_ON: u64 = 0xc400_0003;

/// Power the machine off. Returns only if the firmware refuses, with its
/// PSCI error code (negative, e.g. -1 = not supported).
pub fn system_off(conduit: Conduit) -> i64 {
    call(conduit, SYSTEM_OFF, [0; 3])
}

/// Restart the machine (a cold reset: the firmware starts from scratch).
/// Returns only if the firmware refuses, with its PSCI error code.
pub fn system_reset(conduit: Conduit) -> i64 {
    call(conduit, SYSTEM_RESET, [0; 3])
}

/// Start the core whose MPIDR affinity is `target`: the firmware powers it
/// up and jumps to physical address `entry` with the MMU off, at this
/// core's exception level, with `context` in x0. 0 = started; negative =
/// PSCI error (-4: already on).
pub fn cpu_on(conduit: Conduit, target: u64, entry: u64, context: u64) -> i64 {
    call(conduit, CPU_ON, [target, entry, context])
}

fn call(conduit: Conduit, function: u64, args: [u64; 3]) -> i64 {
    let result: u64;
    // SAFETY: a PSCI call per the SMC Calling Convention: function in x0,
    // arguments in x1..x3, result in x0; x1..x3 may be overwritten.
    unsafe {
        match conduit {
            Conduit::Smc => core::arch::asm!("smc #0", inout("x0") function => result,
                inout("x1") args[0] => _, inout("x2") args[1] => _, inout("x3") args[2] => _),
            Conduit::Hvc => core::arch::asm!("hvc #0", inout("x0") function => result,
                inout("x1") args[0] => _, inout("x2") args[1] => _, inout("x3") args[2] => _),
        }
    }
    result as i64
}
