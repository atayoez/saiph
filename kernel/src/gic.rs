//! The interrupt controller: Arm's GIC-700 on the O6 (a GICv4.1; QEMU
//! emulates a GICv4 or GICv3 for us).
//!
//! It has three parts. The distributor (one, memory-mapped) routes the
//! interrupts all cores share (SPIs, 32 and up). Each core has its own
//! redistributor (memory-mapped, two or four 64 KiB frames per core), which
//! handles that core's private interrupts: SGIs (0-15, one core tapping
//! another) and PPIs (16-31, like its own timer). The CPU interface is a set
//! of system registers (ICC_*): the interrupt handler reads ICC_IAR1 to
//! learn which interrupt it is and writes ICC_EOIR1 when done.
//!
//! All interrupts are in group 1 ("non-secure"), which the CPU takes as an
//! IRQ. Every interrupt gets the same priority, so an interrupt handler is
//! never interrupted itself. ICC_CTLR.EOImode stays 0: the EOIR write both
//! drops the priority and deactivates the interrupt. (EOImode 1 splits the
//! two, which the GIC-700's erratum 2941627 would need a workaround for.)

use crate::acpi;
use crate::sync::{IrqsOn, SpinLock};
use crate::vm::{self, DEVICE_WINDOW};
use core::sync::atomic::{AtomicU64, Ordering};
use paging::{Kind, PAGE_SIZE};

// Distributor registers.
const GICD_CTLR: u64 = 0x0000;
const GICD_TYPER: u64 = 0x0004;
const GICD_IIDR: u64 = 0x0008;
const GICD_IGROUPR: u64 = 0x0080;
const GICD_ICENABLER: u64 = 0x0180;
const GICD_ICPENDR: u64 = 0x0280;
const GICD_IPRIORITYR: u64 = 0x0400;
/// Peripheral ID 2: bits 7..4 are the GIC architecture version.
const GICD_PIDR2: u64 = 0xffe8;
/// CTLR bits: affinity routing (ARE_NS), group 1 enables, write pending.
const CTLR_ARE_NS: u32 = 1 << 4;
const CTLR_ENABLE_G1A: u32 = 1 << 1;
const CTLR_ENABLE_G1: u32 = 1 << 0;
const CTLR_RWP: u32 = 1 << 31;

// Redistributor registers: the RD frame...
const GICR_CTLR: u64 = 0x0000;
const GICR_TYPER: u64 = 0x0008;
const GICR_WAKER: u64 = 0x0014;
const GICR_CTLR_RWP: u32 = 1 << 3;
const TYPER_LAST: u64 = 1 << 4;
const TYPER_VLPIS: u64 = 1 << 1;
const WAKER_PROCESSOR_SLEEP: u32 = 1 << 1;
const WAKER_CHILDREN_ASLEEP: u32 = 1 << 2;
// ...and the SGI frame (64 KiB further), for SGIs and PPIs.
const SGI_FRAME: u64 = 0x1_0000;
const GICR_IGROUPR0: u64 = 0x0080;
const GICR_ISENABLER0: u64 = 0x0100;
const GICR_ICENABLER0: u64 = 0x0180;
const GICR_ICPENDR0: u64 = 0x0280;
const GICR_IPRIORITYR: u64 = 0x0400;
const GICR_ICFGR1: u64 = 0x0c04;

/// The priority of every interrupt (lower is more urgent; the middle).
const PRIORITY: u8 = 0xa0;
/// Size of the distributor's register block.
const DISTRIBUTOR_SIZE: u64 = 64 * 1024;

