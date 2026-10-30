//! The Saiph kernel, early days: Limine loads it; it sets up its consoles
//! (screen and serial), exceptions, PAC and BTI, physical memory, its own
//! page tables, a heap, interrupts and the timer, then powers the machine
//! off, or reboots it after a countdown when the command line says
//! `reboot-after=<seconds>` (as on the board).

#![no_std]
#![no_main]

extern crate alloc;

mod acpi;
mod console;
mod efi;
mod exceptions;
mod gic;
mod heap;
mod irq;
mod memory;
mod mmu;
mod percpu;
mod protect;
mod psci;
mod screen;
mod smp;
mod stacks;
mod sync;
mod timer;
mod uart;
mod vm;

use console::Console;
use core::fmt::Write;
use limine::request::{ExecutableAddressRequest, ExecutableCmdlineRequest, HhdmRequest, RsdpRequest};
use limine::{BaseRevision, RequestsEndMarker, RequestsStartMarker};

// Requests: Limine scans the .requests section (between the two markers)
// and fills in each response before jumping to `kmain`.

/// Protocol revision 6: enter at EL2 with VHE when the firmware hands over
/// at EL2 (the O6, QEMU TCG), at EL1 otherwise (a KVM guest).
#[used]
#[unsafe(link_section = ".requests")]
static BASE_REVISION: BaseRevision = BaseRevision::new();

/// Where Limine mapped all of RAM ("higher half direct map", HHDM):
/// physical address + offset = virtual address.
#[used]
#[unsafe(link_section = ".requests")]
static HHDM: HhdmRequest = HhdmRequest::new();

/// The ACPI root pointer (RSDP), as a virtual address.
#[used]
#[unsafe(link_section = ".requests")]
static RSDP: RsdpRequest = RsdpRequest::new();

/// Where the kernel itself sits, physically and virtually.
#[used]
#[unsafe(link_section = ".requests")]
static EXECUTABLE: ExecutableAddressRequest = ExecutableAddressRequest::new();

/// The kernel command line (`cmdline:` in limine.conf).
#[used]
#[unsafe(link_section = ".requests")]
static CMDLINE: ExecutableCmdlineRequest = ExecutableCmdlineRequest::new();

#[used]
#[unsafe(link_section = ".requests_start_marker")]
static REQUESTS_START: RequestsStartMarker = RequestsStartMarker::new();

#[used]
#[unsafe(link_section = ".requests_end_marker")]
static REQUESTS_END: RequestsEndMarker = RequestsEndMarker::new();

// The real entry point. Limine hands over with PSTATE.SPSel = 0, i.e. running
// on SP_EL0, the stack pointer meant for user code; an exception would then
// switch to this level's own stack pointer (SP_EL2, or SP_EL1 under KVM),
// which nobody has set. So: keep the stack Limine gave us, but move it to
// our own stack pointer, then call kmain with the old SPSel value. `bti jc`
// marks this as a valid target of Limine's jump, should BTI be enforced.
core::arch::global_asm!(
    ".section .text._start, \"ax\"",
    ".global _start",
    "_start:",
    "bti jc",
    "mrs x0, SPSel",
    "mov x1, sp",
    "msr SPSel, #1",
    "mov sp, x1",
    "bl kmain",
    "1:",
    "wfe",
    "b 1b",
);

/// Stop this core for good (WFE waits for an event that never comes).
pub fn halt() -> ! {
    loop {
        // SAFETY: wfe has no effect besides waiting.
        unsafe { core::arch::asm!("wfe") };
    }
}

