//! Per-core variables: `cpu_local!` statics, after Linux's per-CPU areas
//! and Asterinas's `cpu_local!`.
//!
//! The linker collects every `cpu_local!` static into the `.cpu_local`
//! section. The boot core uses that section itself; every other core gets a
//! copy of it. The copies are made from a snapshot of the section taken
//! first thing at boot, so each starts out with the initial values, not
//! with whatever the boot core has changed since. TPIDR_EL1 holds how far
//! the running core's copy is from the section, so a variable's address on
//! this core is its address in the section plus TPIDR_EL1.
//!
//! TPIDR_EL1 serves at EL2 too: VHE does not redirect it to TPIDR_EL2, and
//! nothing else uses EL1's registers while HCR_EL2.TGE is set.

use core::alloc::Layout;

/// Declare per-core statics: `cpu_local! { static NAME: Type = value; }`.
/// `Type` must be `Sync` (atomics, typically), since a reference to this
/// core's copy could still reach another core.
#[macro_export]
macro_rules! cpu_local {
    ($($(#[$attr:meta])* $vis:vis static $name:ident: $t:ty = $init:expr;)*) => {
        $(
            $(#[$attr])*
            #[unsafe(link_section = ".cpu_local")]
            $vis static $name: $crate::percpu::CpuLocal<$t> = $crate::percpu::CpuLocal::new($init);
        )*
    };
}

/// One per-core variable; `get` reaches this core's copy.
pub struct CpuLocal<T>(T);

// SAFETY: every core uses its own copy; `T: Sync` covers references that
// travel to other cores anyway.
unsafe impl<T: Sync> Sync for CpuLocal<T> {}

impl<T> CpuLocal<T> {
    pub const fn new(value: T) -> Self {
        CpuLocal(value)
    }

    /// This core's copy.
    pub fn get(&self) -> &T {
        // SAFETY: TPIDR_EL1 is this core's offset (see `init_boot_core` and
        // `new_area`), so the result is this variable in this core's copy
        // of the section, which lives as long as the kernel.
        unsafe { &*(&raw const self.0).wrapping_byte_add(offset()) }
    }
}

/// This core's offset from the section to its copy.
fn offset() -> usize {
    let offset: usize;
    // SAFETY: reading TPIDR_EL1 has no side effects.
    unsafe { core::arch::asm!("mrs {}, TPIDR_EL1", out(reg) offset, options(nomem, nostack, preserves_flags)) };
    offset
}

unsafe extern "C" {
    static __cpu_local_start: u8;
    static __cpu_local_end: u8;
}

/// Room for the snapshot of the section's initial values.
const MAX_SIZE: usize = 1024;
static mut SNAPSHOT: [u8; MAX_SIZE] = [0; MAX_SIZE];

fn section() -> (*const u8, usize) {
    let (start, end) = (&raw const __cpu_local_start, &raw const __cpu_local_end);
    (start, end as usize - start as usize)
}

/// First thing at boot: the boot core uses the section itself (offset 0),
/// and the initial values are saved for the other cores.
pub fn init_boot_core() {
    let (start, size) = section();
    assert!(size <= MAX_SIZE, "the .cpu_local section outgrew percpu::MAX_SIZE");
    // SAFETY: nothing has used the section yet; SNAPSHOT is only written here.
    unsafe {
        core::arch::asm!("msr TPIDR_EL1, xzr", options(nomem, nostack, preserves_flags));
        core::ptr::copy_nonoverlapping(start, (&raw mut SNAPSHOT).cast::<u8>(), size);
    }
}

/// A fresh copy of the section for another core, from the heap. Returns the
/// value its TPIDR_EL1 must hold.
pub fn new_area() -> usize {
    let (start, size) = section();
    let layout = Layout::from_size_align(size.max(1), 64).unwrap();
    // SAFETY: the layout has a nonzero size.
    let area = unsafe { alloc::alloc::alloc(layout) };
    if area.is_null() {
        alloc::alloc::handle_alloc_error(layout);
    }
    // SAFETY: `area` has room for `size` bytes; SNAPSHOT holds `size`
    // initialised bytes (see `init_boot_core`). The area is never freed.
    unsafe { core::ptr::copy_nonoverlapping((&raw const SNAPSHOT).cast::<u8>(), area, size) };
    (area as usize).wrapping_sub(start as usize)
}