/// The distributor, mapped (0 = not yet).
static DISTRIBUTOR: AtomicU64 = AtomicU64::new(0);
crate::cpu_local! {
    /// This core's SGI frame, mapped (0 = not yet).
    static SGI_BASE: AtomicU64 = AtomicU64::new(0);
}
/// Read-modify-write changes to the configuration go one at a time. Only
/// set-up code changes it, never an interrupt handler.
static CONFIG: SpinLock<(), IrqsOn> = SpinLock::new(());
/// The redistributor ranges, mapped: (virtual, physical, length), and how
/// many there are.
type Ranges = ([(u64, u64, u64); 4], usize);
static RANGES: SpinLock<Ranges, IrqsOn> = SpinLock::new(([(0, 0, 0); 4], 0));

fn read32(addr: u64) -> u32 {
    // SAFETY: only called with addresses inside the mapped GIC blocks.
    unsafe { core::ptr::read_volatile(addr as *const u32) }
}
fn write32(addr: u64, value: u32) {
    // SAFETY: as for read32.
    unsafe { core::ptr::write_volatile(addr as *mut u32, value) }
}
fn read64(addr: u64) -> u64 {
    // SAFETY: as for read32.
    unsafe { core::ptr::read_volatile(addr as *const u64) }
}

/// Spin until `bit` of the register at `addr` reads 0 (a few microseconds).
fn wait_clear(addr: u64, bit: u32) {
    while read32(addr) & bit != 0 {
        core::hint::spin_loop();
    }
}

#[derive(Debug)]
pub enum Error {
    Map(vm::Error),
    /// None of the redistributors is this core's.
    NoRedistributor,
}

impl core::fmt::Display for Error {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Error::Map(e) => write!(f, "mapping its registers failed: {e}"),
            Error::NoRedistributor => write!(f, "no redistributor belongs to this core"),
        }
    }
}

/// What `init` found.
pub struct Found {
    /// Architecture version, from the distributor's ID registers.
    pub arch: u32,
    pub iidr: u32,
    /// Interrupt IDs the distributor handles (SGIs, PPIs and SPIs).
    pub lines: u32,
    /// Physical address of this core's redistributor.
    pub redistributor: u64,
}

fn map(phys: u64, len: u64) -> Result<u64, Error> {
    let first = phys & !(PAGE_SIZE - 1);
    let end = (phys + len).next_multiple_of(PAGE_SIZE);
    vm::map(DEVICE_WINDOW + first, first, end - first, Kind::Device).map_err(Error::Map)?;
    Ok(DEVICE_WINDOW + phys)
}

/// This core's affinity in the format GICR_TYPER uses: Aff3.Aff2.Aff1.Aff0.
fn my_affinity() -> u64 {
    let mpidr: u64;
    // SAFETY: reading MPIDR has no side effects.
    unsafe { core::arch::asm!("mrs {}, MPIDR_EL1", out(reg) mpidr) };
    ((mpidr >> 8) & 0xff00_0000) | (mpidr & 0x00ff_ffff)
}

/// Set up the distributor, this core's redistributor and CPU interface.
/// Every interrupt starts out disabled; `enable` turns one on. Interrupts
/// still stay masked on the core until `irq::unmask` (DAIF).
pub fn init(gic: &acpi::Gic) -> Result<Found, Error> {
    let dist = map(gic.distributor, DISTRIBUTOR_SIZE)?;

    // Distributor: off while it is set up, as Linux does it. Then all
    // shared interrupts disabled, not pending, in group 1, one priority.
    write32(dist + GICD_CTLR, 0);
    wait_clear(dist + GICD_CTLR, CTLR_RWP);
    let lines = 32 * ((read32(dist + GICD_TYPER) & 0x1f) + 1);
    for n in 1..lines / 32 {
        let reg = 4 * n as u64;
        write32(dist + GICD_IGROUPR + reg, !0);
        write32(dist + GICD_ICENABLER + reg, !0);
        write32(dist + GICD_ICPENDR + reg, !0);
    }
    for n in 32..lines {
        // SAFETY: a byte-sized priority register inside the distributor.
        unsafe { core::ptr::write_volatile((dist + GICD_IPRIORITYR + n as u64) as *mut u8, PRIORITY) };
    }
    wait_clear(dist + GICD_CTLR, CTLR_RWP);
    write32(dist + GICD_CTLR, CTLR_ARE_NS | CTLR_ENABLE_G1A | CTLR_ENABLE_G1);
    wait_clear(dist + GICD_CTLR, CTLR_RWP);

    // Map every redistributor range once; each core finds its own in them.
    {
        let mut ranges = RANGES.lock();
        for (n, &(base, len)) in gic.redistributors[..gic.ranges].iter().enumerate() {
            ranges.0[n] = (map(base, len)?, base, len);
        }
        ranges.1 = gic.ranges;
    }
    DISTRIBUTOR.store(dist, Ordering::Release);
    Ok(Found {
        arch: (read32(dist + GICD_PIDR2) >> 4) & 0xf,
        iidr: read32(dist + GICD_IIDR),
        lines,
        redistributor: init_this_core()?,
    })
}

