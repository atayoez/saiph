//! The console: what the kernel prints goes to the serial port and to every
//! screen, whichever of them is set up: `writeln!(Console, ...)`.
//!
//! A lock keeps cores from printing over each other: each `write!` or
//! `writeln!` comes out whole. It masks interrupts, so interrupt handlers
//! may print too. Fault reports and panics write through `Raw` instead,
//! without the lock: the fault may have hit while this core held it.

use crate::sync::{IrqsOff, SpinLock};
use crate::{screen, uart};
use core::fmt::{self, Write};

static LOCK: SpinLock<(), IrqsOff> = SpinLock::new(());

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
        if let Some(mut serial) = uart::console() {
            serial.write_str(s)?;
        }
        screen::write_str(s);
        Ok(())
    }
}
