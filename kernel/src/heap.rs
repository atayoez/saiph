//! The kernel heap: memory for `Box`, `Vec`, `String` and the rest of the
//! `alloc` crate.
//!
//! talc does the allocating. Its memory is the heap region, a range of
//! virtual addresses of its own starting at `HEAP_BASE`. Whenever talc runs
//! out, `Pages` takes frames from the frame allocator and maps them at the
//! region's end, at least 2 MiB at a time. The heap is one growing range of
//! addresses, while its frames can come from anywhere in RAM; past its end
//! nothing is mapped, so running off it faults. It never shrinks yet.
//!
//! It works once the kernel runs on its own page tables (`vm::switch_to`);
//! before that, every allocation fails.

use crate::{memory, vm};
use core::alloc::Layout;
use core::fmt::Write;
use core::ptr::NonNull;
use paging::{Kind, PAGE_SIZE};
use spin::mutex::SpinMutex;
use talc::TalcLock;
use talc::base::Talc;
use talc::base::binning::Binning;
use talc::source::Source;

/// Where the heap region starts: above the device window (which ends at
/// 0xffff_c100_0000_0000 for the O6's 40-bit physical addresses) and below
/// the kernel image.
pub const HEAP_BASE: u64 = 0xffff_d000_0000_0000;
/// How large the heap may grow (1 TiB of addresses).
const HEAP_MAX: u64 = 1 << 40;
/// How much the heap grows at least each time talc runs out.
const GROW: u64 = 2 << 20;

#[global_allocator]
static HEAP: TalcLock<SpinMutex<()>, Pages> = TalcLock::new(Pages { mapped: 0, end: 0 });

/// talc's source of memory: pages mapped at the end of the heap region.
#[derive(Debug)]
pub struct Pages {
    /// Bytes mapped at `HEAP_BASE` so far.
    mapped: u64,
    /// The end of talc's heap, 0 before talc has one.
    end: usize,
}

// SAFETY: `acquire` never allocates from the heap: it only uses the frame
// allocator and the page tables, and neither of them uses the heap.
unsafe impl Source for Pages {
    fn acquire<B: Binning>(talc: &mut Talc<Self, B>, layout: Layout) -> Result<(), ()> {
        // Room for the request and talc's bookkeeping, in whole steps.
        let want = (layout.size() as u64 + layout.align() as u64 + PAGE_SIZE).next_multiple_of(GROW);
        let start = HEAP_BASE + talc.source.mapped;
        let want = want.min(HEAP_MAX - talc.source.mapped);
        let mut got = 0;
        while got < want {
            let Some(frame) = memory::alloc_frame() else { break };
            if vm::map(start + got, frame, PAGE_SIZE, Kind::ReadWrite).is_err() {
                memory::free_frame(frame);
                break;
            }
            got += PAGE_SIZE;
        }
        if got == 0 {
            return Err(());
        }
        talc.source.mapped += got;
        let mapped = talc.source.mapped;

        // SAFETY: the pages up to `HEAP_BASE + mapped` are mapped for the
        // heap alone, and are never unmapped.
        let end = unsafe {
            match NonNull::new(talc.source.end as *mut u8) {
                None => talc.claim(HEAP_BASE as *mut u8, mapped as usize).ok_or(())?,
                Some(end) => talc.extend(end, (HEAP_BASE + mapped) as *mut u8),
            }
        };
        talc.source.end = end.as_ptr() as usize;
        Ok(())
    }
}

/// Bytes mapped for the heap so far.
fn mapped() -> u64 {
    HEAP.lock().source.mapped
}

/// Allocate, check and free a few things, among them more than one growth
/// step, and check that freed memory is used again.
pub fn self_test(out: &mut impl Write) -> bool {
    use alloc::{boxed::Box, format, vec::Vec};

    let boxed = Box::new(0x5eed_u64);
    let mut ok = *boxed == 0x5eed;

    // 3 MiB, pushed one number at a time: the vector moves to larger and
    // larger buffers, and the heap has to grow past its first step.
    let count = (3 << 20) / 8;
    let mut numbers = Vec::new();
    for i in 0..count {
        numbers.push(i as u64);
    }
    ok &= numbers.iter().enumerate().all(|(i, &n)| n == i as u64);
    ok &= mapped() > GROW;
    drop(numbers);

    // The same amount again fits into what was just freed.
    let before = mapped();
    let again: Vec<u64> = Vec::with_capacity(count);
    ok &= again.capacity() >= count && mapped() == before;
    drop(again);

    let text = format!("{} + {} = {}", 2, 3, 2 + 3);
    ok &= text == "2 + 3 = 5";
    drop(boxed);

    let _ = writeln!(out, "  heap: at {HEAP_BASE:#x}, grown to {} KiB; self-test: {}",
        mapped() / 1024, if ok { "ok" } else { "FAILED" });
    ok
}
