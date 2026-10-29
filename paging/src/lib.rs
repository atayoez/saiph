//! Building AArch64 stage-1 page tables (16 KiB granule, 48-bit addresses).
//!
//! A virtual address is translated in up to four steps (levels 0..3). Each
//! table is one 16 KiB page of 2048 eight-byte descriptors, indexed by 11
//! address bits; level 0, at the top, only has bit 47 left over, so it uses
//! just two entries. A level-2 descriptor can map a whole 32 MiB block
//! instead of pointing to a level-3 table; a level-3 descriptor maps a
//! 16 KiB page. (With 4 KiB pages it would be 512 entries, 9 bits per
//! level and 2 MiB blocks.)
//!
//! Tables live in physical memory. `TableMemory` abstracts how to get a new
//! zeroed table and how to reach a table by its physical address, so the
//! kernel can back it with frames and the direct map, and unit tests with
//! ordinary heap memory (`cargo test`).

#![no_std]

pub const PAGE_SIZE: u64 = 16 * 1024;
pub const BLOCK_SIZE: u64 = 32 << 20;
/// Descriptors per table.
pub const ENTRIES: usize = 2048;

const VALID: u64 = 1 << 0;
/// Below level 3: "points to a table". At level 3: "maps a page".
const TABLE_OR_PAGE: u64 = 1 << 1;
/// AP[2]: read-only. AP[1] (bit 6), "user code may access", stays 0.
const READ_ONLY: u64 = 1 << 7;
/// Inner shareable: kept coherent between all cores.
const INNER_SHAREABLE: u64 = 3 << 8;
/// Access flag: without it, the first access faults.
const ACCESS_FLAG: u64 = 1 << 10;
/// Guarded page: BTI checks apply.
const GUARDED: u64 = 1 << 50;
/// Never executable by the kernel (privileged) / by user code.
const PXN: u64 = 1 << 53;
const UXN: u64 = 1 << 54;
/// Output address (physical address) bits of a descriptor: 47..14.
const ADDRESS: u64 = 0x0000_ffff_ffff_c000;

/// MAIR slots, as Limine sets them up (0, 1) plus the one we add (2).
pub const ATTR_NORMAL: u64 = 0;
pub const ATTR_FRAMEBUFFER: u64 = 1;
pub const ATTR_DEVICE: u64 = 2;

/// What a mapping is for; each kind has fixed permissions. No kind is both
/// writable and executable (W^X).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    /// Kernel code: read-only, executable, BTI-guarded.
    Code,
    /// Kernel code compiled without BTI landing pads: read-only and
    /// executable but not guarded, since on a guarded page its first
    /// indirect call would fault.
    UnguardedCode,
    /// Read-only data.
    ReadOnly,
    /// Read-write data.
    ReadWrite,
    /// Device registers: read-write, uncached (Device-nGnRnE).
    Device,
    /// The framebuffer, with the memory type Limine chose for it.
    Framebuffer,
}

impl Kind {
    fn attributes(self) -> u64 {
        let normal = (ATTR_NORMAL << 2) | INNER_SHAREABLE;
        ACCESS_FLAG
            | UXN
            | match self {
                Kind::Code => normal | READ_ONLY | GUARDED,
                Kind::UnguardedCode => normal | READ_ONLY,
                Kind::ReadOnly => normal | READ_ONLY | PXN,
                Kind::ReadWrite => normal | PXN,
                Kind::Device => (ATTR_DEVICE << 2) | PXN,
                Kind::Framebuffer => (ATTR_FRAMEBUFFER << 2) | INNER_SHAREABLE | PXN,
            }
    }
}

/// True if a leaf descriptor allows the kernel to both write and execute.
pub fn writable_and_executable(descriptor: u64) -> bool {
    descriptor & READ_ONLY == 0 && descriptor & PXN == 0
}

pub trait TableMemory {
    /// A new, zeroed table (one page); returns its physical address.
    fn alloc_table(&mut self) -> Option<u64>;
    /// The table at physical address `phys`.
    fn table(&mut self, phys: u64) -> &mut [u64; ENTRIES];
}

