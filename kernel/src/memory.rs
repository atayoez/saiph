//! Physical memory: Limine's memory map and the frame allocator.

use core::fmt::Write;
use frames::{FRAME_SIZE, FrameAllocator, words_for};
use limine::memmap::{self, Entry};
use limine::request::MemmapRequest;
use spin::Mutex;

/// The frame allocator, once `init` has set it up. The page tables (`vm`)
/// and the heap take their frames from here.
pub static FRAMES: Mutex<Option<FrameAllocator<'static>>> = Mutex::new(None);

/// One free frame, if there is one.
pub fn alloc_frame() -> Option<u64> {
    FRAMES.lock().as_mut()?.alloc()
}

/// Give back a frame from `alloc_frame`.
pub fn free_frame(frame: u64) {
    if let Some(fa) = FRAMES.lock().as_mut() {
        let _ = fa.free(frame);
    }
}

#[used]
#[unsafe(link_section = ".requests")]
static MEMMAP: MemmapRequest = MemmapRequest::new();

/// Limine's memory map (empty if Limine sent none).
pub fn entries() -> &'static [&'static Entry] {
    MEMMAP.response().map_or(&[], |r| r.entries())
}

fn type_name(t: u64) -> &'static str {
    match t {
        memmap::MEMMAP_USABLE => "usable",
        memmap::MEMMAP_RESERVED => "reserved",
        memmap::MEMMAP_ACPI_RECLAIMABLE => "ACPI reclaimable",
        memmap::MEMMAP_ACPI_NVS => "ACPI NVS",
        memmap::MEMMAP_BAD_MEMORY => "bad memory",
        memmap::MEMMAP_BOOTLOADER_RECLAIMABLE => "bootloader reclaimable",
        memmap::MEMMAP_EXECUTABLE_AND_MODULES => "kernel and modules",
        memmap::MEMMAP_FRAMEBUFFER => "framebuffer",
        memmap::MEMMAP_MAPPED_RESERVED => "reserved (mapped)",
        _ => "unknown",
    }
}

/// Print the memory map by type, then set up the frame allocator (`FRAMES`)
/// over all usable and bootloader-reclaimable memory (the latter stays
/// marked in use until we stop needing Limine's page tables and stack).
/// False if Limine sent no usable memory map.
pub fn init(out: &mut impl Write, hhdm: u64) -> bool {
    set_up(out, hhdm).is_some()
}

fn set_up(out: &mut impl Write, hhdm: u64) -> Option<()> {
    let entries: &[&Entry] = MEMMAP.response()?.entries();

    let _ = writeln!(out, "  memory map: {} regions", entries.len());
    for t in 0..=memmap::MEMMAP_MAPPED_RESERVED {
        let (n, bytes) = entries.iter().filter(|e| e.type_ == t).fold((0, 0), |(n, b), e| (n + 1, b + e.length));
        if n > 0 {
            let _ = writeln!(out, "    {:<24} {:>3} regions  {:>7} KiB", type_name(t), n, bytes / 1024);
        }
    }

    // The allocator covers frames from the lowest to the highest address of
    // memory it may ever hand out.
    let managed = |e: &&&Entry| matches!(e.type_, memmap::MEMMAP_USABLE | memmap::MEMMAP_BOOTLOADER_RECLAIMABLE);
    let low = entries.iter().filter(managed).map(|e| e.base).min()? / FRAME_SIZE * FRAME_SIZE;
    let high = entries.iter().filter(managed).map(|e| e.base + e.length).max()?;
    let frames = (high - low).div_ceil(FRAME_SIZE) as usize;

    // The bitmap goes at the start of the first usable region large enough.
    let bytes = (words_for(frames) * 8) as u64;
    let home = entries.iter().find(|e| e.type_ == memmap::MEMMAP_USABLE && e.base % 8 == 0 && e.length >= bytes)?;
    // SAFETY: usable RAM, mapped by the HHDM, not used by anything else yet;
    // the allocator marks these frames in use below before handing any out.
    let bits = unsafe { core::slice::from_raw_parts_mut((home.base + hhdm) as *mut u64, words_for(frames)) };

    let mut fa = FrameAllocator::new(bits, low, frames);
    for e in entries.iter().filter(|e| e.type_ == memmap::MEMMAP_USABLE) {
        fa.mark_free(e.base, e.length);
    }
    fa.mark_used(home.base, bytes);
    let _ = writeln!(out, "  frame allocator: {low:#x}..{high:#x} ({frames} frames), bitmap {} KiB at {:#x}, {} frames ({} MiB) free",
        bytes.div_ceil(1024), home.base, fa.free_frames(), fa.free_frames() as u64 * FRAME_SIZE >> 20);
    *FRAMES.lock() = Some(fa);
    Some(())
}

/// Allocate a few frames, write and read them through the direct map, give
/// them back, and check that nothing went missing.
pub fn self_test(out: &mut impl Write, hhdm: u64) -> bool {
    let mut frames = FRAMES.lock();
    let Some(fa) = frames.as_mut() else { return false };
    let before = fa.free_frames();
    let mut ok = true;
    let mut singles = [0u64; 3];
    for (n, slot) in singles.iter_mut().enumerate() {
        let Some(frame) = fa.alloc() else { return false };
        *slot = frame;
        let p = (frame + hhdm) as *mut u64;
        // SAFETY: the frame was just allocated to us and is mapped by the HHDM.
        unsafe {
            p.write_volatile(0x5eed_0000 + n as u64);
            ok &= p.read_volatile() == 0x5eed_0000 + n as u64;
        }
    }
    // 16 contiguous frames on a 256 KiB boundary.
    let run = fa.alloc_contiguous(16, 16);
    ok &= run.is_some_and(|r| r % (16 * FRAME_SIZE) == 0);
    for f in singles {
        ok &= fa.free(f).is_ok();
    }
    if let Some(r) = run {
        for i in 0..16 {
            ok &= fa.free(r + i * FRAME_SIZE).is_ok();
        }
    }
    ok &= fa.free_frames() == before;
    let _ = writeln!(out, "  frame allocator self-test: {}", if ok { "ok" } else { "FAILED" });
    ok
}
