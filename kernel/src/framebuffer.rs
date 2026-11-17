//! Linear framebuffer graphics, discovered from the Multiboot2 info structure.

use core::ptr::read_unaligned;
use core::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

use crate::diagnostics;
use crate::font::FONT;
use crate::sync::SpinLock;

extern "C" {
    static mb2_info_ptr: u64;
    static mb2_magic: u64;
    static mb2_valid: u32;
}

#[repr(C)]
struct Mb2FramebufferTag {
    tag_type: u32,
    size: u32,
    address: u64,
    pitch: u32,
    width: u32,
    height: u32,
    bpp: u8,
    framebuffer_type: u8,
    reserved: u8,
}

struct Framebuffer {
    address: usize,
    pitch: usize,
    width: usize,
    height: usize,
    bytes_per_pixel: usize,
}

static FRAMEBUFFER: SpinLock<Option<Framebuffer>> = SpinLock::new(None);

/// Framebuffer orientation. Some VMs present the linear framebuffer rotated
/// 180 degrees. Set `ZENITH_FB_FLIP` to `x`, `y`, or `xy` when building, or call
/// `set_orientation()` at runtime, to correct it.
const fn str_eq(left: &str, right: &str) -> bool {
    let left = left.as_bytes();
    let right = right.as_bytes();
    if left.len() != right.len() {
        return false;
    }
    let mut index = 0;
    while index < left.len() {
        if left[index] != right[index] {
            return false;
        }
        index += 1;
    }
    true
}

const fn flip_from(value: Option<&str>, horizontal: bool) -> bool {
    match value {
        Some(setting) => {
            if horizontal {
                str_eq(setting, "x") || str_eq(setting, "xy")
            } else {
                str_eq(setting, "y") || str_eq(setting, "xy")
            }
        }
        None => false,
    }
}

const FLIP_X_DEFAULT: bool = flip_from(option_env!("ZENITH_FB_FLIP"), true);
const FLIP_Y_DEFAULT: bool = flip_from(option_env!("ZENITH_FB_FLIP"), false);

static FLIP_X: AtomicBool = AtomicBool::new(FLIP_X_DEFAULT);
static FLIP_Y: AtomicBool = AtomicBool::new(FLIP_Y_DEFAULT);

/// The on-screen console stays quiet during boot. Full logs still go to COM1
/// serial; the framebuffer is painted only once `boot_complete()` is called.
static CONSOLE_READY: AtomicBool = AtomicBool::new(false);

/// Repaint the desktop and reset the console to the top-left.
pub fn clear_screen() {
    reset_cursor();
    *SHADOW.lock() = [[0; SHADOW_COLS]; SHADOW_ROWS];
    crate::wm::render_now();
}

/// Show the finished desktop and enable on-screen console output.
pub fn boot_complete() {
    reset_cursor();
    *SHADOW.lock() = [[0; SHADOW_COLS]; SHADOW_ROWS];
    crate::wm::render_now();
    CONSOLE_READY.store(true, Ordering::Release);
}

/// Set framebuffer orientation (used to correct VMs with a rotated display).
pub fn set_orientation(flip_x: bool, flip_y: bool) {
    FLIP_X.store(flip_x, Ordering::Relaxed);
    FLIP_Y.store(flip_y, Ordering::Relaxed);
}

/// Current orientation as `(flip_x, flip_y)`.
pub fn orientation() -> (bool, bool) {
    (FLIP_X.load(Ordering::Relaxed), FLIP_Y.load(Ordering::Relaxed))
}

/// Cycle orientation: normal -> X -> XY -> Y -> normal, then redraw.
/// Used by the F12 debug key so a rotated VM can be corrected at runtime.
pub fn cycle_orientation() {
    let (x, y) = orientation();
    let (next_x, next_y) = match (x, y) {
        (false, false) => (true, true), // first press fixes a 180-degree VM
        (true, true) => (true, false),
        (true, false) => (false, true),
        (false, true) => (false, false),
    };
    set_orientation(next_x, next_y);

    // Reset the console and repaint the desktop so the change is immediate.
    clear_screen();
    let label = match (next_x, next_y) {
        (false, false) => "normal",
        (true, false) => "mirror-x",
        (true, true) => "rotated-180",
        (false, true) => "flip-y",
    };
    diagnostics::println("[fb] orientation:");
    diagnostics::println(label);
    diagnostics::println("[fb] F12 to change");
}

