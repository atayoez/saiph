//! The Arm PL011 serial port. The firmware has already set its speed and
//! format; we only put bytes into its transmit queue.

use core::sync::atomic::{AtomicU64, Ordering};

/// Data register: writing a byte sends it.
const UARTDR: u64 = 0x00;
/// Flag register; bit 5 (TXFF) = transmit queue full.
const UARTFR: u64 = 0x18;
const TXFF: u32 = 1 << 5;

/// Where the console PL011 is mapped (0 = not yet).
static CONSOLE: AtomicU64 = AtomicU64::new(0);

pub struct Pl011 {
    base: u64,
}

/// Use the PL011 mapped at `base` as the console from now on. `base` must
/// be a mapped PL011 register block.
pub fn set_console(base: u64) {
    CONSOLE.store(base, Ordering::Relaxed);
}

impl Pl011 {
    fn put(&mut self, byte: u8) {
        // SAFETY: `base` is a mapped PL011 (see `set_console`).
        unsafe {
            while core::ptr::read_volatile((self.base + UARTFR) as *const u32) & TXFF != 0 {}
            core::ptr::write_volatile((self.base + UARTDR) as *mut u32, byte as u32);
        }
    }
}

impl core::fmt::Write for Pl011 {
    fn write_str(&mut self, s: &str) -> core::fmt::Result {
        for b in s.bytes() {
            if b == b'\n' {
                self.put(b'\r'); // terminals expect CR LF
            }
            self.put(b);
        }
        Ok(())
    }
}

/// The console, if it has been mapped already.
pub fn console() -> Option<Pl011> {
    match CONSOLE.load(Ordering::Relaxed) {
        0 => None,
        base => Some(Pl011 { base }),
    }
}
