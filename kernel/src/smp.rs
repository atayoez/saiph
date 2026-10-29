//! The other cores, started through PSCI CPU_ON.
//!
//! The firmware wakes a core at a physical address with the MMU and caches
//! off, at this core's exception level, and nothing else set up. The entry
//! code below gives it the boot core's settings, read from the boot core's
//! own registers into a `Boot` block: EL2 with VHE, the same page tables,
//! exception vectors and PAC key, its own stacks and per-core variables.
//! It switches the MMU on while running at its physical address, mapped
//! 1:1 in the lower half, and jumps up into the kernel (`secondary_main`).
//! There the core sets up its part of the GIC and its timer, reports in and
//! waits in `wfi`. There is no scheduler yet to give it work.

use crate::console::Console;
use crate::{gic, irq, memory, mmu, percpu, psci, stacks, timer, vm};
use core::fmt::Write;
use core::mem::offset_of;
use core::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};

/// What a starting core reads before its MMU is on, at a physical address.
#[repr(C)]
struct Boot {
    hcr: u64,
    cnthctl: u64,
    mair: u64,
    tcr: u64,
    ttbr0: u64,
    ttbr1: u64,
    vbar: u64,
    tpidr: u64,
    key: [u64; 2],
    sctlr: u64,
    kernel_sp: u64,
    exception_sp: u64,
    entry: u64,
    core: u64,
}

// The entry point, in a page of its own, so that page alone needs to be
// mapped 1:1. x0 = the physical address of this core's `Boot`.
core::arch::global_asm!(
    ".section .text.smp_entry, \"ax\"",
    ".balign 4096",
    ".global saiph_secondary_entry",
    "saiph_secondary_entry:",
    // At EL2: hypervisor configuration as on the boot core (VHE: E2H, TGE),
    // which makes the _EL1 register names below reach the EL2 registers.
    "mrs x1, CurrentEL",
    "cmp x1, #(2 << 2)",
    "b.ne 1f",
    "ldr x2, [x0, #{hcr}]",
    "msr HCR_EL2, x2",
    "ldr x2, [x0, #{cnthctl}]",
    "msr CNTHCTL_EL2, x2",
    "msr CNTVOFF_EL2, xzr",
    "isb",
    "1:",
    "ldr x2, [x0, #{mair}]",
    "msr MAIR_EL1, x2",
    "ldr x2, [x0, #{tcr}]",
    "msr TCR_EL1, x2",
    "ldr x2, [x0, #{ttbr0}]",
    "msr TTBR0_EL1, x2",
    "ldr x2, [x0, #{ttbr1}]",
    "msr TTBR1_EL1, x2",
    "ldr x2, [x0, #{vbar}]",
    "msr VBAR_EL1, x2",
    "ldr x2, [x0, #{tpidr}]",
    "msr TPIDR_EL1, x2",
    "ldp x2, x3, [x0, #{key}]",
    "msr S3_0_C2_C1_0, x2", // APIAKeyLo_EL1
    "msr S3_0_C2_C1_1, x3", // APIAKeyHi_EL1
    // Everything else the block holds, before the MMU hides it.
    "ldr x4, [x0, #{sctlr}]",
    "ldr x5, [x0, #{kernel_sp}]",
    "ldr x6, [x0, #{exception_sp}]",
    "ldr x7, [x0, #{entry}]",
    "ldr x8, [x0, #{core}]",
    "tlbi vmalle1",
    "ic iallu",
    "dsb nsh",
    "isb",
    "msr SCTLR_EL1, x4", // MMU, caches, PAC and BTI on
    "isb",
    "msr SPSel, #1",
    "mov sp, x6",        // the exception stack
    "msr SP_EL0, x5",    // the kernel stack
    "msr SPSel, #0",
    "mov x0, x8",
    "blr x7",            // secondary_main(core), up in the kernel
    "2: wfe",
    "b 2b",
    hcr = const offset_of!(Boot, hcr),
    cnthctl = const offset_of!(Boot, cnthctl),
    mair = const offset_of!(Boot, mair),
    tcr = const offset_of!(Boot, tcr),
    ttbr0 = const offset_of!(Boot, ttbr0),
    ttbr1 = const offset_of!(Boot, ttbr1),
    vbar = const offset_of!(Boot, vbar),
    tpidr = const offset_of!(Boot, tpidr),
    key = const offset_of!(Boot, key),
    sctlr = const offset_of!(Boot, sctlr),
    kernel_sp = const offset_of!(Boot, kernel_sp),
    exception_sp = const offset_of!(Boot, exception_sp),
    entry = const offset_of!(Boot, entry),
    core = const offset_of!(Boot, core),
);

