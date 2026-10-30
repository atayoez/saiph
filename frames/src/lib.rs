//! A physical frame allocator: one bit per 16 KiB frame (1 = in use).
//!
//! The kernel gives it a bitmap and the range of physical memory it covers,
//! marks the usable regions free, then allocates and frees single frames or
//! contiguous runs. It never touches the memory it manages, so it can be
//! unit-tested on a normal computer (`cargo test`).
//!
//! It needs no lock: every core can allocate and free at the same time.
//! The bitmap is made of atomic words. Taking a frame is one atomic "set
//! this bit" (`fetch_or`): the core that turns a bit from 0 to 1 owns the
//! frame, and a core that loses the race tries the next bit. Freeing is one
//! atomic "clear this bit". A contiguous run claims its bits one by one and
//! gives them back if another core got one of them first. (LLFree, a
//! lock-free allocator from 2023, builds on the same idea, adding per-core
//! reservations.)

#![no_std]

use core::sync::atomic::{AtomicU64, AtomicUsize, Ordering};

/// Size of one frame: Saiph's page size (see the `paging` crate).
pub const FRAME_SIZE: u64 = 16 * 1024;

#[derive(Debug, PartialEq, Eq)]
pub enum FreeError {
    /// Not a multiple of FRAME_SIZE.
    Misaligned,
    /// Outside the range this allocator manages.
    OutOfRange,
    /// The frame was not allocated (freed twice, or never handed out).
    NotAllocated,
}

pub struct FrameAllocator<'a> {
    /// Bit i describes the frame at `base + i * FRAME_SIZE`.
    bits: &'a [AtomicU64],
    base: u64,
    frames: usize,
    free: AtomicUsize,
    /// Word to start searching from, so allocations do not rescan the start.
    hint: AtomicUsize,
}

/// Number of `u64` words a bitmap for `frames` frames needs.
pub const fn words_for(frames: usize) -> usize {
    frames.div_ceil(64)
}

impl<'a> FrameAllocator<'a> {
    /// Manage `frames` frames starting at physical address `base` (which
    /// must be frame-aligned). Every frame starts out in use; call
    /// `mark_free` for the usable regions.
    pub fn new(bits: &'a [AtomicU64], base: u64, frames: usize) -> Self {
        assert!(base.is_multiple_of(FRAME_SIZE), "base must be frame-aligned");
        assert!(bits.len() >= words_for(frames), "bitmap too small");
        for word in bits {
            word.store(u64::MAX, Ordering::Relaxed);
        }
        FrameAllocator { bits, base, frames, free: AtomicUsize::new(0), hint: AtomicUsize::new(0) }
    }

    /// Free frames right now (other cores may change it any moment).
    pub fn free_frames(&self) -> usize {
        self.free.load(Ordering::Relaxed)
    }

    pub fn total_frames(&self) -> usize {
        self.frames
    }

    fn is_used(&self, i: usize) -> bool {
        self.bits[i / 64].load(Ordering::Relaxed) & (1 << (i % 64)) != 0
    }

    /// Set frame `i`'s bit; true if this call changed it from 0 to 1.
    fn claim(&self, i: usize) -> bool {
        let bit = 1 << (i % 64);
        let won = self.bits[i / 64].fetch_or(bit, Ordering::Acquire) & bit == 0;
        if won {
            self.free.fetch_sub(1, Ordering::Relaxed);
        }
        won
    }

    /// Clear frame `i`'s bit; true if this call changed it from 1 to 0.
    fn release(&self, i: usize) -> bool {
        let bit = 1 << (i % 64);
        let was_used = self.bits[i / 64].fetch_and(!bit, Ordering::Release) & bit != 0;
        if was_used {
            self.free.fetch_add(1, Ordering::Relaxed);
        }
        was_used
    }

    /// Frame indices covered by [start, start+len), clipped to our range.
    /// `inward` keeps only whole frames (for free memory); otherwise any
    /// partially covered frame counts (for memory that must not be used).
    fn frame_range(&self, start: u64, len: u64, inward: bool) -> core::ops::Range<usize> {
        let end = start.saturating_add(len);
        let (first, last) = if inward {
            (start.div_ceil(FRAME_SIZE) * FRAME_SIZE, end / FRAME_SIZE * FRAME_SIZE)
        } else {
            (start / FRAME_SIZE * FRAME_SIZE, end.div_ceil(FRAME_SIZE) * FRAME_SIZE)
        };
        let limit = self.base + self.frames as u64 * FRAME_SIZE;
        let (first, last) = (first.clamp(self.base, limit), last.clamp(self.base, limit));
        if first >= last {
            return 0..0;
        }
        ((first - self.base) / FRAME_SIZE) as usize..((last - self.base) / FRAME_SIZE) as usize
    }

    /// Mark the whole frames inside [start, start+len) as free.
    pub fn mark_free(&self, start: u64, len: u64) {
        for i in self.frame_range(start, len, true) {
            self.release(i);
        }
    }

