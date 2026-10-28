//! Text on the screen.
//!
//! The firmware has already set up a framebuffer for each display it drives:
//! a block of memory holding the picture, one 32-bit value per pixel, row
//! after row. Whatever is written there appears on the monitor. Limine
//! passes the framebuffers on; we draw characters into them by copying
//! ready-made letter images that build.rs renders from Fira Code.
//!
//! The firmware decides how the framebuffer is cached, and Limine maps it
//! that way. If it is ordinary cached memory, new pixels first land in the
//! CPU's caches, and the display engine, which reads RAM, keeps showing the
//! old picture until they happen to be written back. So after drawing we
//! clean the touched cache lines out to RAM (`dc cvac`).

use core::cell::UnsafeCell;
use core::fmt::Write;
use limine::framebuffer::{FRAMEBUFFER_RGB, Framebuffer};
use limine::request::FramebufferRequest;

#[used]
#[unsafe(link_section = ".requests")]
static FRAMEBUFFERS: FramebufferRequest = FramebufferRequest::new();

/// Text colour, as red, green, blue; the background is black.
const TEXT: [u32; 3] = [0xd8, 0xd8, 0xd8];

const MAX_SCREENS: usize = 4;

/// A console font: one image per printable ASCII character (' ' to '~'),
/// `width` x `height` pixels each, one brightness byte (0-255) per pixel.
struct Font {
    width: usize,
    height: usize,
    cells: &'static [u8],
}

// FONTS: the console font at 16, 24 and 32 px row heights (see build.rs).
include!(concat!(env!("OUT_DIR"), "/fonts.rs"));

impl Font {
    /// The image of `c`; '?' for characters the font table does not have.
    fn cell(&self, c: char) -> &'static [u8] {
        let c = if (' '..='~').contains(&c) { c } else { '?' };
        let size = self.width * self.height;
        &self.cells[(c as usize - ' ' as usize) * size..][..size]
    }
}

#[derive(Clone, Copy)]
struct Screen {
    /// The framebuffer, mapped in the direct map.
    base: *mut u8,
    width: usize,
    height: usize,
    /// Bytes from one pixel row to the next (at least width * 4).
    pitch: usize,
    /// Bit positions of red, green and blue in a pixel.
    shifts: [u8; 3],
    font: &'static Font,
    cols: usize,
    rows: usize,
    /// Where the next character goes.
    col: usize,
    row: usize,
}

struct Screens(UnsafeCell<[Option<Screen>; MAX_SCREENS]>);

// SAFETY: one core and no interrupts yet, so only one piece of code uses
// the screens at a time. An exception in the middle of a line just prints
// its report from where the cursor is.
unsafe impl Sync for Screens {}

static SCREENS: Screens = Screens(UnsafeCell::new([None; MAX_SCREENS]));

fn screens() -> &'static mut [Option<Screen>; MAX_SCREENS] {
    // SAFETY: see `Screens`.
    unsafe { &mut *SCREENS.0.get() }
}

impl Screen {
    /// Only the usual format: 32 bits per pixel, 8 bits per colour.
    fn new(fb: &Framebuffer) -> Option<Screen> {
        let usual = fb.bpp == 32
            && fb.memory_model == FRAMEBUFFER_RGB
            && [fb.red_mask_size, fb.green_mask_size, fb.blue_mask_size] == [8, 8, 8];
        if !usual {
            return None;
        }
        let (width, height) = (fb.width as usize, fb.height as usize);
        // The largest font that still leaves 45 rows: 24 px on a 1080p
        // monitor, 32 px on 1440p and 4K.
        let font = FONTS.iter().rev().find(|f| height / f.height >= 45).unwrap_or(&FONTS[0]);
        Some(Screen {
            base: fb.address() as *mut u8,
            width,
            height,
            pitch: fb.pitch as usize,
            shifts: [fb.red_mask_shift, fb.green_mask_shift, fb.blue_mask_shift],
            font,
            cols: width / font.width,
            rows: height / font.height,
            col: 0,
            row: 0,
        })
    }

