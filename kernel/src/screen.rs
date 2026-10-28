//! Text on the screen.
//!
//! The firmware has already set up a framebuffer for each display it drives:
//! a block of memory holding the picture, one 32-bit value per pixel, row
//! after row. Whatever is written there appears on the monitor. Limine
//! passes the framebuffers on; we draw characters into them with the
//! pre-rendered Noto Sans Mono glyphs of the noto-sans-mono-bitmap crate,
//! which store one brightness byte per pixel.

use core::cell::UnsafeCell;
use core::fmt::Write;
use limine::framebuffer::{FRAMEBUFFER_RGB, Framebuffer};
use limine::request::FramebufferRequest;
use noto_sans_mono_bitmap::{FontWeight, RasterHeight, get_raster, get_raster_width};

#[used]
#[unsafe(link_section = ".requests")]
static FRAMEBUFFERS: FramebufferRequest = FramebufferRequest::new();

/// Text colour, as red, green, blue; the background is black.
const TEXT: [u32; 3] = [0xd8, 0xd8, 0xd8];

const MAX_SCREENS: usize = 4;

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
    font: RasterHeight,
    char_width: usize,
    char_height: usize,
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
        // Text about 45 rows high on a 1080p monitor, still readable on 4K.
        let font = match height {
            2000.. => RasterHeight::Size32,
            1000.. => RasterHeight::Size24,
            _ => RasterHeight::Size16,
        };
        let (char_width, char_height) = (get_raster_width(FontWeight::Regular, font), font.val());
        Some(Screen {
            base: fb.address() as *mut u8,
            width,
            height,
            pitch: fb.pitch as usize,
            shifts: [fb.red_mask_shift, fb.green_mask_shift, fb.blue_mask_shift],
            font,
            char_width,
            char_height,
            cols: width / char_width,
            rows: height / char_height,
            col: 0,
            row: 0,
        })
    }

    fn clear(&self) {
        // SAFETY: the framebuffer is height * pitch bytes long.
        unsafe { core::ptr::write_bytes(self.base, 0, self.height * self.pitch) };
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
        let glyph = get_raster(c, FontWeight::Regular, self.font)
            .or_else(|| get_raster('?', FontWeight::Regular, self.font));
        let Some(glyph) = glyph else { return };
        let (x0, y0) = (self.col * self.char_width, self.row * self.char_height);
        for (y, line) in glyph.raster().iter().enumerate() {
            for (x, &brightness) in line.iter().enumerate() {
                self.pixel(x0 + x, y0 + y, brightness);
            }
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
        let line = self.char_height * self.pitch;
        let used = self.rows * line;
        // SAFETY: both ranges lie inside the framebuffer; `copy` allows the
        // overlap.
        unsafe {
            core::ptr::copy(self.base.add(line), self.base, used - line);
            core::ptr::write_bytes(self.base.add(used - line), 0, line);
        }
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
            Some(s) => writeln!(out, "  screen: {}x{} framebuffer, {} px text ({}x{} characters)",
                s.width, s.height, s.char_height, s.cols, s.rows),
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
