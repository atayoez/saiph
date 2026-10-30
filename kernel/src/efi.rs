//! Calling the UEFI firmware after boot (its "runtime services"), to save
//! the boot log in UEFI variables. Linux shows those as files in
//! /sys/firmware/efi/efivars, and `cargo board log` prints them. Linux saves
//! its own crash logs the same way (efi-pstore).
//!
//! The firmware stays in memory after boot. Nobody moved it (Limine does not
//! call SetVirtualAddressMap), so its code and data still expect to run at
//! their physical addresses. So, like Linux, Saiph gives it tables of its
//! own for the lower half, mapping every runtime region of the EFI memory
//! map 1:1 (with 4 KiB pages: the regions are 4 KiB-aligned), plus a page
//! for the call's arguments. For each call it switches the lower half on
//! with those tables, masks interrupts and allows the FP unit (firmware may
//! use it), calls, and switches everything back.
//!
//! The firmware's code regions are mapped writable and executable: without
//! the firmware's memory attributes table there is no telling which parts
//! of them are data. These tables are only in use during a call.

use crate::console;
use crate::memory;
use crate::sync::{IrqsOff, SpinLock, order};
use core::fmt::{self, Write};
use core::sync::atomic::{AtomicBool, Ordering};
use limine::request::{EfiMemmapRequest, EfiRequest};

#[used]
#[unsafe(link_section = ".requests")]
static SYSTEM_TABLE: EfiRequest = EfiRequest::new();

#[used]
#[unsafe(link_section = ".requests")]
static EFI_MEMMAP: EfiMemmapRequest = EfiMemmapRequest::new();

/// Saiph's GUID for its variables (the "vendor" part of their names):
/// 85a3298b-98bf-462c-8985-29bb1d4e9363, in UEFI's byte order.
const GUID: [u8; 16] = [0x8b, 0x29, 0xa3, 0x85, 0xbf, 0x98, 0x2c, 0x46, 0x89, 0x85, 0x29, 0xbb, 0x1d, 0x4e, 0x93, 0x63];
/// The log goes into SaiphLog00, SaiphLog01, ... of at most CHUNK bytes,
/// small enough for any firmware's variable size limit.
const CHUNK: usize = 4000;
const MAX_CHUNKS: usize = 16;
/// Non-volatile, readable during boot and by the OS.
const ATTRIBUTES: u32 = 0x7;

// EFI memory map: descriptor types and attributes.
const RUNTIME_CODE: u32 = 5;
const MMIO: u32 = 11;
const MMIO_PORT: u32 = 12;
const ATTR_WB: u64 = 1 << 3;
const ATTR_RUNTIME: u64 = 1 << 63;

// 4 KiB-granule descriptors (as in mmu.rs).
const PAGE: u64 = 4096;
const VALID: u64 = 1;
const TABLE_OR_PAGE: u64 = 1 << 1;
const NORMAL: u64 = 3 << 8; // MAIR slot 0 (attribute bits 0), inner shareable
const DEVICE: u64 = 2 << 2; // MAIR slot 2: Device-nGnRnE (see mmu.rs)
const ACCESS_FLAG: u64 = 1 << 10;
const PXN: u64 = 1 << 53;
const UXN: u64 = 1 << 54;

/// UEFI status codes: 0 is success, errors have the top bit set.
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct Status(pub u64);

impl fmt::Display for Status {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let name = match self.0 & !(1 << 63) {
            0 => "success",
            2 => "invalid parameter",
            3 => "unsupported",
            8 => "write protected",
            9 => "out of resources",
            14 => "not found",
            _ => "error",
        };
        write!(f, "{name} ({:#x})", self.0)
    }
}

/// No firmware to call ("unsupported").
const NO_FIRMWARE: Status = Status(1 << 63 | 3);

/// The firmware's lower-half tables and the argument page.
struct Firmware {
    root: u64,
    hhdm: u64,
    /// A frame being cut into 4 KiB tables, and how many are used.
    spare: (u64, usize),
    /// The argument page: physical address = its address during a call.
    args: u64,
    system_table: u64,
}

/// One call at a time, with interrupts masked.
static FIRMWARE: SpinLock<Option<Firmware>, IrqsOff, order::Firmware> = SpinLock::new(None);

impl Firmware {
    fn alloc_table(&mut self) -> Option<u64> {
        if self.spare.0 == 0 || self.spare.1 == 4 {
            let frame = memory::alloc_frame()?;
            // SAFETY: a fresh 16 KiB frame, reached through the direct map.
            unsafe { core::ptr::write_bytes((frame + self.hhdm) as *mut u8, 0, 4 * PAGE as usize) };
            self.spare = (frame, 0);
        }
        self.spare.1 += 1;
        Some(self.spare.0 + (self.spare.1 as u64 - 1) * PAGE)
    }

    fn entry(&self, table: u64, index: usize) -> *mut u64 {
        (table + self.hhdm + index as u64 * 8) as *mut u64
    }