    fn clear(&self) {
        // SAFETY: the framebuffer is height * pitch bytes long.
        unsafe { core::ptr::write_bytes(self.base, 0, self.height * self.pitch) };
        to_ram(self.base as usize, self.height * self.pitch);
    }

    /// Set pixel (x, y) to the text colour at `brightness` (0 = black).
    fn pixel(&self, x: usize, y: usize, brightness: u8) {
        let mut value = 0;
        for (colour, shift) in TEXT.iter().zip(self.shifts) {
            value |= colour * brightness as u32 / 255 << shift;
        }
        // SAFETY: callers keep x < width and y < height, so the pixel is
        // inside the framebuffer.
        unsafe { (self.base.add(y * self.pitch + x * 4) as *mut u32).write_volatile(value) };
    }

    fn draw(&self, c: char) {
        let (w, h) = (self.font.width, self.font.height);
        let (x0, y0) = (self.col * w, self.row * h);
        for (i, &brightness) in self.font.cell(c).iter().enumerate() {
            self.pixel(x0 + i % w, y0 + i / w, brightness);
        }
        for y in y0..y0 + h {
            to_ram(self.base as usize + y * self.pitch + x0 * 4, w * 4);
        }
    }

    fn put(&mut self, c: char) {
        match c {
            '\n' => self.newline(),
            '\r' => self.col = 0,
            _ => {
                if self.col == self.cols {
                    self.newline();
                }
                self.draw(c);
                self.col += 1;
            }
        }
    }

    fn newline(&mut self) {
        self.col = 0;
        if self.row + 1 < self.rows {
            self.row += 1;
        } else {
            self.scroll();
        }
    }

    /// Move every text row up by one and blank the last.
    fn scroll(&self) {
        let line = self.font.height * self.pitch;
        let used = self.rows * line;
        // SAFETY: both ranges lie inside the framebuffer; `copy` allows the
        // overlap.
        unsafe {
            core::ptr::copy(self.base.add(line), self.base, used - line);
            core::ptr::write_bytes(self.base.add(used - line), 0, line);
        }
        to_ram(self.base as usize, used);
    }
}

/// Write the cache lines covering [start, start + len) back to RAM ("clean
/// to the point of coherency"). Harmless if the memory is not cached.
fn to_ram(start: usize, len: usize) {
    let ctr: u64;
    // SAFETY: reading CTR_EL0 has no side effects.
    unsafe { core::arch::asm!("mrs {}, CTR_EL0", out(reg) ctr) };
    // CTR_EL0.DminLine: log2 of the smallest data cache line, in words.
    let line = 4 << ((ctr >> 16) & 0xf);
    let mut a = start & !(line - 1);
    while a < start + len {
        // SAFETY: cleaning a mapped address only writes its data to RAM.
        unsafe { core::arch::asm!("dc cvac, {}", in(reg) a) };
        a += line;
    }
    // SAFETY: waits until the writes have reached RAM.
    unsafe { core::arch::asm!("dsb sy") };
}

/// How Limine mapped the framebuffers: the memory type in MAIR slot 1.
fn memory_type() -> &'static str {
    let mair: u64;
    // SAFETY: reading MAIR has no side effects.
    unsafe { core::arch::asm!("mrs {}, MAIR_EL1", out(reg) mair) };
    let attr = (mair >> 8) & 0xff;
    if attr & 0xf0 == 0 {
        return "device memory";
    }
    // Normal memory: the low four bits are the inner (CPU-side) caching.
    match attr & 0x0f {
        0b0100 => "uncached",
        0b1000..=0b1011 => "cached (write-through)",
        0b1100..=0b1111 => "cached (write-back)",
        _ => "cached (transient)",
    }
}

/// Take over the screens Limine found and clear them. Returns how many.
pub fn init() -> usize {
    let Some(response) = FRAMEBUFFERS.response() else { return 0 };
    let mut n = 0;
    for fb in response.framebuffers() {
        if n < MAX_SCREENS && let Some(screen) = Screen::new(fb) {
            screen.clear();
            screens()[n] = Some(screen);
            n += 1;
        }
    }
    n
}

