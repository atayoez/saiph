//! The console: what the kernel prints goes to the serial port and to every
//! screen, whichever of them is set up: `writeln!(Console, ...)`.
//!
//! A lock keeps cores from printing over each other: each `write!` or
//! `writeln!` comes out whole. It masks interrupts, so interrupt handlers
//! may print too. Fault reports and panics write through `Raw` instead,
//! without the lock: the fault may have hit while this core held it.
//!
//! Everything printed is also kept in a log (the newest 32 KiB), which
//! `efi::save_log` saves for reading after a reboot.

use crate::sync::{IrqsOff, SpinLock, order};
use crate::{screen, uart};
use core::fmt::{self, Write};
use core::sync::atomic::{AtomicU8, AtomicUsize, Ordering};

/// The log: byte `n` of everything printed is at `LOG[n % LOG.len()]`.
static LOG: [AtomicU8; 32 * 1024] = [const { AtomicU8::new(0) }; 32 * 1024];
/// Bytes printed so far.
static LOGGED: AtomicUsize = AtomicUsize::new(0);

/// The positions of the bytes the log still holds (see `log_byte`).
pub fn log_range() -> core::ops::Range<usize> {
    let end = LOGGED.load(Ordering::Acquire);
    end.saturating_sub(LOG.len())..end
}

/// Byte `n` of everything printed (only valid for `n` in `log_range()`).
pub fn log_byte(n: usize) -> u8 {
    LOG[n % LOG.len()].load(Ordering::Relaxed)
}

static LOCK: SpinLock<(), IrqsOff, order::Console> = SpinLock::new(());

pub struct Console;

impl Write for Console {
    fn write_str(&mut self, s: &str) -> fmt::Result {
        let _lock = LOCK.lock();
        Raw.write_str(s)
    }

    /// One lock for everything a single `write!` prints.
    fn write_fmt(&mut self, args: fmt::Arguments<'_>) -> fmt::Result {
        let _lock = LOCK.lock();
        fmt::write(&mut Raw, args)
    }
}

/// The console without its lock, for fault reports and panics.
pub struct Raw;

impl Write for Raw {
    fn write_str(&mut self, s: &str) -> fmt::Result {
        let start = LOGGED.fetch_add(s.len(), Ordering::AcqRel);
        for (n, b) in s.bytes().enumerate() {
            LOG[(start + n) % LOG.len()].store(b, Ordering::Relaxed);
        }
        if let Some(mut serial) = uart::console() {
            serial.write_str(s)?;
        }
        screen::write_str(s);
        Ok(())
    }
}