unsafe extern "C" {
    fn saiph_secondary_entry();
}

/// The SGI the boot core pings the others with.
const PING: u32 = 1;

crate::cpu_local! {
    /// This core's number: 0 for the boot core, 1.. for the others.
    static CORE: AtomicUsize = AtomicUsize::new(0);
}

/// Per core: running (set by the core itself), its MIDR, and whether it
/// answered the ping.
static RUNNING: [AtomicBool; stacks::MAX_CORES] = [const { AtomicBool::new(false) }; stacks::MAX_CORES];
static MIDR: [AtomicU64; stacks::MAX_CORES] = [const { AtomicU64::new(0) }; stacks::MAX_CORES];
static PINGED: [AtomicBool; stacks::MAX_CORES] = [const { AtomicBool::new(false) }; stacks::MAX_CORES];
/// Per core: its own timer woke it from a 10 ms sleep.
static TIMER_OK: [AtomicBool; stacks::MAX_CORES] = [const { AtomicBool::new(false) }; stacks::MAX_CORES];

fn read_midr() -> u64 {
    let midr: u64;
    // SAFETY: reading MIDR has no side effects.
    unsafe { core::arch::asm!("mrs {}, MIDR_EL1", out(reg) midr) };
    midr
}

/// The core type, from MIDR_EL1 (the way FINDINGS.md says to tell them apart).
fn core_name(midr: u64) -> &'static str {
    match (midr >> 24 & 0xff, midr >> 4 & 0xfff) {
        (0x41, 0xd81) => "Cortex-A720",
        (0x41, 0xd80) => "Cortex-A520",
        (0x41, _) => "an Arm core",
        _ => "a core",
    }
}

/// SGI `PING` arrived: note it for this core.
fn pinged() {
    PINGED[CORE.get().load(Ordering::Relaxed)].store(true, Ordering::Release);
}

/// The kernel side of a new core's start.
extern "C" fn secondary_main(core: usize) -> ! {
    CORE.get().store(core, Ordering::Relaxed);
    vm::disable_lower_half(); // only the entry page used it
    let midr = read_midr();
    MIDR[core].store(midr, Ordering::Relaxed);
    match gic::init_this_core() {
        Ok(_) => {
            gic::enable(PING);
            timer::init_this_core();
            irq::unmask();
            TIMER_OK[core].store(timer::self_test().0, Ordering::Relaxed);
        }
        Err(e) => {
            let _ = writeln!(Console, "  core {core}: setting up its part of the GIC failed: {e}");
        }
    }
    RUNNING[core].store(true, Ordering::Release);
    loop {
        // SAFETY: waits for an interrupt; changes nothing.
        unsafe { core::arch::asm!("wfi", options(nostack)) };
    }
}

/// Write a block of memory out of the caches to RAM, for a core that reads
/// it with its caches still off.
fn clean_to_ram(start: u64, len: u64) {
    for line in (start & !63..start + len).step_by(64) {
        // SAFETY: cleaning a cache line changes no data.
        unsafe { core::arch::asm!("dc cvac, {}", in(reg) line) };
    }
    // SAFETY: a barrier.
    unsafe { core::arch::asm!("dsb sy") };
}