    /// Mark every frame touching [start, start+len) as in use.
    pub fn mark_used(&self, start: u64, len: u64) {
        for i in self.frame_range(start, len, false) {
            self.claim(i);
        }
    }

    /// Allocate one frame; returns its physical address.
    pub fn alloc(&self) -> Option<u64> {
        let words = words_for(self.frames);
        let hint = self.hint.load(Ordering::Relaxed);
        for n in 0..words {
            let w = (hint + n) % words;
            // Another core may take the bit we pick; then look again.
            loop {
                let word = self.bits[w].load(Ordering::Relaxed);
                if word == u64::MAX {
                    break;
                }
                // The lowest zero bit of the word is the first free frame in it.
                let i = w * 64 + word.trailing_ones() as usize;
                if i >= self.frames {
                    break; // padding bits past the last frame
                }
                if self.claim(i) {
                    self.hint.store(w, Ordering::Relaxed);
                    return Some(self.base + i as u64 * FRAME_SIZE);
                }
            }
        }
        None
    }

    /// Allocate `count` physically contiguous frames whose first frame's
    /// address is a multiple of `align` frames (e.g. align = 2048 for 32 MiB).
    pub fn alloc_contiguous(&self, count: usize, align: usize) -> Option<u64> {
        assert!(count > 0 && align > 0);
        // Alignment is about physical addresses, so start from the first
        // frame index whose address is aligned.
        let align_bytes = align as u64 * FRAME_SIZE;
        let first = ((self.base.div_ceil(align_bytes) * align_bytes - self.base) / FRAME_SIZE) as usize;
        // Starts are `first`, `first + align`, `first + 2 * align`, ...
        let next_start_after = |j: usize| first + (j + 1 - first).div_ceil(align) * align;
        let mut i = first;
        while i + count <= self.frames {
            match (i..i + count).find(|&j| self.is_used(j)) {
                Some(used) => i = next_start_after(used),
                None => {
                    // Looks free: claim it frame by frame. If another core
                    // takes one first, give back what we took and go on.
                    match (i..i + count).find(|&j| !self.claim(j)) {
                        None => return Some(self.base + i as u64 * FRAME_SIZE),
                        Some(lost) => {
                            for j in i..lost {
                                self.release(j);
                            }
                            i = next_start_after(lost);
                        }
                    }
                }
            }
        }
        None
    }

    /// Give back one frame.
    pub fn free(&self, addr: u64) -> Result<(), FreeError> {
        if !addr.is_multiple_of(FRAME_SIZE) {
            return Err(FreeError::Misaligned);
        }
        if addr < self.base || addr >= self.base + self.frames as u64 * FRAME_SIZE {
            return Err(FreeError::OutOfRange);
        }
        let i = ((addr - self.base) / FRAME_SIZE) as usize;
        if !self.release(i) {
            return Err(FreeError::NotAllocated);
        }
        self.hint.fetch_min(i / 64, Ordering::Relaxed);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    extern crate std;
    use super::*;
        use std::vec::Vec;

    const BASE: u64 = 0x4000_0000;

    fn bitmap(frames: usize) -> Vec<AtomicU64> {
        (0..words_for(frames)).map(|_| AtomicU64::new(0)).collect()
    }

    fn allocator(bits: &[AtomicU64], frames: usize) -> FrameAllocator<'_> {
        let a = FrameAllocator::new(bits, BASE, frames);
        a.mark_free(BASE, frames as u64 * FRAME_SIZE);
        a
    }

    #[test]
    fn starts_all_used_until_marked_free() {
        let bits = bitmap(100);
        let a = FrameAllocator::new(&bits, BASE, 100);
        assert_eq!(a.free_frames(), 0);
    }

    #[test]
    fn alloc_and_free_round_trip() {
        let bits = bitmap(100);
        let a = allocator(&bits, 100);
        assert_eq!(a.free_frames(), 100);
        let x = a.alloc().unwrap();
        let y = a.alloc().unwrap();
        assert_ne!(x, y);
        assert_eq!(x % FRAME_SIZE, 0);
        assert_eq!(a.free_frames(), 98);
        a.free(x).unwrap();
        a.free(y).unwrap();
        assert_eq!(a.free_frames(), 100);
    }

    #[test]
    fn hands_out_every_frame_exactly_once() {
        let bits = bitmap(130);
        let a = allocator(&bits, 130); // not a multiple of 64
        let mut got: Vec<u64> = core::iter::from_fn(|| a.alloc()).collect();
        assert_eq!(got.len(), 130);
        got.sort();
        got.dedup();
        assert_eq!(got.len(), 130);
        assert!(got.iter().all(|&f| (BASE..BASE + 130 * FRAME_SIZE).contains(&f)));
        assert_eq!(a.alloc(), None);
    }