/// Set up this core's redistributor and CPU interface (after `init`, on
/// every core). Returns the redistributor's physical address.
pub fn init_this_core() -> Result<u64, Error> {
    // Walk the ranges until a redistributor's affinity is ours. Each takes
    // 128 KiB, or 256 KiB with GICv4's two extra frames.
    let me = my_affinity();
    let (ranges, count) = *RANGES.lock();
    let mut found = None;
    'ranges: for &(virt, base, len) in &ranges[..count] {
        let mut offset = 0;
        while offset < len {
            let typer = read64(virt + offset + GICR_TYPER);
            if typer >> 32 == me {
                found = Some((virt + offset, base + offset));
                break 'ranges;
            }
            if typer & TYPER_LAST != 0 {
                break;
            }
            offset += if typer & TYPER_VLPIS != 0 { 0x4_0000 } else { 0x2_0000 };
        }
    }
    let (rd, rd_phys) = found.ok_or(Error::NoRedistributor)?;

    // Wake it (firmware may have left it "asleep"), then set up the SGIs
    // and PPIs like the shared ones.
    write32(rd + GICR_WAKER, read32(rd + GICR_WAKER) & !WAKER_PROCESSOR_SLEEP);
    wait_clear(rd + GICR_WAKER, WAKER_CHILDREN_ASLEEP);
    let sgi = rd + SGI_FRAME;
    write32(sgi + GICR_IGROUPR0, !0);
    write32(sgi + GICR_ICENABLER0, !0);
    wait_clear(rd + GICR_CTLR, GICR_CTLR_RWP);
    write32(sgi + GICR_ICPENDR0, !0);
    for n in 0..8 {
        write32(sgi + GICR_IPRIORITYR + 4 * n, u32::from_ne_bytes([PRIORITY; 4]));
    }
    write32(sgi + GICR_ICFGR1, 0); // all PPIs level-triggered until `configure` says otherwise

    // The CPU interface: use it through system registers (SRE), let every
    // priority through (PMR), EOImode 0, take group 1 interrupts.
    // SAFETY: GIC system registers, set up as described above.
    unsafe {
        if (crate::mmu::read_current_el() >> 2) & 3 == 2 {
            // At EL2 its own SRE register; bit 3 also allows EL1 to use it.
            core::arch::asm!("mrs {t}, ICC_SRE_EL2", "orr {t}, {t}, #0x1", "orr {t}, {t}, #0x8", "msr ICC_SRE_EL2, {t}", "isb", t = out(reg) _);
        } else {
            core::arch::asm!("mrs {t}, ICC_SRE_EL1", "orr {t}, {t}, #0x1", "msr ICC_SRE_EL1, {t}", "isb", t = out(reg) _);
        }
        core::arch::asm!(
            "msr ICC_PMR_EL1, {pmr}",
            "msr ICC_BPR1_EL1, xzr",
            "mrs {t}, ICC_CTLR_EL1",
            "bic {t}, {t}, #(1 << 1)",
            "msr ICC_CTLR_EL1, {t}",
            "mov {t}, #1",
            "msr ICC_IGRPEN1_EL1, {t}",
            "isb",
            pmr = in(reg) 0xffu64,
            t = out(reg) _,
        );
    }
    SGI_BASE.get().store(sgi, Ordering::Release);
    Ok(rd_phys)
}