#[inline]
fn mapped(fb: &Framebuffer, x: usize, y: usize) -> (usize, usize) {
    (
        if FLIP_X.load(Ordering::Relaxed) { fb.width - 1 - x } else { x },
        if FLIP_Y.load(Ordering::Relaxed) { fb.height - 1 - y } else { y },
    )
}

/// ACPI RSDP pointer reported by the Multiboot2 ACPI tag (type 14).
static mut MB2_RSDP: u64 = 0;

/// Return the RSDP pointer supplied by the bootloader, if any.
pub fn mb2_rsdp() -> u64 {
    unsafe { MB2_RSDP }
}

/// Parse Multiboot2 tags and initialise the framebuffer if one was provided.
pub fn init() {
    if let Some(fb) = crate::limine::framebuffer() {
        if fb.bpp == 32 && fb.address != 0 {
            *FRAMEBUFFER.lock() = Some(Framebuffer {
                address: fb.address as usize,
                pitch: fb.pitch as usize,
                width: fb.width as usize,
                height: fb.height as usize,
                bytes_per_pixel: (fb.bpp / 8) as usize,
            });
            diagnostics::debug("Limine linear framebuffer detected");
            return;
        }
    }

    // `mb2_magic` is kept for diagnostics; the validity flag is set in asm.
    let _ = unsafe { mb2_magic };
    if unsafe { mb2_valid } == 0 {
        return;
    }

    let base = unsafe { mb2_info_ptr } as usize;
    if base == 0 {
        return;
    }

    let total_size = unsafe { read_unaligned(base as *const u32) } as usize;
    if total_size < 16 || total_size > 1024 * 1024 {
        return;
    }

    let mut offset = 8usize;
    while offset + 8 <= total_size {
        let tag_type = unsafe { read_unaligned((base + offset) as *const u32) };
        if tag_type == 0 {
            break;
        }
        let tag_size = unsafe { read_unaligned((base + offset + 4) as *const u32) } as usize;
        if tag_size < 8 || offset + tag_size > total_size {
            break;
        }

        if tag_type == 14 && tag_size >= 16 {
            // ACPI tag: on BIOS the RSDP structure is embedded inline; on EFI
            // it is a pointer to the structure.
            let data = base + offset + 8;
            let signature = unsafe { core::slice::from_raw_parts(data as *const u8, 8) };
            if signature == b"RSD PTR " {
                unsafe { MB2_RSDP = data as u64 };
            } else {
                unsafe {
                    MB2_RSDP = read_unaligned(data as *const u64);
                }
            }
        }

        if tag_type == 8 && tag_size >= core::mem::size_of::<Mb2FramebufferTag>() {
            let tag = unsafe { &*((base + offset) as *const Mb2FramebufferTag) };
            if tag.bpp == 32 && tag.framebuffer_type == 1 {
                *FRAMEBUFFER.lock() = Some(Framebuffer {
                    address: tag.address as usize,
                    pitch: tag.pitch as usize,
                    width: tag.width as usize,
                    height: tag.height as usize,
                    bytes_per_pixel: (tag.bpp / 8) as usize,
                });
            }
        }

        offset += (tag_size + 7) & !7;
    }

    if FRAMEBUFFER.lock().is_some() {
        diagnostics::debug("framebuffer detected");
    }
}

/// Returns true when a usable linear framebuffer is available.
pub fn available() -> bool {
    FRAMEBUFFER.lock().is_some()
}

/// Return the framebuffer size in pixels, or `(0, 0)` when unavailable.
pub fn dimensions() -> (usize, usize) {
    FRAMEBUFFER
        .lock()
        .as_ref()
        .map(|fb| (fb.width, fb.height))
        .unwrap_or((0, 0))
}

struct TextCursor {
    column: usize,
    row: usize,
}

static TEXT: SpinLock<TextCursor> = SpinLock::new(TextCursor {
    column: 0,
    row: 0,
});

