//! Group G: the same registers on every core.
//!
//! At boot only the boot core runs. There are two ways to run the register
//! reader (the assembly routine in cpu.rs) on the others:
//!
//! 1. The firmware's MP services protocol, if it has one: upstream EDK2
//!    powers the core on through PSCI, runs our function, and powers it off.
//! 2. Otherwise we do that ourselves: PSCI CPU_ON starts the core at a small
//!    assembly entry point with the MMU and caches off; it reads the
//!    registers into a result slot, marks it done and calls PSCI CPU_OFF.
//!    We then wait until PSCI AFFINITY_INFO reports the core off again, so
//!    the OS can later start it normally.
//!
//! Neither function may print or call the firmware (it is not built for
//! several cores at once); the boot core prints all results.

use crate::acpi::{Madt, Table};
use crate::cpu::{self, Ids};
use crate::firmware::{self, Conduit};
use crate::report::{Report, say};
use alloc::boxed::Box;
use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use core::ffi::c_void;
use core::mem::offset_of;
use core::time::Duration;
use uefi::boot::{self, AllocateType, MemoryType};
use uefi::proto::pi::mp::MpServices;

/// Affinity bits of an MPIDR (Aff3 in 39..32, Aff2..Aff0 in 23..0).
const AFFINITY: u64 = 0xff_00ff_ffff;

/// Names of the registers in which `a` and `b` differ (MPIDR always does).
fn differences(a: &Ids, b: &Ids) -> Vec<&'static str> {
    let pairs = [
        ("MIDR", a.midr, b.midr), ("PFR0", a.pfr0, b.pfr0), ("PFR1", a.pfr1, b.pfr1),
        ("ISAR0", a.isar0, b.isar0), ("ISAR1", a.isar1, b.isar1), ("ISAR2", a.isar2, b.isar2),
        ("MMFR0", a.mmfr0, b.mmfr0), ("MMFR1", a.mmfr1, b.mmfr1), ("MMFR2", a.mmfr2, b.mmfr2),
        ("DFR0", a.dfr0, b.dfr0), ("ZFR0", a.zfr0, b.zfr0), ("CLIDR", a.clidr, b.clidr),
    ];
    let mut out: Vec<&str> = pairs.iter().filter(|p| p.1 != p.2).map(|p| p.0).collect();
    if a.ccsidr != b.ccsidr {
        out.push("caches");
    }
    out
}

fn print_core(r: &mut Report, i: usize, mpidr: u64, eff: Option<u8>, boot_core: &Ids, ids: Result<Ids, String>) -> Option<Ids> {
    let eff = eff.map_or(String::from("?"), |e| format!("{e}"));
    let out = ids.as_ref().ok().copied();
    match ids {
        Ok(ids) => {
            let d = differences(boot_core, &ids);
            let model = model(&ids);
            let is_boot = ids.mpidr & AFFINITY == boot_core.mpidr & AFFINITY;
            say!(r, "{i:>2}  {mpidr:#011x}  {model:<17}  {eff:>3}  {:<26}  {}{}", cpu::cache_summary(&ids),
                if d.is_empty() { String::from("-") } else { d.join(",") }, if is_boot { "  (boot core)" } else { "" });
        }
        Err(e) => say!(r, "{i:>2}  {mpidr:#011x}  {e}"),
    }
    r.flush();
    out
}

fn model(ids: &Ids) -> String {
    format!("{} r{}p{}", cpu::core_name(ids.midr), (ids.midr >> 20) & 0xf, ids.midr & 0xf)
}

/// For the first core of each type other than the boot core's, list every
/// ID-register field that differs, then that core type's caches.
fn compare_types(r: &mut Report, boot_core: &Ids, cores: &[Ids]) {
    let mut seen = alloc::vec![boot_core.midr];
    for ids in cores {
        if seen.contains(&ids.midr) {
            continue;
        }
        seen.push(ids.midr);
        say!(r, "");
        say!(r, "{} (MPIDR {:#x}) compared with the boot core, a {}:", model(ids), ids.mpidr & AFFINITY, model(boot_core));
        say!(r, "  (values: boot core -> this core)");
        let b = boot_core;
        let registers = [
            ("PFR0", b.pfr0, ids.pfr0), ("PFR1", b.pfr1, ids.pfr1), ("ISAR0", b.isar0, ids.isar0),
            ("ISAR1", b.isar1, ids.isar1), ("ISAR2", b.isar2, ids.isar2), ("MMFR0", b.mmfr0, ids.mmfr0),
            ("MMFR1", b.mmfr1, ids.mmfr1), ("MMFR2", b.mmfr2, ids.mmfr2), ("DFR0", b.dfr0, ids.dfr0),
            ("ZFR0", b.zfr0, ids.zfr0),
        ];
        for (name, x, y) in registers {
            if x == y {
                continue;
            }
            say!(r, "  ID_AA64{name}_EL1: {x:#018x} -> {y:#018x}");
            let names = cpu::field_names(name);
            for (n, field) in names.iter().enumerate() {
                let (fx, fy) = (cpu::field(x, 4 * n as u32), cpu::field(y, 4 * n as u32));
                if fx != fy {
                    say!(r, "    {field:<11} {fx:x} -> {fy:x}  {}", cpu::field_meaning(field));
                }
            }
        }
        say!(r, "  Caches of this core type:");
        cpu::report_caches(r, ids);
        r.flush();
    }
}