/// Start every core in `mpidrs` other than this one, one after the other,
/// then ping each. `kernel_phys` translates kernel addresses. Returns how
/// many cores run, this one included.
pub fn start_all(out: &mut impl Write, mpidrs: &[u64], conduit: psci::Conduit, hhdm: u64, kernel_phys: impl Fn(u64) -> u64) -> usize {
    let entry_virt = saiph_secondary_entry as *const () as u64;
    let entry = kernel_phys(entry_virt);
    if !mmu::map_identity_code(entry, &kernel_phys) {
        let _ = writeln!(out, "  cores: could not map the entry code; staying on one core");
        return 1;
    }
    clean_to_ram(entry_virt, 4096); // it runs with the caches off at first

    // The boot core's settings, which every core shares.
    let el2 = (mmu::read_current_el() >> 2) & 3 == 2;
    let (mut hcr, mut cnthctl) = (0, 0);
    let (mair, tcr, ttbr0, ttbr1, vbar, sctlr, key_lo, key_hi): (u64, u64, u64, u64, u64, u64, u64, u64);
    // SAFETY: reads system registers only.
    unsafe {
        if el2 {
            core::arch::asm!("mrs {}, HCR_EL2", "mrs {}, CNTHCTL_EL2", out(reg) hcr, out(reg) cnthctl);
        }
        core::arch::asm!("mrs {}, MAIR_EL1", "mrs {}, TCR_EL1", "mrs {}, TTBR0_EL1", "mrs {}, TTBR1_EL1",
            out(reg) mair, out(reg) tcr, out(reg) ttbr0, out(reg) ttbr1);
        core::arch::asm!("mrs {}, VBAR_EL1", "mrs {}, SCTLR_EL1", "mrs {}, S3_0_C2_C1_0", "mrs {}, S3_0_C2_C1_1",
            out(reg) vbar, out(reg) sctlr, out(reg) key_lo, out(reg) key_hi);
    }

    irq::set_handler(PING, pinged);
    gic::enable(PING);
    MIDR[0].store(read_midr(), Ordering::Relaxed);
    RUNNING[0].store(true, Ordering::Relaxed);

    let mut me: u64;
    // SAFETY: reading MPIDR has no side effects.
    unsafe { core::arch::asm!("mrs {}, MPIDR_EL1", out(reg) me) };
    me &= 0xff_00ff_ffff;

    // (number, MPIDR) of every running core; numbers are never reused, even
    // for a core that did not start.
    let mut running = [(0usize, 0u64); stacks::MAX_CORES];
    running[0] = (0, me);
    let mut cores = 1;
    for (core, &mpidr) in (1..stacks::MAX_CORES).zip(mpidrs.iter().filter(|&&m| m != me)) {
        let stacks = match stacks::allocate(core) {
            Ok(s) => s,
            Err(e) => {
                let _ = writeln!(out, "  core {core} (MPIDR {mpidr:#x}): no stacks ({e}); not started");
                continue;
            }
        };
        let Some(frame) = memory::alloc_frame() else {
            let _ = writeln!(out, "  core {core} (MPIDR {mpidr:#x}): out of memory; not started");
            break;
        };
        let block = (frame + hhdm) as *mut Boot;
        let boot = Boot {
            hcr,
            cnthctl,
            mair,
            tcr: tcr & !(1 << 7), // EPD0 clear: the entry page is in the lower half
            ttbr0,
            ttbr1,
            vbar,
            tpidr: percpu::new_area() as u64,
            key: [key_lo, key_hi],
            sctlr,
            kernel_sp: stacks.kernel_top,
            exception_sp: stacks.exception_top,
            entry: secondary_main as *const () as u64,
            core: core as u64,
        };
        // SAFETY: a fresh frame, reached through the direct map.
        unsafe { block.write(boot) };
        clean_to_ram(block as u64, size_of::<Boot>() as u64);

        let result = psci::cpu_on(conduit, mpidr, entry, frame);
        let started = result == 0 && {
            // Give it up to a second to report in.
            let deadline = timer::now() + timer::frequency();
            while !RUNNING[core].load(Ordering::Acquire) && timer::now() < deadline {
                timer::sleep(timer::frequency() / 1000);
            }
            RUNNING[core].load(Ordering::Acquire)
        };
        if started {
            memory::free_frame(frame);
            running[cores] = (core, mpidr);
            cores += 1;
        } else {
            let _ = writeln!(out, "  core {core} (MPIDR {mpidr:#x}) did not start (PSCI result {result})");
            // Its stacks and block stay reserved: it might still wake up.
        }
    }

    // Ping every core that runs and wait for the answers.
    for &(core, mpidr) in &running[..cores] {
        gic::send_sgi(mpidr, PING);
        let deadline = timer::now() + timer::frequency() / 10;
        while !PINGED[core].load(Ordering::Acquire) && timer::now() < deadline {
            timer::sleep(timer::frequency() / 10_000);
        }
    }
    let answered = running[..cores].iter().filter(|&&(c, _)| PINGED[c].load(Ordering::Acquire)).count();
    let timers = running[1..cores].iter().filter(|&&(c, _)| TIMER_OK[c].load(Ordering::Relaxed)).count();

    let (mut a720, mut a520) = (0, 0);
    for &(core, _) in &running[..cores] {
        let midr = MIDR[core].load(Ordering::Relaxed);
        match core_name(midr) {
            "Cortex-A720" => a720 += 1,
            "Cortex-A520" => a520 += 1,
            _ => {}
        }
    }
    let _ = write!(out, "  cores: {cores} of {} running", mpidrs.len().max(1));
    if a720 + a520 > 0 {
        let _ = write!(out, " ({a720} Cortex-A720, {a520} Cortex-A520)");
    } else {
        let _ = write!(out, " (MIDR {:#x})", MIDR[0].load(Ordering::Relaxed));
    }
    let _ = writeln!(out, "; {answered} answered an interrupt from the boot core, {timers} of the {} others were woken by their own timer",
        cores - 1);
    cores
}