// Logical console layout. The window manager decides where the terminal window
// sits; this module only tracks the text grid and how many cells fit.
const COL_PITCH: usize = 16;
const ROW_PITCH: usize = 20;
const ACCENT: u32 = 0x000078D4;
const SHADOW_COLS: usize = 100;
const SHADOW_ROWS: usize = 40;

static CONSOLE_COLS: AtomicUsize = AtomicUsize::new(56);
static CONSOLE_ROWS: AtomicUsize = AtomicUsize::new(20);

/// Tell the console how many character cells fit in the visible terminal.
pub fn set_console_size(columns: usize, rows: usize) {
    CONSOLE_COLS.store(columns.clamp(1, SHADOW_COLS), Ordering::Relaxed);
    CONSOLE_ROWS.store(rows.clamp(1, SHADOW_ROWS), Ordering::Relaxed);
}

fn reset_cursor() {
    let mut cursor = TEXT.lock();
    cursor.column = 0;
    cursor.row = 0;
}

/// Logical cursor position as `(column, row)`.
pub fn console_cursor() -> (usize, usize) {
    let cursor = TEXT.lock();
    (cursor.column, cursor.row)
}

// A shadow copy of the console is the source of truth for terminal text. The
// compositor replays it into the terminal window on every frame.
static SHADOW: SpinLock<[[u8; SHADOW_COLS]; SHADOW_ROWS]> =
    SpinLock::new([[0; SHADOW_COLS]; SHADOW_ROWS]);

fn draw_glyph_to(
    frame: &mut [u32],
    stride: usize,
    height: usize,
    x: usize,
    y: usize,
    byte: u8,
    color: u32,
) {
    let glyph = &FONT[(byte as usize) & 0x7F];
    for row in 0..8 {
        let py = y + row * 2;
        if py + 1 >= height {
            break;
        }
        for column in 0..8 {
            if glyph[row] & (1 << (7 - column)) == 0 {
                continue;
            }
            let px = x + column * 2;
            if px + 1 >= stride {
                continue;
            }
            let offset = py * stride + px;
            frame[offset] = color;
            frame[offset + 1] = color;
            frame[offset + stride] = color;
            frame[offset + stride + 1] = color;
        }
    }
}

/// Replay the console shadow into a compositor frame buffer.
pub fn draw_console_into(
    frame: &mut [u32],
    stride: usize,
    height: usize,
    x0: usize,
    y0: usize,
    clip_w: usize,
    clip_h: usize,
    color: u32,
) {
    if stride == 0 || height == 0 || frame.len() < stride.saturating_mul(height) {
        return;
    }
    let shadow = SHADOW.lock();
    let columns = (clip_w / COL_PITCH).min(SHADOW_COLS);
    let rows = (clip_h / ROW_PITCH).min(SHADOW_ROWS);
    for row in 0..rows {
        for column in 0..columns {
            let byte = shadow[row][column];
            if byte != 0 {
                draw_glyph_to(
                    frame,
                    stride,
                    height,
                    x0 + column * COL_PITCH,
                    y0 + row * ROW_PITCH,
                    byte,
                    color,
                );
            }
        }
    }
}

fn scroll_up(shadow: &mut [[u8; SHADOW_COLS]; SHADOW_ROWS]) {
    for row in 0..SHADOW_ROWS - 1 {
        shadow[row] = shadow[row + 1];
    }
    shadow[SHADOW_ROWS - 1] = [0; SHADOW_COLS];
}