fn header(r: &mut Report, n: usize) {
    say!(r, "{n} cores. '-' = same ID registers as the boot core.");
    say!(r, " #  MPIDR        core               eff  caches                      differs");
}

pub fn report(r: &mut Report, boot_core: &Ids, madt: Option<&Madt>, tables: &[Table]) {
    let mp = boot::get_handle_for_protocol::<MpServices>().and_then(boot::open_protocol_exclusive::<MpServices>);
    match mp {
        Ok(mp) => with_mp_services(r, &mp, boot_core, madt),
        Err(_) => {
            say!(r, "The firmware offers no MP services protocol; starting the cores through PSCI instead.");
            with_psci(r, boot_core, madt, tables);
        }
    }
}

// ---------------------------------------------------------------------------
// Way 1: EFI_MP_SERVICES_PROTOCOL.

/// Runs on the other core.
extern "efiapi" fn on_core(arg: *mut c_void) {
    // SAFETY: `arg` is a separate, live `Ids` owned by the boot core.
    unsafe { cpu::orion_probe_read_ids(arg as *mut Ids) };
}

fn with_mp_services(r: &mut Report, mp: &MpServices, boot_core: &Ids, madt: Option<&Madt>) {
    let total = match mp.get_number_of_processors() {
        Ok(n) => n.total,
        Err(e) => {
            say!(r, "GetNumberOfProcessors failed ({:?})", e.status());
            return;
        }
    };
    header(r, total);
    let mut read = Vec::new();
    for i in 0..total {
        let Ok(info) = mp.get_processor_info(i) else {
            say!(r, "{i:>2}  (no information)");
            continue;
        };
        let ids = if info.is_bsp() {
            Ok(*boot_core)
        } else if !info.is_enabled() {
            Err(String::from("disabled by the firmware"))
        } else {
            let mut slot = Box::new(Ids::default());
            let arg = &mut *slot as *mut Ids as *mut c_void;
            match mp.startup_this_ap(i, on_core, arg, None, Some(Duration::from_secs(1))) {
                Ok(()) if slot.midr != 0 => {
                    slot.tidy();
                    Ok(*slot)
                }
                Ok(()) => Err(String::from("returned without results")),
                Err(e) => Err(format!("could not run on this core ({:?})", e.status())),
            }
        };
        let eff = madt.and_then(|m| m.cpus.iter().find(|c| c.mpidr == info.processor_id & AFFINITY)).map(|c| c.efficiency);
        read.extend(print_core(r, i, info.processor_id, eff, boot_core, ids));
    }
    compare_types(r, boot_core, &read);
}

// ---------------------------------------------------------------------------
// Way 2: PSCI CPU_ON with our own entry point.

/// Result slot for a core started with CPU_ON. The entry point below uses
/// the field offsets.
#[repr(C)]
struct Slot {
    ids: Ids,
    /// Set to 1 by the core once `ids` is filled in.
    done: u64,
    /// 1: call CPU_OFF with HVC (KVM guest), 0: with SMC.
    use_hvc: u64,
}

// The started core begins here, at our exception level with the MMU and
// caches off, interrupts masked, no stack; x0 holds the slot address that
// we passed as CPU_ON's "context ID". It fills in the slot, marks it done,
// and powers itself off with PSCI CPU_OFF (function 0x84000002), which does
// not return; if it ever did, the core parks in a WFE loop.
core::arch::global_asm!(
    ".global orion_probe_core_entry",
    ".p2align 2",
    "orion_probe_core_entry:",
    "mov x19, x0",
    "bl orion_probe_read_ids", // x0 = &slot.ids (offset 0); uses no stack
    "mov x1, #1",
    "str x1, [x19, #{done}]",
    "dsb sy",
    "ldr x2, [x19, #{use_hvc}]",
    "movz x0, #0x0002",
    "movk x0, #0x8400, lsl #16",
    "cbnz x2, 1f",
    "smc #0",
    "b 2f",
    "1:",
    "hvc #0",
    "2:",
    "wfe",
    "b 2b",
    done = const offset_of!(Slot, done),
    use_hvc = const offset_of!(Slot, use_hvc),
);

unsafe extern "efiapi" {
    fn orion_probe_core_entry();
}

const CPU_ON: u32 = 0xc400_0003;
const AFFINITY_INFO: u32 = 0xc400_0004;