/// One line per framebuffer Limine found, for the boot log.
pub fn describe(out: &mut impl Write) {
    let found = FRAMEBUFFERS.response().map_or(&[][..], |r| r.framebuffers());
    if found.is_empty() {
        let _ = writeln!(out, "  screen: none (no framebuffer from Limine)");
    }
    for fb in found {
        let used = screens().iter().flatten().find(|s| s.base == fb.address() as *mut u8);
        let _ = match used {
            Some(s) => writeln!(out, "  screen: {}x{} framebuffer, {}, Fira Code in {} px rows ({}x{} characters)",
                s.width, s.height, memory_type(), s.font.height, s.cols, s.rows),
            None => writeln!(out, "  screen: {}x{} framebuffer at {} bits per pixel, not used (unsupported format)",
                fb.width, fb.height, fb.bpp),
        };
    }
}

/// Physical address ranges of the screens in use, as (start, length), so
/// the page tables can map them. `hhdm` is the direct-map offset.
pub fn regions(hhdm: u64) -> impl Iterator<Item = (u64, u64)> {
    screens().iter().flatten().map(move |s| (s.base as u64 - hhdm, (s.height * s.pitch) as u64))
}

/// Print `s` on every screen.
pub fn write_str(s: &str) {
    for screen in screens().iter_mut().flatten() {
        for c in s.chars() {
            screen.put(c);
        }
    }
}

/// Last resort, for a fault while a fault report is being printed: draw `a`
/// and `b` as two rows of 64 squares at the bottom of the first screen (each
/// row starts with a red marker, then the bits from the most significant:
/// bright = 1, dim = 0), then stop. It calls no other function, not even for
/// a loop or a pixel write, because a function call may be exactly what
/// keeps faulting.
pub fn show_bits_and_halt(a: u64, b: u64) -> ! {
    // SAFETY: reads the screen table in place, without a call; see `Screens`.
    let first = unsafe { &(*(&raw const SCREENS.0 as *const [Option<Screen>; MAX_SCREENS]))[0] };
    if let Some(s) = first {
        let [r, g, bl] = s.shifts;
        let (bright, dim, red) = (0xffu32 << r | 0xff << g | 0xff << bl, 0x50u32 << r | 0x50 << g | 0x50 << bl, 0xffu32 << r);
        // 65 squares (marker + 64 bits) take about 82 square widths.
        let size = s.width / 140;
        let step = size + size / 4 + 1;
        let mut row = 0;
        while row < 2 {
            let value = if row == 0 { a } else { b };
            let top = s.height - (2 - row) * step - size;
            let mut bit = 0;
            while bit < 65 {
                let colour = if bit == 0 { red } else if value >> (64 - bit) & 1 == 1 { bright } else { dim };
                let left = size + bit * step;
                let mut y = top;
                while y < top + size {
                    let line = s.base as usize + y * s.pitch;
                    let mut x = left;
                    while x < left + size {
                        // SAFETY: inside the framebuffer: the squares fit its
                        // width and the bottom rows are within its height.
                        unsafe { core::arch::asm!("str {c:w}, [{p}]", c = in(reg) colour, p = in(reg) line + x * 4) };
                        x += 1;
                    }
                    // Write the row's pixels back to RAM (see `to_ram`).
                    let mut c = (line + left * 4) & !31;
                    while c < line + (left + size) * 4 {
                        // SAFETY: cleaning a mapped address only writes its data to RAM.
                        unsafe { core::arch::asm!("dc cvac, {}", in(reg) c) };
                        c += 32;
                    }
                    y += 1;
                }
                bit += 1;
            }
            row += 1;
        }
        // SAFETY: waits until the writes have reached RAM.
        unsafe { core::arch::asm!("dsb sy") };
    }
    loop {
        // SAFETY: wfe has no effect besides waiting.
        unsafe { core::arch::asm!("wfe") };
    }
}
