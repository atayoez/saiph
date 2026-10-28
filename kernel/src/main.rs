//! The Saiph kernel, early days: Limine loads it; it sets up its consoles
//! (screen and serial), exceptions, PAC and BTI, physical memory and its own
//! page tables, then powers the machine off, or reboots it after a wait
//! when the command line says `reboot-after=<seconds>` (as on the board).

#![no_std]
#![no_main]

mod acpi;
mod console;
mod exceptions;
mod memory;
mod mmu;
mod protect;
mod psci;
mod screen;
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

    // Our page tables are statics inside the kernel image, so their
    // physical address is their offset from the kernel's start.
    let kernel_phys = |va: u64| va - exe.virtual_base + exe.physical_base;
    let uart_virt = mmu::map_device_page(uart_phys, kernel_phys);
    uart::set_console(uart_virt);

    let el = (mmu::read_current_el() >> 2) & 3;
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

    // PAC and BTI: load the key and mark our code guarded, then switch both
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

    // Physical memory.
    let Some(mut frames) = memory::init(&mut out, hhdm.offset) else {
        let _ = writeln!(out, "No usable memory map from Limine; halting.");
        halt()
    };
    memory::self_test(&mut out, &mut frames, hhdm.offset);

    // Our own page tables: build, switch, move the console into the device
    // window, then switch off the lower half (reserved for user space).
    let kernel_tables = match vm::build(&mut frames, hhdm.offset, exe.virtual_base, exe.physical_base, uart_phys) {
        Ok(t) => t,
        Err(e) => {
            let _ = writeln!(out, "Building page tables failed: {e}; halting.");
            halt()
        }
    };
    let _ = writeln!(out, "  page tables built: {} tables, {} pages + {} blocks of 2 MiB, {} W^X violations",
        kernel_tables.tables, kernel_tables.pages, kernel_tables.blocks, kernel_tables.wx_violations);
    vm::switch_to(kernel_tables.root);
    uart::set_console(kernel_tables.uart);
    vm::disable_lower_half();
    let _ = writeln!(out, "  running on our own page tables; console now at {:#x}; lower half off", kernel_tables.uart);

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

    let Some(conduit) = conduit else {
        let _ = writeln!(out, "No PSCI in the FADT; halting.");
        halt()
    };
    match reboot_after {
        // On the board: leave the output up for a while, then restart. The
        // one-time boot entry is used up by then, so Linux comes back.
        Some(secs) => {
            let _ = writeln!(out, "Rebooting in {secs} seconds.");
            timer::wait_seconds(secs);
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

#[panic_handler]
fn panic(info: &core::panic::PanicInfo) -> ! {
    let _ = writeln!(Console, "\nkernel panic: {info}");
    halt()
}