/// Write back and drop `len` bytes at `start` from this core's caches
/// (DC CIVAC, "clean and invalidate to the point of coherency"), so a core
/// running with caches off sees what we wrote, and we then see what it wrote.
fn clean_and_invalidate(start: u64, len: u64) {
    // CTR_EL0 bits 19..16: log2 of the smallest data cache line in 4-byte words.
    let line = 4u64 << ((read_sysreg!("CTR_EL0") >> 16) & 0xf);
    let mut a = start & !(line - 1);
    while a < start + len {
        // SAFETY: cache maintenance does not change memory contents.
        unsafe { core::arch::asm!("dc civac, {}", in(reg) a) };
        a += line;
    }
    // SAFETY: as above.
    unsafe { core::arch::asm!("dsb sy") };
}

fn psci_error(code: i64) -> &'static str {
    match code {
        -1 => "NOT_SUPPORTED",
        -2 => "INVALID_PARAMETERS",
        -3 => "DENIED",
        -4 => "ALREADY_ON",
        -5 => "ON_PENDING",
        -6 => "INTERNAL_FAILURE",
        -7 => "NOT_PRESENT",
        -8 => "DISABLED",
        -9 => "INVALID_ADDRESS",
        _ => "unknown error",
    }
}

/// Start one core, collect its registers and wait until it is off again.
fn run_on(c: Conduit, mpidr: u64, slot: *mut Slot) -> Result<Ids, String> {
    // SAFETY: `slot` is a page of our own memory, not in use elsewhere.
    unsafe {
        slot.write(Slot { ids: Ids::default(), done: 0, use_hvc: (c == Conduit::Hvc) as u64 });
    }
    clean_and_invalidate(slot as u64, size_of::<Slot>() as u64);
    // The core runs these two routines with caches off, so they must be in memory.
    clean_and_invalidate(orion_probe_core_entry as *const () as u64, 256);
    clean_and_invalidate(cpu::orion_probe_read_ids as *const () as u64, 1024);

    let entry = orion_probe_core_entry as *const () as u64; // UEFI maps 1:1, so this is physical
    let rc = firmware::smccc(c, CPU_ON, [mpidr, entry, slot as u64]);
    if rc != 0 {
        return Err(format!("CPU_ON failed: {} ({rc})", psci_error(rc)));
    }
    let mut finished = false;
    for _ in 0..1000 {
        clean_and_invalidate(slot as u64, size_of::<Slot>() as u64);
        // SAFETY: the core only writes into the slot; we read it.
        if unsafe { core::ptr::read_volatile(&raw const (*slot).done) } == 1 {
            finished = true;
            break;
        }
        boot::stall(Duration::from_millis(1));
    }
    // Wait for the core to be powered off again (AFFINITY_INFO: 0 on, 1 off).
    let mut off = false;
    for _ in 0..1000 {
        if firmware::smccc(c, AFFINITY_INFO, [mpidr, 0, 0]) == 1 {
            off = true;
            break;
        }
        boot::stall(Duration::from_millis(1));
    }
    match (finished, off) {
        (true, true) => {
            // SAFETY: the core is off; the slot is ours alone again.
            let mut ids = unsafe { (*slot).ids };
            ids.tidy();
            Ok(ids)
        }
        (true, false) => Err(String::from("read its registers, but did not power off within 1 s")),
        (false, _) => Err(format!("did not report back within 1 s (powered off: {})", if off { "yes" } else { "NO" })),
    }
}

fn with_psci(r: &mut Report, boot_core: &Ids, madt: Option<&Madt>, tables: &[Table]) {
    let Some(m) = madt else {
        say!(r, "No MADT, so the other cores' addresses are unknown; skipped.");
        return;
    };
    let Some(c) = firmware::conduit(boot_core, tables) else {
        say!(r, "No safe way to reach PSCI from here; skipped.");
        return;
    };
    let Ok(page) = boot::allocate_pages(AllocateType::AnyPages, MemoryType::LOADER_DATA, 1) else {
        say!(r, "Could not allocate the result slot; skipped.");
        return;
    };
    let slot = page.as_ptr() as *mut Slot;
    header(r, m.cpus.len());
    let mut all_off = true;
    let mut read = Vec::new();
    for (i, core) in m.cpus.iter().enumerate() {
        let ids = if core.mpidr == boot_core.mpidr & AFFINITY {
            Ok(*boot_core)
        } else if !core.enabled {
            Err(String::from("disabled in the MADT"))
        } else {
            let result = run_on(c, core.mpidr, slot);
            if result.is_err() {
                all_off = false;
            }
            result
        };
        read.extend(print_core(r, i, core.mpidr, Some(core.efficiency), boot_core, ids));
    }
    compare_types(r, boot_core, &read);
    // A core that timed out might still write into the slot later, so the
    // page is only given back when every started core is known to be off.
    if all_off {
        // SAFETY: no core uses the slot any more.
        let _ = unsafe { boot::free_pages(page, 1) };
    }
}