    /// Map the 4 KiB page at physical `pa` at the same address.
    fn map(&mut self, pa: u64, attributes: u64) -> Option<()> {
        let mut table = self.root;
        for level in 0..3 {
            let slot = self.entry(table, ((pa >> (39 - 9 * level)) & 511) as usize);
            // SAFETY: `slot` is inside one of our tables.
            let d = unsafe { slot.read() };
            table = if d & VALID == 0 {
                let new = self.alloc_table()?;
                // SAFETY: as above.
                unsafe { slot.write(new | TABLE_OR_PAGE | VALID) };
                new
            } else {
                d & 0x0000_ffff_ffff_f000
            };
        }
        let slot = self.entry(table, ((pa >> 12) & 511) as usize);
        // SAFETY: as above. A page already mapped keeps its first mapping.
        unsafe {
            if slot.read() & VALID == 0 {
                slot.write(pa | attributes | ACCESS_FLAG | TABLE_OR_PAGE | VALID);
            }
        }
        Some(())
    }

    /// Build the tables from the EFI memory map.
    fn new(hhdm: u64) -> Option<Firmware> {
        let system_table = SYSTEM_TABLE.response()?.address as u64;
        let memmap = EFI_MEMMAP.response()?;
        let mut fw = Firmware { root: 0, hhdm, spare: (0, 0), args: 0, system_table: 0 };
        fw.root = fw.alloc_table()?;
        // Base revision 6 hands the system table over as a direct-map address.
        fw.system_table = system_table.checked_sub(hhdm).unwrap_or(system_table);
        let map = memmap.memmap();
        for d in map.chunks_exact(memmap.desc_size as usize) {
            let kind = u32::from_le_bytes(d[0..4].try_into().unwrap());
            let start = u64::from_le_bytes(d[8..16].try_into().unwrap());
            let pages = u64::from_le_bytes(d[24..32].try_into().unwrap());
            let attr = u64::from_le_bytes(d[32..40].try_into().unwrap());
            if attr & ATTR_RUNTIME == 0 {
                continue;
            }
            let attributes = match kind {
                RUNTIME_CODE => NORMAL | UXN,
                MMIO | MMIO_PORT => DEVICE | PXN | UXN,
                _ if attr & ATTR_WB == 0 => DEVICE | PXN | UXN,
                _ => NORMAL | PXN | UXN,
            };
            for page in 0..pages {
                fw.map(start + page * PAGE, attributes)?;
            }
        }
        let args = memory::alloc_frame()?;
        for page in 0..paging::PAGE_SIZE / PAGE {
            fw.map(args + page * PAGE, NORMAL | PXN | UXN)?;
        }
        fw.args = args;
        Some(fw)
    }

    /// Run `call` with the firmware's tables in the lower half.
    fn call<R>(&self, call: impl FnOnce() -> R) -> R {
        let (ttbr0, tcr, cpacr): (u64, u64, u64);
        // SAFETY: the lower half shows the firmware's regions 1:1 during
        // the call, then goes back to how it was (off); interrupts are
        // masked (FIRMWARE is an IrqsOff lock). FPEN (CPACR bits 21:20)
        // lets the firmware use FP/SIMD registers, which Saiph never does.
        unsafe {
            core::arch::asm!("mrs {}, TTBR0_EL1", "mrs {}, TCR_EL1", "mrs {}, CPACR_EL1",
                out(reg) ttbr0, out(reg) tcr, out(reg) cpacr);
            core::arch::asm!(
                "dsb ishst",
                "msr TTBR0_EL1, {root}",
                "msr TCR_EL1, {tcr}",
                "msr CPACR_EL1, {cpacr}",
                "isb",
                "tlbi vmalle1",
                "dsb ish",
                "isb",
                root = in(reg) self.root,
                tcr = in(reg) tcr & !(1 << 7),
                cpacr = in(reg) cpacr | 3 << 20,
            );
        }
        let result = call();
        // SAFETY: restores what was there before.
        unsafe {
            core::arch::asm!(
                "msr TTBR0_EL1, {ttbr0}",
                "msr TCR_EL1, {tcr}",
                "msr CPACR_EL1, {cpacr}",
                "isb",
                "tlbi vmalle1",
                "dsb ish",
                "isb",
                ttbr0 = in(reg) ttbr0,
                tcr = in(reg) tcr,
                cpacr = in(reg) cpacr,
            );
        }
        result
    }

    /// The runtime-services function at byte `offset` in the table (88 =
    /// SetVariable, 72 = GetVariable). Only valid inside `call`.
    fn function(&self, offset: u64) -> Option<u64> {
        // SAFETY: inside `call`, the system table and the runtime services
        // table (runtime data) are mapped at their physical addresses.
        unsafe {
            let runtime = ((self.system_table + 88) as *const u64).read_volatile();
            if runtime == 0 {
                return None;
            }
            let f = ((runtime + offset) as *const u64).read_volatile();
            (f != 0).then_some(f)
        }
    }
}

type SetVariable = unsafe extern "C" fn(*const u16, *const [u8; 16], u32, usize, *const u8) -> u64;
type GetVariable = unsafe extern "C" fn(*const u16, *const [u8; 16], *mut u32, *mut usize, *mut u8) -> u64;