/// Called by `_start`, with interrupts masked, the MMU on and Limine's
/// 64 KiB stack. Must never return. `spsel_at_entry` is the stack-pointer
/// selection Limine handed over (0 = SP_EL0).
#[unsafe(no_mangle)]
extern "C" fn kmain(spsel_at_entry: u64) -> ! {
    // Without a supported protocol nothing Limine sent can be trusted, and
    // there is no console yet to say so.
    // Per-core variables first: the locks use them, and the console has one.
    percpu::init_boot_core();
    if !BASE_REVISION.is_supported() {
        halt();
    }
    // The screen first: it needs nothing but Limine's mappings, and on the
    // board it is the only output anyone sees. From here on `out` prints
    // to every console that is set up.
    let screens = screen::init();
    let mut out = Console;
    let (Some(hhdm), Some(rsdp), Some(exe)) = (HHDM.response(), RSDP.response(), EXECUTABLE.response()) else {
        let _ = writeln!(out, "Limine sent no direct-map offset, ACPI pointer or kernel address; halting.");
        halt()
    };
    let Some(tables) = acpi::Tables::new(rsdp.address as u64, hhdm.offset) else {
        let _ = writeln!(out, "No usable ACPI tables; halting.");
        halt()
    };
    let Some(uart_phys) = tables.spcr_uart() else {
        let _ = writeln!(out, "No console UART in the ACPI SPCR table; halting.");
        halt()
    };
    // Read now, while Limine's page tables still map every ACPI table.
    let conduit = tables.psci_conduit();
    let gic_info = tables.gic();
    let el = (mmu::read_current_el() >> 2) & 3;
    let timer_irq = tables.virtual_timer_interrupt(el == 2);
    let mut mpidrs = [0; stacks::MAX_CORES];
    let cores = tables.cores(&mut mpidrs);

    // Our page tables are statics inside the kernel image, so their
    // physical address is their offset from the kernel's start.
    let kernel_phys = |va: u64| va - exe.virtual_base + exe.physical_base;
    let uart_virt = mmu::map_device_page(uart_phys, kernel_phys);
    uart::set_console(uart_virt);

    let _ = writeln!(out, "\n{}", include_str!("banner.txt"));
    let _ = writeln!(out, "  booted by Limine, running at EL{el}");
    let _ = writeln!(out, "  serial console (ACPI SPCR): PL011 at {uart_phys:#x}, mapped at {uart_virt:#x}");
    screen::describe(&mut out);
    if screens == 0 {
        let _ = writeln!(out, "  (printing to serial only)");
    }
    let _ = writeln!(out, "  timer: {} MHz", timer::frequency() / 1_000_000);
    let _ = writeln!(out, "  kernel: virtual {:#x} = physical {:#x}", exe.virtual_base, exe.physical_base);
    let _ = writeln!(out, "  RAM direct map offset (HHDM): {:#x}", hhdm.offset);
    let _ = writeln!(out, "  stack: Limine handed over on SP_EL{}, now on SP_EL{el}", if spsel_at_entry & 1 == 1 { el } else { 0 });

    exceptions::install();
    let _ = writeln!(out, "  exception vectors installed");
    exceptions::self_test();

    // PAC and BTI, if this kernel was compiled for them.
    let protected = protect::built_with_branch_protection();
    if protected {
        // Load the key and mark our code guarded, then switch both
        // on with one SCTLR write. That write must happen here, in kmain, which
        // never returns: a function that started before PAC was on never signed
        // its return address, so its own `autiasp` would fail on the way out.
        let key = protect::load_pac_key();
        let guarded = protect::guard_kernel_code(hhdm.offset);
        // SAFETY: EnIA (bit 31) enables PAC with key A. BT (bit 36) makes
        // `paciasp` stop counting as a landing pad for plain `br` jumps, the
        // strict BTI mode. BTI checks themselves apply as soon as a page has
        // its GP bit, which guard_kernel_code just set.
        unsafe {
            core::arch::asm!(
                "mrs {t}, SCTLR_EL1",
                "orr {t}, {t}, #(1 << 31)",
                "orr {t}, {t}, #(1 << 36)",
                "msr SCTLR_EL1, {t}",
                "isb",
                t = out(reg) _,
            )
        };
        match key {
            protect::KeySource::Limine => {
                let _ = writeln!(out, "  PAC: on (return addresses signed), key from Limine's entropy");
            }
            protect::KeySource::Counter => {
                let _ = writeln!(out, "  PAC: on (return addresses signed), but WEAK key: Limine sent no entropy");
            }
        }
        let _ = writeln!(out, "  BTI: on, {guarded} kernel code pages marked guarded");
    } else {
        let _ = writeln!(out, "  PAC and BTI: left off; this kernel was built without branch protection (build it with `cargo kernel` or `cargo board`)");
    }

    // Physical memory.
    if !memory::init(&mut out, hhdm.offset) {
        let _ = writeln!(out, "No usable memory map from Limine; halting.");
        halt()
    }
    memory::self_test(&mut out, hhdm.offset);

    // Our own page tables: build, switch, move the console into the device
    // window, then switch off the lower half (reserved for user space).
    let kernel_tables = match vm::build(hhdm.offset, exe.virtual_base, exe.physical_base, uart_phys, protected) {
        Ok(t) => t,
        Err(e) => {
            let _ = writeln!(out, "Building page tables failed: {e}; halting.");
            halt()
        }
    };
    let _ = writeln!(out, "  page tables built: {} tables, {} pages of 16 KiB + {} blocks of 32 MiB, {} W^X violations",
        kernel_tables.tables, kernel_tables.pages, kernel_tables.blocks, kernel_tables.wx_violations);
    let uart = kernel_tables.uart;
    vm::switch_to(kernel_tables);
    uart::set_console(uart);
    vm::disable_lower_half();
    let _ = writeln!(out, "  running on our own page tables; console now at {uart:#x}; lower half off");

    // The heap grows in its own region of these tables.
    heap::self_test(&mut out);
    // Ready to save the log in UEFI variables, from now on even on a crash.
    if !efi::init(hhdm.offset) {
        let _ = writeln!(out, "  boot log: no EFI system table or memory map from Limine; it will not be saved");
    }

    // From here on, kernel code runs on this core's own stack, with a
    // guard page below it, and exceptions on a separate one (stacks.rs).
    // Limine's stack stays behind, holding `later`; it is never reused.
    let later = Later {
        gic: gic_info,
        timer_irq,
        conduit,
        mpidrs,
        cores,
        hhdm: hhdm.offset,
        kernel: (exe.virtual_base, exe.physical_base),
    };
    match stacks::allocate(0) {
        Ok(stacks) => {
            let _ = writeln!(out, "  stacks: moving to this core's kernel stack (top {:#x}) and exception stack (top {:#x}), each above a guard page",
                stacks.kernel_top, stacks.exception_top);
            stacks::switch(stacks, continue_boot, &later as *const Later as usize)
        }
        Err(e) => {
            let _ = writeln!(out, "Mapping this core's stacks failed: {e}; halting.");
            halt()
        }
    }
}