#[derive(Debug, PartialEq, Eq)]
pub enum MapError {
    /// An address or length is not a multiple of the page size.
    Misaligned,
    /// Something is already mapped at this virtual address.
    AlreadyMapped(u64),
    /// No memory for another table.
    OutOfMemory,
}

/// Lowest address bit a descriptor at `level` translates.
fn shift(level: u32) -> u32 {
    14 + 11 * (3 - level)
}

/// Index into the table at `level` for virtual address `va`. Level 0 only
/// has bit 47 (the bits above it are copies of it).
fn index(va: u64, level: u32) -> usize {
    let mask = if level == 0 { 1 } else { ENTRIES as u64 - 1 };
    ((va >> shift(level)) & mask) as usize
}

/// Bytes one descriptor covers at `level` (level 1: 64 GiB, 2: 32 MiB, 3: 16 KiB).
fn span(level: u32) -> u64 {
    1 << shift(level)
}

/// Upper-half addresses (bit 47 set) have their top 16 bits set too.
fn canonical(va: u64) -> u64 {
    if va & (1 << 47) != 0 { va | 0xffff_0000_0000_0000 } else { va }
}

pub struct Mapper<M> {
    /// Physical address of the level-0 table (what goes into TTBR).
    pub root: u64,
    pub mem: M,
    /// Tables allocated so far, including the root.
    pub tables: usize,
}

impl<M: TableMemory> Mapper<M> {
    pub fn new(mut mem: M) -> Option<Self> {
        let root = mem.alloc_table()?;
        Some(Mapper { root, mem, tables: 1 })
    }

    /// Map `len` bytes at virtual `va` to physical `pa`. Uses 32 MiB blocks
    /// wherever both addresses are 32 MiB-aligned and enough length
    /// remains, 16 KiB pages elsewhere.
    pub fn map(&mut self, va: u64, pa: u64, len: u64, kind: Kind) -> Result<(), MapError> {
        if (va | pa | len) % PAGE_SIZE != 0 {
            return Err(MapError::Misaligned);
        }
        let mut done = 0;
        while done < len {
            let (v, p) = (va + done, pa + done);
            if (v | p) % BLOCK_SIZE == 0 && len - done >= BLOCK_SIZE {
                self.map_one(v, p, 2, kind)?;
                done += BLOCK_SIZE;
            } else {
                self.map_one(v, p, 3, kind)?;
                done += PAGE_SIZE;
            }
        }
        Ok(())
    }

    /// One block (level 2) or page (level 3), creating tables on the way.
    fn map_one(&mut self, va: u64, pa: u64, leaf_level: u32, kind: Kind) -> Result<(), MapError> {
        let mut table = self.root;
        for level in 0..leaf_level {
            let i = index(va, level);
            let d = self.mem.table(table)[i];
            table = if d & VALID == 0 {
                let new = self.mem.alloc_table().ok_or(MapError::OutOfMemory)?;
                self.tables += 1;
                self.mem.table(table)[i] = new | TABLE_OR_PAGE | VALID;
                new
            } else if d & TABLE_OR_PAGE == 0 {
                return Err(MapError::AlreadyMapped(canonical(va))); // a block covers it
            } else {
                d & ADDRESS
            };
        }
        let slot = &mut self.mem.table(table)[index(va, leaf_level)];
        if *slot & VALID != 0 {
            return Err(MapError::AlreadyMapped(canonical(va)));
        }
        let type_bit = if leaf_level == 3 { TABLE_OR_PAGE } else { 0 };
        *slot = pa | kind.attributes() | type_bit | VALID;
        Ok(())
    }

    /// Where `va` leads: (physical address, leaf descriptor, level).
    pub fn translate(&mut self, va: u64) -> Option<(u64, u64, u32)> {
        let mut table = self.root;
        for level in 0..4 {
            let d = self.mem.table(table)[index(va, level)];
            if d & VALID == 0 {
                return None;
            }
            if level < 3 && d & TABLE_OR_PAGE != 0 {
                table = d & ADDRESS;
                continue;
            }
            let size = span(level);
            return Some(((d & ADDRESS & !(size - 1)) + (va & (size - 1)), d, level));
        }
        None
    }