// Where the arguments go in the argument page.
const ARG_GUID: u64 = 0;
const ARG_NAME: u64 = 64;
const ARG_SIZE: u64 = 128;
const ARG_DATA: u64 = 256;
const ARG_BACK: u64 = ARG_DATA + 4096;

/// What `save_log` did: bytes saved, in how many variables, and whether the
/// first one read back the same.
pub struct Saved {
    pub bytes: usize,
    pub chunks: usize,
    pub read_back: bool,
}

/// Build the firmware's tables (after `vm::switch_to`, before anything may
/// need to save the log). `hhdm` is Limine's direct-map offset. False if
/// Limine passed on no EFI system table or memory map, or memory ran out.
pub fn init(hhdm: u64) -> bool {
    let firmware = Firmware::new(hhdm);
    let ok = firmware.is_some();
    *FIRMWARE.lock() = firmware;
    ok
}

/// Save the console log into UEFI variables SaiphLog00, SaiphLog01, ...,
/// and delete the ones a longer earlier log left.
pub fn save_log() -> Result<Saved, Status> {
    let firmware = FIRMWARE.lock();
    save(firmware.as_ref().ok_or(NO_FIRMWARE)?)
}

/// After a fault report or a panic: save the log once, if the firmware is
/// not in the middle of a call already (the crash may have happened in
/// one), and say how it went.
pub fn save_log_after_crash(out: &mut impl Write) {
    static TRIED: AtomicBool = AtomicBool::new(false);
    if TRIED.swap(true, Ordering::Relaxed) {
        return;
    }
    let Some(firmware) = FIRMWARE.try_lock() else { return };
    let Some(fw) = firmware.as_ref() else { return };
    match save(fw) {
        Ok(saved) => {
            let _ = writeln!(out, "    log saved in {} UEFI variables (`cargo board log` reads it)", saved.chunks);
        }
        Err(e) => {
            let _ = writeln!(out, "    log not saved: {e}");
        }
    }
}

fn save(fw: &Firmware) -> Result<Saved, Status> {
    let hhdm = fw.hhdm;
    let range = console::log_range();
    let chunks = range.len().div_ceil(CHUNK).min(MAX_CHUNKS);
    let args = fw.args;
    let arg = |offset: u64| (args + hhdm + offset) as *mut u8;

    // SAFETY: the argument page is ours, reached through the direct map.
    unsafe { core::ptr::copy_nonoverlapping(GUID.as_ptr(), arg(ARG_GUID), 16) };
    let set_name = |n: usize| {
        let name: [u8; 10] = *b"SaiphLog00";
        let digits = [b'0' + (n / 10) as u8, b'0' + (n % 10) as u8];
        let mut utf16 = [0u16; 11];
        for (i, &c) in name[..8].iter().chain(&digits).enumerate() {
            utf16[i] = c as u16;
        }
        // SAFETY: as above; 22 bytes fit before ARG_SIZE.
        unsafe { core::ptr::copy_nonoverlapping(utf16.as_ptr().cast::<u8>(), arg(ARG_NAME), 22) };
    };

    fw.call(|| {
        let set: SetVariable = match fw.function(88) {
            // SAFETY: the firmware's SetVariable, at its physical address.
            Some(f) => unsafe { core::mem::transmute::<u64, SetVariable>(f) },
            None => return Err(NO_FIRMWARE),
        };
        let guid = (args + ARG_GUID) as *const [u8; 16];
        let name = (args + ARG_NAME) as *const u16;
        for n in 0..MAX_CHUNKS {
            set_name(n);
            let from = range.start + n * CHUNK;
            let len = range.end.saturating_sub(from).min(CHUNK);
            for i in 0..len {
                // SAFETY: as above; CHUNK bytes fit before ARG_BACK.
                unsafe { arg(ARG_DATA).add(i).write(console::log_byte(from + i)) };
            }
            // Size 0 deletes the variable (fine if it does not exist).
            // SAFETY: a runtime service with valid arguments at their
            // physical addresses.
            let status = Status(unsafe { set(name, guid, ATTRIBUTES, len, (args + ARG_DATA) as *const u8) });
            if n < chunks && status.0 != 0 {
                return Err(status);
            }
        }
        // Read the first one back.
        let read_back = match fw.function(72) {
            Some(f) => {
                // SAFETY: the firmware's GetVariable, at its physical address.
                let get = unsafe { core::mem::transmute::<u64, GetVariable>(f) };
                set_name(0);
                let size = (args + ARG_SIZE) as *mut usize;
                let mut attributes = 0u32;
                // SAFETY: as for SetVariable; the buffer has 4096 bytes.
                unsafe {
                    size.write(4096);
                    let status = get(name, guid, &mut attributes, size, (args + ARG_BACK) as *mut u8);
                    let len = range.len().min(CHUNK);
                    status == 0
                        && size.read() == len
                        && (0..len).all(|i| arg(ARG_BACK).add(i).read() == console::log_byte(range.start + i))
                }
            }
            None => false,
        };
        Ok(Saved { bytes: range.len(), chunks, read_back })
    })
}
