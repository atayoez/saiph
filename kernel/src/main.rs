//! Orion kernel, first stub: Limine loads it, it finds the console UART in
//! ACPI, maps it, prints where it is running, and powers the machine off.

#![no_std]
#![no_main]

mod acpi;
mod exceptions;
mod mmu;
mod protect;
mod psci;
mod uart;

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
    // Without a console there is nobody to tell, so problems before the
    // UART is mapped just stop the core.
    if !BASE_REVISION.is_supported() {
        halt();
    }
    let (Some(hhdm), Some(rsdp), Some(exe)) = (HHDM.response(), RSDP.response(), EXECUTABLE.response()) else {
        halt()
    };
    let Some(tables) = acpi::Tables::new(rsdp.address as u64, hhdm.offset) else { halt() };
    let Some(uart_phys) = tables.spcr_uart() else { halt() };

    // Our page tables are statics inside the kernel image, so their
    // physical address is their offset from the kernel's start.
    let kernel_phys = |va: u64| va - exe.virtual_base + exe.physical_base;
    let uart_virt = mmu::map_device_page(uart_phys, kernel_phys);
    let mut out = uart::Pl011::new(uart_virt);

    let el = (mmu::read_current_el() >> 2) & 3;
    let _ = writeln!(out, "\nOrion kernel: booted by Limine, running at EL{el}");
    let _ = writeln!(out, "  console (ACPI SPCR): PL011 at {uart_phys:#x}, mapped at {uart_virt:#x}");
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
    // SAFETY: EnIA (bit 31) enables PAC with key A and BT (bit 36) enables
    // BTI checks on guarded pages, both for this exception level.
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

    // Deliberate faults, on request from the kernel command line.
    let cmdline = CMDLINE.response().map_or("", |c| c.cmdline());
    for word in cmdline.split_whitespace() {
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
            _ => {}
        }
    }

    match tables.psci_conduit() {
        Some(conduit) => {
            let _ = writeln!(out, "Powering off.");
            psci::system_off(conduit)
        }
        None => {
            let _ = writeln!(out, "No PSCI in the FADT; halting.");
            halt()
        }
    }
}

#[panic_handler]
fn panic(info: &core::panic::PanicInfo) -> ! {
    if let Some(mut out) = uart::console() {
        let _ = writeln!(out, "\nkernel panic: {info}");
    }
    halt()
}
