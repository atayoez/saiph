//! A physical frame allocator: one bit per 16 KiB frame (1 = in use).
//!
//! The kernel gives it a bitmap and the range of physical memory it covers,
//! marks the usable regions free, then allocates and frees single frames or
//! contiguous runs. It never touches the memory it manages, so it can be
//! unit-tested on a normal computer (`cargo test`).

#![no_std]

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
    bits: &'a mut [u64],
    base: u64,
    frames: usize,
    free: usize,
    /// Word to start searching from, so allocations do not rescan the start.
    hint: usize,
}

/// Number of `u64` words a bitmap for `frames` frames needs.
pub const fn words_for(frames: usize) -> usize {
    frames.div_ceil(64)
}

impl<'a> FrameAllocator<'a> {
    /// Manage `frames` frames starting at physical address `base` (which
    /// must be frame-aligned). Every frame starts out in use; call
    /// `mark_free` for the usable regions.
    pub fn new(bits: &'a mut [u64], base: u64, frames: usize) -> Self {
        assert!(base % FRAME_SIZE == 0, "base must be frame-aligned");
        assert!(bits.len() >= words_for(frames), "bitmap too small");
        bits.fill(u64::MAX);
        FrameAllocator { bits, base, frames, free: 0, hint: 0 }
    }

    pub fn free_frames(&self) -> usize {
        self.free
    }

    pub fn total_frames(&self) -> usize {
        self.frames
    }

    fn is_used(&self, i: usize) -> bool {
        self.bits[i / 64] & (1 << (i % 64)) != 0
    }

