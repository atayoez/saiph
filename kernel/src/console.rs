//! The console: what the kernel prints goes to the serial port and to every
//! screen, whichever of them is set up. Works anywhere, including in the
//! exception and panic handlers: `writeln!(Console, ...)`.

use crate::{screen, uart};
use core::fmt::Write;

pub struct Console;

impl Write for Console {
    fn write_str(&mut self, s: &str) -> core::fmt::Result {
        if let Some(mut serial) = uart::console() {
            serial.write_str(s)?;
        }
        screen::write_str(s);
        Ok(())
    }
}
