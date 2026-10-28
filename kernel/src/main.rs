//! Orion kernel, first stub: Limine loads it, it finds the console UART in
//! ACPI, maps it, prints where it is running, and powers the machine off.

#![no_std]
#![no_main]

mod acpi;
mod mmu;
mod psci;
mod uart;

use core::fmt::Write;
use limine::request::{ExecutableAddressRequest, HhdmRequest, RsdpRequest};
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

#[used]
#[unsafe(link_section = ".requests_start_marker")]
static REQUESTS_START: RequestsStartMarker = RequestsStartMarker::new();

#[used]
#[unsafe(link_section = ".requests_end_marker")]
static REQUESTS_END: RequestsEndMarker = RequestsEndMarker::new();

/// Stop this core for good (WFE waits for an event that never comes).
fn halt() -> ! {
    loop {
        // SAFETY: wfe has no effect besides waiting.
        unsafe { core::arch::asm!("wfe") };
    }
}

/// The entry point Limine jumps to, with interrupts masked, the MMU on and a
/// 64 KiB stack. It must never return: Limine set the return address to 0.
#[unsafe(no_mangle)]
extern "C" fn kmain() -> ! {
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