/// Write one character into the console shadow. The compositor repaints the
/// terminal window from that shadow on the next frame.
pub fn putc(byte: u8) {
    // Boot logs are for COM1 only; nothing is displayed until boot_complete().
    if !CONSOLE_READY.load(Ordering::Acquire) {
        return;
    }

    let columns = CONSOLE_COLS.load(Ordering::Relaxed);
    let rows = CONSOLE_ROWS.load(Ordering::Relaxed);
    let mut cursor = TEXT.lock();
    let mut shadow = SHADOW.lock();

    match byte {
        b'\n' => {
            cursor.column = 0;
            cursor.row += 1;
        }
        b'\r' => cursor.column = 0,
        0x08 => {
            if cursor.column > 0 {
                cursor.column -= 1;
                if cursor.row < SHADOW_ROWS && cursor.column < SHADOW_COLS {
                    shadow[cursor.row][cursor.column] = 0;
                }
            }
        }
        other => {
            if cursor.row < SHADOW_ROWS && cursor.column < SHADOW_COLS {
                shadow[cursor.row][cursor.column] = other;
            }
            cursor.column += 1;
        }
    }

    if cursor.column >= columns {
        cursor.column = 0;
        cursor.row += 1;
    }
    if cursor.row >= rows {
        scroll_up(&mut shadow);
        cursor.row = rows.saturating_sub(1);
    }
    drop(shadow);
    drop(cursor);
    crate::wm::invalidate();
}

/// Write a string to the framebuffer console.
pub fn puts(text: &str) {
    for &byte in text.as_bytes() {
        putc(byte);
    }
}

/// Fill the entire screen with one colour (caller holds no framebuffer lock).
pub fn clear(color: u32) {
    if let Some(fb) = FRAMEBUFFER.lock().as_ref() {
        clear_locked(fb, color);
    }
}

fn clear_locked(fb: &Framebuffer, color: u32) {
    for y in 0..fb.height {
        for x in 0..fb.width {
            put_pixel(fb, x, y, color);
        }
    }
}

#[inline]
fn put_pixel(fb: &Framebuffer, x: usize, y: usize, color: u32) {
    if x >= fb.width || y >= fb.height {
        return;
    }
    let (x, y) = mapped(fb, x, y);
    let offset = y * fb.pitch + x * fb.bytes_per_pixel;
    unsafe {
        core::ptr::write_volatile((fb.address + offset) as *mut u32, color);
    }
}

/// Draw a filled rectangle.
pub fn fill_rect(x: usize, y: usize, width: usize, height: usize, color: u32) {
    if let Some(fb) = FRAMEBUFFER.lock().as_ref() {
        fill_rect_locked(fb, x, y, width, height, color);
    }
}

fn fill_rect_locked(fb: &Framebuffer, x: usize, y: usize, width: usize, height: usize, color: u32) {
    for row in y..y.saturating_add(height) {
        for col in x..x.saturating_add(width) {
            put_pixel(fb, col, row, color);
        }
    }
}

/// Draw a string at an arbitrary pixel position (used by desktop and installer).
pub fn text(x: usize, y: usize, message: &str, color: u32) {
    if let Some(fb) = FRAMEBUFFER.lock().as_ref() {
        draw_text(fb, x, y, message, color);
    }
}

fn draw_text(fb: &Framebuffer, x: usize, y: usize, text: &str, color: u32) {
    for (index, byte) in text.bytes().enumerate() {
        let glyph = &FONT[(byte as usize) & 0x7F];
        for row in 0..8 {
            for column in 0..8 {
                if glyph[row] & (1 << (7 - column)) == 0 {
                    continue;
                }
                for dy in 0..2 {
                    for dx in 0..2 {
                        put_pixel(
                            fb,
                            x + index * COL_PITCH + column * 2 + dx,
                            y + row * 2 + dy,
                            color,
                        );
                    }
                }
            }
        }
    }
}

/// Copy a composited frame into the linear framebuffer.
///
/// The frame is `width * height` pixels in `0x00RRGGBB` form. Row copies use
/// 64-bit stores and honour the runtime display orientation.
pub fn present(frame: &[u32]) {
    let framebuffer = FRAMEBUFFER.lock();
    let Some(fb) = framebuffer.as_ref() else {
        return;
    };
    if fb.bytes_per_pixel != 4 || frame.len() < fb.width.saturating_mul(fb.height) {
        return;
    }
    let flip_x = FLIP_X.load(Ordering::Relaxed);
    let flip_y = FLIP_Y.load(Ordering::Relaxed);

    for y in 0..fb.height {
        let source_y = if flip_y { fb.height - 1 - y } else { y };
        let source = &frame[source_y * fb.width..source_y * fb.width + fb.width];
        let destination = (fb.address + y * fb.pitch) as *mut u32;
        if flip_x {
            for x in 0..fb.width {
                unsafe {
                    core::ptr::write_volatile(destination.add(x), source[fb.width - 1 - x]);
                }
            }
        } else {
            let mut x = 0;
            while x + 2 <= fb.width {
                let word = (source[x] as u64) | ((source[x + 1] as u64) << 32);
                unsafe {
                    core::ptr::write_volatile(destination.add(x) as *mut u64, word);
                }
                x += 2;
            }
            while x < fb.width {
                unsafe { core::ptr::write_volatile(destination.add(x), source[x]) };
                x += 1;
            }
        }
    }
}