    /// Call `f(va, pa, size, descriptor)` for every block and page.
    pub fn for_each_leaf(&mut self, mut f: impl FnMut(u64, u64, u64, u64)) {
        let root = self.root;
        self.visit(root, 0, 0, &mut f);
    }

    fn visit(&mut self, table: u64, level: u32, base: u64, f: &mut impl FnMut(u64, u64, u64, u64)) {
        for i in 0..ENTRIES {
            let d = self.mem.table(table)[i];
            if d & VALID == 0 {
                continue;
            }
            let va = base | ((i as u64) << shift(level));
            if level < 3 && d & TABLE_OR_PAGE != 0 {
                self.visit(d & ADDRESS, level + 1, va, f);
            } else {
                f(canonical(va), d & ADDRESS, span(level), d);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    extern crate std;
    use super::*;
    use std::boxed::Box;
    use std::vec::Vec;

    /// Tables in heap memory, at made-up physical addresses.
    struct FakeMemory {
        tables: Vec<Box<[u64; ENTRIES]>>,
        limit: usize,
    }

    const FAKE_BASE: u64 = 0x100_0000;

    impl TableMemory for FakeMemory {
        fn alloc_table(&mut self) -> Option<u64> {
            if self.tables.len() == self.limit {
                return None;
            }
            self.tables.push(Box::new([0; ENTRIES]));
            Some(FAKE_BASE + (self.tables.len() as u64 - 1) * PAGE_SIZE)
        }
        fn table(&mut self, phys: u64) -> &mut [u64; ENTRIES] {
            &mut self.tables[((phys - FAKE_BASE) / PAGE_SIZE) as usize]
        }
    }

    fn mapper() -> Mapper<FakeMemory> {
        Mapper::new(FakeMemory { tables: Vec::new(), limit: usize::MAX }).unwrap()
    }

    const KERNEL: u64 = 0xffff_ffff_8000_0000;

    #[test]
    fn maps_a_page() {
        let mut m = mapper();
        m.map(KERNEL, 0x4000_0000, PAGE_SIZE, Kind::Code).unwrap();
        let (pa, d, level) = m.translate(KERNEL + 0x123).unwrap();
        assert_eq!((pa, level), (0x4000_0123, 3));
        assert!(d & GUARDED != 0 && d & READ_ONLY != 0 && d & PXN == 0);
        assert_eq!(m.translate(KERNEL + PAGE_SIZE), None);
        assert_eq!(m.tables, 4); // one table per level
    }

    #[test]
    fn uses_blocks_when_aligned() {
        let mut m = mapper();
        m.map(0xffff_0000_4000_0000, 0x4000_0000, 2 * BLOCK_SIZE, Kind::ReadWrite).unwrap();
        let (pa, _, level) = m.translate(0xffff_0000_4200_0010).unwrap();
        assert_eq!((pa, level), (0x4200_0010, 2));
        assert_eq!(m.tables, 3); // no level-3 table needed
    }

    #[test]
    fn ragged_regions_mix_pages_and_blocks() {
        let mut m = mapper();
        let (va, pa) = (0xffff_0000_41ff_8000, 0x41ff_8000); // two pages below a block boundary
        let len = 2 * PAGE_SIZE + BLOCK_SIZE + 3 * PAGE_SIZE;
        m.map(va, pa, len, Kind::ReadWrite).unwrap();
        for off in (0..len).step_by(PAGE_SIZE as usize) {
            assert_eq!(m.translate(va + off).unwrap().0, pa + off);
        }
        let mut sizes = Vec::new();
        m.for_each_leaf(|_, _, size, _| sizes.push(size));
        sizes.sort();
        assert_eq!(sizes, [PAGE_SIZE, PAGE_SIZE, PAGE_SIZE, PAGE_SIZE, PAGE_SIZE, BLOCK_SIZE]);
    }

    #[test]
    fn misaligned_requests_are_refused() {
        let mut m = mapper();
        assert_eq!(m.map(KERNEL + 1, 0, PAGE_SIZE, Kind::Code), Err(MapError::Misaligned));
        assert_eq!(m.map(KERNEL, 1, PAGE_SIZE, Kind::Code), Err(MapError::Misaligned));
        assert_eq!(m.map(KERNEL, 0, 100, Kind::Code), Err(MapError::Misaligned));
    }

    #[test]
    fn overlaps_are_refused() {
        let mut m = mapper();
        m.map(KERNEL, 0x4000_0000, PAGE_SIZE, Kind::ReadWrite).unwrap();
        assert_eq!(m.map(KERNEL, 0x5000_0000, PAGE_SIZE, Kind::ReadWrite), Err(MapError::AlreadyMapped(KERNEL)));
        // A block over a range that already has a page table.
        let block_va = KERNEL & !(BLOCK_SIZE - 1);
        assert!(matches!(m.map(block_va, 0x6000_0000, BLOCK_SIZE, Kind::ReadWrite), Err(MapError::AlreadyMapped(_))));
        // A page inside an existing block.
        m.map(0xffff_0000_0000_0000, 0, BLOCK_SIZE, Kind::ReadWrite).unwrap();
        assert!(matches!(m.map(0xffff_0000_0000_4000, 0, PAGE_SIZE, Kind::ReadWrite), Err(MapError::AlreadyMapped(_))));
    }

    #[test]
    fn running_out_of_tables_is_reported() {
        let mut m = Mapper::new(FakeMemory { tables: Vec::new(), limit: 2 }).unwrap();
        assert_eq!(m.map(KERNEL, 0, PAGE_SIZE, Kind::Code), Err(MapError::OutOfMemory));
    }

    #[test]
    fn kinds_have_the_right_permissions() {
        let mut m = mapper();
        let kinds = [Kind::Code, Kind::UnguardedCode, Kind::ReadOnly, Kind::ReadWrite, Kind::Device, Kind::Framebuffer];
        for (n, kind) in kinds.into_iter().enumerate() {
            let va = KERNEL + n as u64 * PAGE_SIZE;
            m.map(va, PAGE_SIZE * n as u64, PAGE_SIZE, kind).unwrap();
            let d = m.translate(va).unwrap().1;
            assert!(!writable_and_executable(d), "{kind:?} is W+X");
            assert!(d & UXN != 0 && d & ACCESS_FLAG != 0);
            assert_eq!(d & (1 << 6), 0, "{kind:?} reachable from user code");
            let attr = (d >> 2) & 7;
            match kind {
                Kind::Code => assert!(d & PXN == 0 && d & GUARDED != 0 && attr == ATTR_NORMAL),
                Kind::UnguardedCode => assert!(d & PXN == 0 && d & GUARDED == 0 && d & READ_ONLY != 0),
                Kind::ReadOnly => assert!(d & PXN != 0 && d & READ_ONLY != 0),
                Kind::ReadWrite => assert!(d & PXN != 0 && d & READ_ONLY == 0),
                Kind::Device => assert!(d & PXN != 0 && attr == ATTR_DEVICE),
                Kind::Framebuffer => assert!(d & PXN != 0 && attr == ATTR_FRAMEBUFFER),
            }
        }
    }

    #[test]
    fn writable_and_executable_is_detected() {
        assert!(writable_and_executable(VALID | ACCESS_FLAG));
        assert!(!writable_and_executable(VALID | PXN));
        assert!(!writable_and_executable(VALID | READ_ONLY));
    }

    #[test]
    fn upper_and_lower_half_use_the_two_level_0_entries() {
        let mut m = mapper();
        m.map(KERNEL, 0x4000_0000, PAGE_SIZE, Kind::Code).unwrap();
        m.map(0x10_0000, 0x4000_4000, PAGE_SIZE, Kind::ReadOnly).unwrap();
        let root = m.root;
        let entries = m.mem.table(root);
        assert!(entries[0] & VALID != 0 && entries[1] & VALID != 0);
        assert!(entries[2..].iter().all(|&d| d == 0));
    }

    #[test]
    fn leaves_report_canonical_upper_half_addresses() {
        let mut m = mapper();
        m.map(KERNEL, 0x4000_0000, PAGE_SIZE, Kind::Code).unwrap();
        let mut seen = Vec::new();
        m.for_each_leaf(|va, pa, size, _| seen.push((va, pa, size)));
        assert_eq!(seen, [(KERNEL, 0x4000_0000, PAGE_SIZE)]);
    }
}