    fn set(&mut self, i: usize, used: bool) {
        let (word, bit) = (i / 64, 1u64 << (i % 64));
        match (used, self.bits[word] & bit != 0) {
            (true, false) => {
                self.bits[word] |= bit;
                self.free -= 1;
            }
            (false, true) => {
                self.bits[word] &= !bit;
                self.free += 1;
            }
            _ => {}
        }
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
    pub fn mark_free(&mut self, start: u64, len: u64) {
        for i in self.frame_range(start, len, true) {
            self.set(i, false);
        }
    }

    /// Mark every frame touching [start, start+len) as in use.
    pub fn mark_used(&mut self, start: u64, len: u64) {
        for i in self.frame_range(start, len, false) {
            self.set(i, true);
        }
    }

    /// Allocate one frame; returns its physical address.
    pub fn alloc(&mut self) -> Option<u64> {
        let words = words_for(self.frames);
        for n in 0..words {
            let w = (self.hint + n) % words;
            if self.bits[w] == u64::MAX {
                continue;
            }
            // The lowest zero bit of the word is the first free frame in it.
            let i = w * 64 + self.bits[w].trailing_ones() as usize;
            if i >= self.frames {
                continue; // padding bits past the last frame
            }
            self.set(i, true);
            self.hint = w;
            return Some(self.base + i as u64 * FRAME_SIZE);
        }
        None
    }

    /// Allocate `count` physically contiguous frames whose first frame's
    /// address is a multiple of `align` frames (e.g. align = 2048 for 32 MiB).
    pub fn alloc_contiguous(&mut self, count: usize, align: usize) -> Option<u64> {
        assert!(count > 0 && align > 0);
        // Alignment is about physical addresses, so start from the first
        // frame index whose address is aligned.
        let align_bytes = align as u64 * FRAME_SIZE;
        let first = (self.base.div_ceil(align_bytes) * align_bytes - self.base) / FRAME_SIZE;
        let mut i = first as usize;
        while i + count <= self.frames {
            match (i..i + count).find(|&j| self.is_used(j)) {
                None => {
                    for j in i..i + count {
                        self.set(j, true);
                    }
                    return Some(self.base + i as u64 * FRAME_SIZE);
                }
                // Next aligned start after the used frame: starts are
                // `first`, `first + align`, `first + 2 * align`, ...
                Some(used) => i = first as usize + (used + 1 - first as usize).div_ceil(align) * align,
            }
        }
        None
    }

    /// Give back one frame.
    pub fn free(&mut self, addr: u64) -> Result<(), FreeError> {
        if addr % FRAME_SIZE != 0 {
            return Err(FreeError::Misaligned);
        }
        if addr < self.base || addr >= self.base + self.frames as u64 * FRAME_SIZE {
            return Err(FreeError::OutOfRange);
        }
        let i = ((addr - self.base) / FRAME_SIZE) as usize;
        if !self.is_used(i) {
            return Err(FreeError::NotAllocated);
        }
        self.set(i, false);
        if i / 64 < self.hint {
            self.hint = i / 64;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    extern crate std;
    use super::*;
    use std::vec;
    use std::vec::Vec;

    const BASE: u64 = 0x4000_0000;

    fn allocator(bits: &mut [u64], frames: usize) -> FrameAllocator<'_> {
        let mut a = FrameAllocator::new(bits, BASE, frames);
        a.mark_free(BASE, frames as u64 * FRAME_SIZE);
        a
    }

    #[test]
    fn starts_all_used_until_marked_free() {
        let mut bits = vec![0; words_for(100)];
        let a = FrameAllocator::new(&mut bits, BASE, 100);
        assert_eq!(a.free_frames(), 0);
    }

    #[test]
    fn alloc_and_free_round_trip() {
        let mut bits = vec![0; words_for(100)];
        let mut a = allocator(&mut bits, 100);
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
        let mut bits = vec![0; words_for(130)];
        let mut a = allocator(&mut bits, 130); // not a multiple of 64
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
        let mut bits = vec![0; words_for(10)];
        let mut a = allocator(&mut bits, 10);
        let x = a.alloc().unwrap();
        assert_eq!(a.free(x + 1), Err(FreeError::Misaligned));
        assert_eq!(a.free(BASE - FRAME_SIZE), Err(FreeError::OutOfRange));
        assert_eq!(a.free(BASE + 10 * FRAME_SIZE), Err(FreeError::OutOfRange));
        a.free(x).unwrap();
        assert_eq!(a.free(x), Err(FreeError::NotAllocated));
    }

    #[test]
    fn free_regions_round_inward_used_regions_outward() {
        let mut bits = vec![0; words_for(10)];
        let mut a = FrameAllocator::new(&mut bits, BASE, 10);
        // 1.5 frames starting half-way into frame 0: only frame 1 is whole.
        a.mark_free(BASE + FRAME_SIZE / 2, FRAME_SIZE * 3 / 2);
        assert_eq!(a.free_frames(), 1);
        assert_eq!(a.alloc(), Some(BASE + FRAME_SIZE));
        let mut bits = vec![0; words_for(10)];
        let mut a = allocator(&mut bits, 10);
        // One byte in frame 3 makes all of frame 3 unusable.
        a.mark_used(BASE + 3 * FRAME_SIZE + 100, 1);
        assert_eq!(a.free_frames(), 9);
    }

    #[test]
    fn regions_outside_the_range_are_ignored() {
        let mut bits = vec![0; words_for(10)];
        let mut a = FrameAllocator::new(&mut bits, BASE, 10);
        a.mark_free(0, BASE + 2 * FRAME_SIZE); // overlaps the first two frames
        assert_eq!(a.free_frames(), 2);
        a.mark_free(BASE + 100 * FRAME_SIZE, FRAME_SIZE); // beyond the end
        assert_eq!(a.free_frames(), 2);
    }

    #[test]
    fn contiguous_runs_respect_alignment_and_holes() {
        let mut bits = vec![0; words_for(64)];
        let mut a = allocator(&mut bits, 64);
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
        let mut bits = vec![0; words_for(32)];
        let mut a = FrameAllocator::new(&mut bits, base, 32);
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
        let mut bits = vec![0; words_for(32)];
        let mut a = FrameAllocator::new(&mut bits, base, 32);
        a.mark_free(base, 32 * FRAME_SIZE);
        a.mark_used(base + 4 * FRAME_SIZE, FRAME_SIZE);
        assert_eq!(a.alloc_contiguous(4, 4), Some(base + 7 * FRAME_SIZE));
    }

    #[test]
    fn freed_frames_are_found_again() {
        let mut bits = vec![0; words_for(200)];
        let mut a = allocator(&mut bits, 200);
        let all: Vec<u64> = core::iter::from_fn(|| a.alloc()).collect();
        a.free(all[5]).unwrap();
        assert_eq!(a.alloc(), Some(all[5]));
    }
}