// ---- Boot splash ---------------------------------------------------------

const SPLASH_BG: u32 = 0x000B1220;
const SPLASH_FG: u32 = 0x00F0F4F8;
const SPLASH_MUTED: u32 = 0x006B7A8A;

fn splash_bar(fb: &Framebuffer) -> (usize, usize, usize) {
    let bar_w = 440usize.min(fb.width.saturating_sub(80));
    let x = (fb.width.saturating_sub(bar_w)) / 2;
    let y = fb.height.saturating_sub(150);
    (x, y, bar_w)
}

fn centered_text(fb: &Framebuffer, y: usize, message: &str, color: u32) {
    let x = fb.width.saturating_sub(message.len() * COL_PITCH) / 2;
    draw_text(fb, x, y, message, color);
}

/// Paint the Deepin-style boot splash. Called during real initialisation, so
/// the progress bar reflects work actually being done rather than a delay.
pub fn splash_init() {
    let framebuffer = FRAMEBUFFER.lock();
    let Some(fb) = framebuffer.as_ref() else {
        return;
    };
    let (width, height) = (fb.width, fb.height);
    clear_locked(fb, SPLASH_BG);
    fill_rect_locked(fb, 0, 0, width, 4, ACCENT);

    let logo = 96usize.min(width / 6).min(height / 6);
    let logo_x = (width - logo) / 2;
    let logo_y = height / 2 - logo - 64;

    // Framed logo with a stylised "Z".
    fill_rect_locked(fb, logo_x, logo_y, logo, 4, ACCENT);
    fill_rect_locked(fb, logo_x, logo_y + logo - 4, logo, 4, ACCENT);
    fill_rect_locked(fb, logo_x, logo_y, 4, logo, ACCENT);
    fill_rect_locked(fb, logo_x + logo - 4, logo_y, 4, logo, ACCENT);
    let inner = logo / 5;
    let left = logo_x + logo / 4;
    let top = logo_y + logo / 4;
    fill_rect_locked(fb, left, top, inner * 3, inner, SPLASH_FG);
    fill_rect_locked(fb, left, top + inner * 2, inner * 3, inner, SPLASH_FG);
    for step in 0..3 {
        fill_rect_locked(fb, left + inner * (2 - step), top + inner * (step + 1),
                         inner, inner, SPLASH_FG);
    }

    centered_text(fb, logo_y + logo + 24, "ZENITH OS", SPLASH_FG);
    centered_text(fb, logo_y + logo + 52, "INDEPENDENT x86_64 OPERATING SYSTEM", SPLASH_MUTED);

    let (bar_x, bar_y, bar_w) = splash_bar(fb);
    fill_rect_locked(fb, bar_x, bar_y, bar_w, 6, 0x00212930);
    centered_text(fb, bar_y - 32, "Starting Zenith OS...", SPLASH_MUTED);
}

/// Update the splash progress bar (0..=100) and status message.
pub fn splash_step(status: &str, progress: usize) {
    let framebuffer = FRAMEBUFFER.lock();
    let Some(fb) = framebuffer.as_ref() else {
        return;
    };
    let (bar_x, bar_y, bar_w) = splash_bar(fb);
    fill_rect_locked(fb, 0, bar_y - 36, fb.width, 28, SPLASH_BG);
    centered_text(fb, bar_y - 32, status, SPLASH_MUTED);
    let percent = progress.min(100);
    let filled = bar_w * percent / 100;
    fill_rect_locked(fb, bar_x, bar_y, filled, 6, ACCENT);
    fill_rect_locked(fb, bar_x + filled, bar_y, bar_w - filled, 6, 0x00212930);
}
