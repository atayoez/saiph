//! Group G: the same registers on every core.
//!
//! At boot only the boot core runs. The firmware's MP services protocol can
//! run a function on another core: upstream EDK2 powers the core on through
//! PSCI, runs the function, and powers it off again. That function must not
//! print or call the firmware (it is not built for several cores at once),
//! so it is the assembly routine from cpu.rs, which only copies registers
//! into memory; the boot core prints the results afterwards.

use crate::acpi::Madt;
use crate::cpu::{self, Ids};
use crate::report::{Report, say};
use alloc::boxed::Box;
use alloc::vec::Vec;
use core::ffi::c_void;
use core::time::Duration;
use uefi::boot;
use uefi::proto::pi::mp::MpServices;

/// Runs on the other core.
extern "efiapi" fn on_core(arg: *mut c_void) {
    // SAFETY: `arg` is a separate, live `Ids` owned by the boot core.
    unsafe { cpu::orion_probe_read_ids(arg as *mut Ids) };
}

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

pub fn report(r: &mut Report, boot_core: &Ids, madt: Option<&Madt>) {
    let mp = match boot::get_handle_for_protocol::<MpServices>().and_then(boot::open_protocol_exclusive::<MpServices>) {
        Ok(mp) => mp,
        Err(e) => {
            say!(r, "The firmware offers no MP services protocol ({:?}); only the boot core was read.", e.status());
            say!(r, "(QEMU's AAVMF firmware never has it; the O6's firmware may.)");
            return;
        }
    };
    let total = match mp.get_number_of_processors() {
        Ok(n) => n.total,
        Err(e) => {
            say!(r, "GetNumberOfProcessors failed ({:?})", e.status());
            return;
        }
    };
    say!(r, "{total} cores. '-' = same ID registers as the boot core.");
    say!(r, " #  MPIDR        core           eff  caches                        differs");
    for i in 0..total {
        let Ok(info) = mp.get_processor_info(i) else {
            say!(r, "{i:>2}  (no information)");
            continue;
        };
        let ids = if info.is_bsp() {
            Ok(*boot_core)
        } else if !info.is_enabled() {
            Err(alloc::string::String::from("disabled by the firmware"))
        } else {
            let mut slot = Box::new(Ids::default());
            let arg = &mut *slot as *mut Ids as *mut c_void;
            match mp.startup_this_ap(i, on_core, arg, None, Some(Duration::from_secs(1))) {
                Ok(()) if slot.midr != 0 => {
                    slot.tidy();
                    Ok(*slot)
                }
                Ok(()) => Err(alloc::string::String::from("returned without results")),
                Err(e) => Err(alloc::format!("could not run on this core ({:?})", e.status())),
            }
        };
        // The MADT's power-efficiency class (0 = most efficient on this board).
        let eff = madt
            .and_then(|m| m.cpus.iter().find(|c| c.0 == info.processor_id & 0xff_00ff_ffff))
            .map_or(alloc::string::String::from("?"), |c| alloc::format!("{}", c.1));
        match ids {
            Ok(ids) => {
                let d = differences(boot_core, &ids);
                let model = alloc::format!("{} r{}p{}", cpu::core_name(ids.midr), (ids.midr >> 20) & 0xf, ids.midr & 0xf);
                say!(r, "{i:>2}  {:#011x}  {model:<13}  {eff:>3}  {:<28}  {}{}", info.processor_id, cpu::cache_summary(&ids),
                    if d.is_empty() { alloc::string::String::from("-") } else { d.join(",") }, if info.is_bsp() { "  (boot core)" } else { "" });
            }
            Err(e) => say!(r, "{i:>2}  {:#011x}  {e}", info.processor_id),
        }
        r.flush();
    }
}