/// What the rest of the boot needs from the part on Limine's stack.
struct Later {
    gic: Option<acpi::Gic>,
    timer_irq: Option<(u32, bool)>,
    conduit: Option<psci::Conduit>,
    /// The cores in the MADT (MPIDR affinity values).
    mpidrs: [u64; stacks::MAX_CORES],
    cores: usize,
    hhdm: u64,
    /// The kernel's virtual and physical base addresses.
    kernel: (u64, u64),
}

/// The rest of the boot, on this core's own stacks.
extern "C" fn continue_boot(later: usize) -> ! {
    // SAFETY: `later` points to the `Later` in kmain's frame, which stays
    // put: kmain never returns and Limine's stack is never reused.
    let later = unsafe { &*(later as *const Later) };
    let mut out = Console;

    // Interrupts: the GIC first, then let them in, then the timer.
    match later.gic.as_ref().map(gic::init) {
        Some(Ok(found)) => {
            let _ = writeln!(out, "  interrupts: GICv{} (IIDR {:#x}), {} interrupt IDs, this core's redistributor at {:#x}",
                found.arch, found.iidr, found.lines, found.redistributor);
            irq::unmask();
            irq::self_test(&mut out);
            match later.timer_irq {
                Some((intid, edge)) => {
                    timer::init(intid, if edge { gic::Trigger::Edge } else { gic::Trigger::Level });
                    let (ok, us) = timer::self_test();
                    let _ = writeln!(out, "  timer: virtual timer on interrupt {intid}, one-shot deadlines{}; self-test: slept {us} us, {}",
                        if timer::self_synchronised() { ", counter read through CNTVCTSS" } else { "" },
                        if ok { "woken by its interrupt" } else { "FAILED (not woken by its interrupt)" });
                }
                None => {
                    let _ = writeln!(out, "  timer: the GTDT names no interrupt for the virtual timer; waiting by reading the counter");
                }
            }
            // The other cores.
            if let Some(conduit) = later.conduit {
                let (virt, phys) = later.kernel;
                smp::start_all(&mut out, &later.mpidrs[..later.cores], conduit, later.hhdm, |va| va - virt + phys);
            }
        }
        Some(Err(e)) => {
            let _ = writeln!(out, "  interrupts: setting up the GIC failed: {e}; carrying on without interrupts");
        }
        None => {
            let _ = writeln!(out, "  interrupts: no GIC in the MADT; carrying on without interrupts");
        }
    }

    // The kernel command line: deliberate faults on request, and how to end.
    let cmdline = CMDLINE.response().map_or("", |c| c.cmdline());
    let mut reboot_after = None;
    for word in cmdline.split_whitespace() {
        if let Some(secs) = word.strip_prefix("reboot-after=") {
            reboot_after = secs.parse::<u64>().ok();
            continue;
        }
        match word {
            "fault-test" => {
                let _ = writeln!(out, "  fault-test: reading unmapped address 0x0");
                // SAFETY: deliberately faults; the exception handler reports it and halts.
                unsafe { core::ptr::read_volatile(core::ptr::null::<u64>()) };
            }
            "pac-test" => {
                let _ = writeln!(out, "  pac-test: checking a corrupted signed pointer");
                protect::pac_test();
            }
            "bti-test" => {
                let _ = writeln!(out, "  bti-test: jumping to a non-landing-pad instruction");
                protect::bti_test();
            }
            "nested-test" => {
                let _ = writeln!(out, "  nested-test: a fault while a fault report is being printed");
                exceptions::pretend_reporting();
                // SAFETY: deliberately faults; the handler shows the squares and halts.
                unsafe { core::ptr::read_volatile(core::ptr::null::<u64>()) };
            }
            "stack-test" => {
                let _ = writeln!(out, "  stack-test: recursing until the stack runs out");
                recurse(0);
            }
            "wx-test" => {
                let target = kmain as *const () as *mut u32;
                let _ = writeln!(out, "  wx-test: writing to kernel code at {target:p}");
                // SAFETY: deliberately faults (code is mapped read-only); the
                // exception handler reports it and halts.
                unsafe { target.write_volatile(0) };
            }
            _ => {}
        }
    }

    let (unexpected, last) = irq::unexpected();
    if unexpected > 0 {
        let _ = writeln!(out, "  {unexpected} interrupts arrived that nothing handles (the last was {last}); they are switched off");
    }

    // Save everything printed so far, to read after the reboot.
    match efi::save_log() {
        Ok(saved) => {
            let _ = writeln!(out, "  boot log: {} bytes saved in {} UEFI variables{} (`cargo board log` prints them)",
                saved.bytes, saved.chunks, if saved.read_back { ", read back ok" } else { ", but reading it back FAILED" });
        }
        Err(e) => {
            let _ = writeln!(out, "  boot log: not saved, UEFI said {e}");
        }
    }

    let Some(conduit) = later.conduit else {
        let _ = writeln!(out, "No PSCI in the FADT; halting.");
        halt()
    };
    match reboot_after {
        // On the board: leave the output up for a while, then restart. The
        // one-time boot entry is used up by then, so Linux comes back.
        Some(secs) => {
            // Count down in place: `\r` goes back to the start of the line.
            let (start, freq) = (timer::now(), timer::frequency());
            for left in (1..=secs).rev() {
                let _ = write!(out, "\rRebooting in {left} s ");
                timer::sleep_until(start + (secs + 1 - left) * freq);
            }
            let _ = writeln!(out, "\rRebooting now.      ");
            let error = psci::system_reset(conduit);
            let _ = writeln!(out, "The firmware refused to restart (PSCI error {error}); press the reset button.");
        }
        None => {
            let _ = writeln!(out, "Powering off.");
            let error = psci::system_off(conduit);
            let _ = writeln!(out, "The firmware refused to power off (PSCI error {error}); halting.");
        }
    }
    halt()
}

/// For `stack-test`: calls itself forever, 1 KiB of stack per call, until
/// the kernel stack runs into its guard page.
fn recurse(depth: u64) -> u64 {
    if depth == u64::MAX {
        return 0; // never reached; keeps the compiler from calling it endless
    }
    let frame = core::hint::black_box([depth; 128]);
    recurse(depth + 1) + frame[0]
}

#[panic_handler]
fn panic(info: &core::panic::PanicInfo) -> ! {
    let _ = writeln!(console::Raw, "\nkernel panic: {info}");
    efi::save_log_after_crash(&mut console::Raw);
    halt()
}