    #[test]
    fn detects_bad_frees() {
        let bits = bitmap(10);
        let a = allocator(&bits, 10);
        let x = a.alloc().unwrap();
        assert_eq!(a.free(x + 1), Err(FreeError::Misaligned));
        assert_eq!(a.free(BASE - FRAME_SIZE), Err(FreeError::OutOfRange));
        assert_eq!(a.free(BASE + 10 * FRAME_SIZE), Err(FreeError::OutOfRange));
        a.free(x).unwrap();
        assert_eq!(a.free(x), Err(FreeError::NotAllocated));
    }

    #[test]
    fn free_regions_round_inward_used_regions_outward() {
        let bits = bitmap(10);
        let a = FrameAllocator::new(&bits, BASE, 10);
        // 1.5 frames starting half-way into frame 0: only frame 1 is whole.
        a.mark_free(BASE + FRAME_SIZE / 2, FRAME_SIZE * 3 / 2);
        assert_eq!(a.free_frames(), 1);
        assert_eq!(a.alloc(), Some(BASE + FRAME_SIZE));
        let bits = bitmap(10);
        let a = allocator(&bits, 10);
        // One byte in frame 3 makes all of frame 3 unusable.
        a.mark_used(BASE + 3 * FRAME_SIZE + 100, 1);
        assert_eq!(a.free_frames(), 9);
    }

    #[test]
    fn regions_outside_the_range_are_ignored() {
        let bits = bitmap(10);
        let a = FrameAllocator::new(&bits, BASE, 10);
        a.mark_free(0, BASE + 2 * FRAME_SIZE); // overlaps the first two frames
        assert_eq!(a.free_frames(), 2);
        a.mark_free(BASE + 100 * FRAME_SIZE, FRAME_SIZE); // beyond the end
        assert_eq!(a.free_frames(), 2);
    }

    #[test]
    fn contiguous_runs_respect_alignment_and_holes() {
        let bits = bitmap(64);
        let a = allocator(&bits, 64);
        a.mark_used(BASE + 2 * FRAME_SIZE, FRAME_SIZE); // hole at frame 2
        let run = a.alloc_contiguous(4, 4).unwrap();
        assert_eq!(run, BASE + 4 * FRAME_SIZE); // frames 0-3 have the hole
        assert_eq!((run / FRAME_SIZE) % 4, 0);
        assert_eq!(a.free_frames(), 64 - 1 - 4);
        assert_eq!(a.alloc_contiguous(100, 1), None);
    }

    #[test]
    fn alignment_is_about_physical_addresses() {
        // Base not aligned to 4 frames: an aligned run must start at an
        // address that is, not at an index that is.
        let base = BASE + FRAME_SIZE;
        let bits = bitmap(32);
        let a = FrameAllocator::new(&bits, base, 32);
        a.mark_free(base, 32 * FRAME_SIZE);
        let run = a.alloc_contiguous(4, 4).unwrap();
        assert_eq!((run / FRAME_SIZE) % 4, 0);
    }

    #[test]
    fn skipping_a_hole_does_not_miss_the_next_aligned_start() {
        // base is one frame past a 4-frame boundary, so aligned runs start
        // at indices 3, 7, 11, ... With frame 4 in use, the first run that
        // fits starts at index 7.
        let base = BASE + FRAME_SIZE;
        let bits = bitmap(32);
        let a = FrameAllocator::new(&bits, base, 32);
        a.mark_free(base, 32 * FRAME_SIZE);
        a.mark_used(base + 4 * FRAME_SIZE, FRAME_SIZE);
        assert_eq!(a.alloc_contiguous(4, 4), Some(base + 7 * FRAME_SIZE));
    }

    #[test]
    fn freed_frames_are_found_again() {
        let bits = bitmap(200);
        let a = allocator(&bits, 200);
        let all: Vec<u64> = core::iter::from_fn(|| a.alloc()).collect();
        a.free(all[5]).unwrap();
        assert_eq!(a.alloc(), Some(all[5]));
    }

    #[test]
    fn cores_allocating_at_once_never_get_the_same_frame() {
        // Eight threads take frames as fast as they can, give half of them
        // back, and take more; every frame must end up with one owner.
        let bits = bitmap(4096);
        let a = allocator(&bits, 4096);
        let taken: Vec<Vec<u64>> = std::thread::scope(|s| {
            let workers: Vec<_> = (0..8)
                .map(|_| {
                    s.spawn(|| {
                        let mut mine: Vec<u64> = core::iter::from_fn(|| a.alloc()).take(400).collect();
                        let back: Vec<u64> = mine.drain(..200).collect();
                        for f in back {
                            a.free(f).unwrap();
                        }
                        mine.extend(core::iter::from_fn(|| a.alloc_contiguous(4, 4)).take(20).flat_map(|r| (0..4).map(move |k| r + k * FRAME_SIZE)));
                        mine
                    })
                })
                .collect();
            workers.into_iter().map(|w| w.join().unwrap()).collect()
        });
        let mut all: Vec<u64> = taken.into_iter().flatten().collect();
        let count = all.len();
        all.sort();
        all.dedup();
        assert_eq!(all.len(), count, "a frame was handed out twice");
        assert_eq!(a.free_frames(), 4096 - count);
    }
}