/// How an interrupt signals.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Trigger {
    /// Asserted for as long as the device wants attention (timers).
    Level,
    /// A single pulse.
    Edge,
}

/// Set how a private interrupt (PPI, 16-31) of this core signals.
pub fn configure(intid: u32, trigger: Trigger) {
    let sgi = SGI_BASE.get().load(Ordering::Acquire);
    if sgi == 0 || !(16..32).contains(&intid) {
        return;
    }
    let bit = 1 << (2 * (intid - 16) + 1);
    let _config = CONFIG.lock();
    let cfg = read32(sgi + GICR_ICFGR1);
    write32(sgi + GICR_ICFGR1, if trigger == Trigger::Edge { cfg | bit } else { cfg & !bit });
}

/// Enable one of this core's private interrupts (SGI or PPI, 0-31).
/// Shared interrupts (SPIs) come with the first device that needs one.
pub fn enable(intid: u32) {
    let sgi = SGI_BASE.get().load(Ordering::Acquire);
    if sgi != 0 && intid < 32 {
        // Write-1-to-set: no read-modify-write, so no lock needed.
        write32(sgi + GICR_ISENABLER0, 1 << intid);
    }
}

/// Disable one of this core's private interrupts.
pub fn disable(intid: u32) {
    let sgi = SGI_BASE.get().load(Ordering::Acquire);
    if sgi != 0 && intid < 32 {
        write32(sgi + GICR_ICENABLER0, 1 << intid);
    }
}

/// Send SGI `intid` (0-15) to this core itself.
pub fn send_sgi_to_self(intid: u32) {
    let mpidr: u64;
    // SAFETY: reading MPIDR has no side effects.
    unsafe { core::arch::asm!("mrs {}, MPIDR_EL1", out(reg) mpidr) };
    send_sgi(mpidr, intid);
}

/// Send SGI `intid` (0-15) to the core with MPIDR affinity `mpidr`.
pub fn send_sgi(mpidr: u64, intid: u32) {
    let (aff0, aff1, aff2, aff3) = (mpidr & 0xff, (mpidr >> 8) & 0xff, (mpidr >> 16) & 0xff, (mpidr >> 32) & 0xff);
    // ICC_SGI1R: Aff3 [55:48], range selector [47:44] (Aff0 / 16), Aff2
    // [39:32], INTID [27:24], Aff1 [23:16], target list [15:0] (one bit
    // per Aff0 % 16).
    let value = (aff3 << 48) | ((aff0 / 16) << 44) | (aff2 << 32) | ((intid as u64 & 0xf) << 24) | (aff1 << 16) | (1 << (aff0 % 16));
    // SAFETY: sends one SGI.
    unsafe { core::arch::asm!("msr ICC_SGI1R_EL1, {}", "isb", in(reg) value) };
}

/// The interrupt the GIC is signalling, acknowledged (it becomes active).
/// 1020-1023 mean none.
pub fn acknowledge() -> u32 {
    let intid: u64;
    // SAFETY: reading IAR acknowledges the highest-priority pending interrupt.
    unsafe { core::arch::asm!("mrs {}, ICC_IAR1_EL1", out(reg) intid) };
    (intid & 0xff_ffff) as u32
}

/// Done with interrupt `intid`: drop the priority and deactivate it.
pub fn end_of_interrupt(intid: u32) {
    // SAFETY: `intid` came from `acknowledge`.
    unsafe { core::arch::asm!("msr ICC_EOIR1_EL1, {}", "isb", in(reg) intid as u64) };
}
