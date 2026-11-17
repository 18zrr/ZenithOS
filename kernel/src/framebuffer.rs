//! Linear framebuffer graphics, discovered from the Multiboot2 info structure.

use core::ptr::read_unaligned;
use core::sync::atomic::{AtomicBool, Ordering};

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

/// Repaint the desktop chrome and reset the console to the top-left.
pub fn clear_screen() {
    reset_cursor();
    clear(BG);
    draw_desktop();
}

/// Show the finished desktop and enable on-screen console output.
pub fn boot_complete() {
    clear_screen();
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
            draw_desktop();
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
        draw_desktop();
    }
}

/// Returns true when a usable linear framebuffer is available.
pub fn available() -> bool {
    FRAMEBUFFER.lock().is_some()
}

struct TextCursor {
    column: usize,
    row: usize,
    color: u32,
}

static TEXT: SpinLock<TextCursor> = SpinLock::new(TextCursor {
    column: 0,
    row: 0,
    color: 0x00E8E6E3,
});

// Console layout. The 8x8 font is scaled 2x into 16x16 glyphs; ROW_PITCH adds
// four pixels of breathing room between lines. The header and dock are static
// and the console scrolls only in the region between them.
const COL_PITCH: usize = 16;
const ROW_PITCH: usize = 20;
const HEADER_H: usize = 40;
const DOCK_H: usize = 88;
const MARGIN_X: usize = 24;

const BG: u32 = 0x00181C20;
const PANEL: u32 = 0x00212930;
const TITLEBAR: u32 = 0x002D8FB0;
const ACCENT: u32 = 0x00E17055;
const FG: u32 = 0x00E8E6E3;
const MUTED: u32 = 0x008A969B;

fn console_columns(fb: &Framebuffer) -> usize {
    let columns = fb.width.saturating_sub(MARGIN_X * 2) / COL_PITCH;
    if columns == 0 { 1 } else { columns.min(100) }
}

fn console_rows(fb: &Framebuffer) -> usize {
    let rows = fb.height.saturating_sub(HEADER_H + DOCK_H) / ROW_PITCH;
    if rows == 0 { 1 } else { rows }
}

fn cell_x(column: usize) -> usize {
    MARGIN_X + column * COL_PITCH
}

fn cell_y(row: usize) -> usize {
    HEADER_H + row * ROW_PITCH
}

fn reset_cursor() {
    let mut cursor = TEXT.lock();
    cursor.column = 0;
    cursor.row = 0;
}

/// Clear one character cell (including the inter-line gap).
fn clear_cell(fb: &Framebuffer, column: usize, row: usize) {
    let start_x = cell_x(column);
    let start_y = cell_y(row);
    for y in start_y..start_y + ROW_PITCH {
        for x in start_x..start_x + COL_PITCH {
            put_pixel(fb, x, y, BG);
        }
    }
}

/// Scroll the console region up by one row; the header and dock stay fixed.
fn scroll_up(fb: &Framebuffer) {
    let top = HEADER_H;
    let bottom = fb.height.saturating_sub(DOCK_H);
    for y in top..bottom.saturating_sub(ROW_PITCH) {
        for x in 0..fb.width {
            let pixel = get_pixel(fb, x, y + ROW_PITCH);
            put_pixel(fb, x, y, pixel);
        }
    }
    for y in bottom.saturating_sub(ROW_PITCH)..bottom {
        for x in 0..fb.width {
            put_pixel(fb, x, y, BG);
        }
    }
}

/// Write one character to the framebuffer console (2x scaled 8x8 glyph).
pub fn putc(byte: u8) {
    // Boot logs are for COM1 only; nothing is painted until boot_complete().
    if !CONSOLE_READY.load(Ordering::Acquire) {
        return;
    }

    let framebuffer = FRAMEBUFFER.lock();
    let Some(fb) = framebuffer.as_ref() else {
        return;
    };
    let mut cursor = TEXT.lock();
    let columns = console_columns(fb);
    let rows = console_rows(fb);

    match byte {
        b'\n' => {
            cursor.column = 0;
            cursor.row += 1;
        }
        b'\r' => cursor.column = 0,
        0x08 => {
            if cursor.column > 0 {
                cursor.column -= 1;
                clear_cell(fb, cursor.column, cursor.row);
            }
        }
        other => {
            clear_cell(fb, cursor.column, cursor.row);
            let glyph = &FONT[(other as usize) & 0x7F];
            for row in 0..8 {
                for column in 0..8 {
                    if glyph[row] & (1 << (7 - column)) == 0 {
                        continue;
                    }
                    for dy in 0..2 {
                        for dx in 0..2 {
                            put_pixel(
                                fb,
                                cell_x(cursor.column) + column * 2 + dx,
                                cell_y(cursor.row) + row * 2 + dy,
                                cursor.color,
                            );
                        }
                    }
                }
            }
            cursor.column += 1;
        }
    }

    if cursor.column >= columns {
        cursor.column = 0;
        cursor.row += 1;
    }
    if cursor.row >= rows {
        scroll_up(fb);
        cursor.row = rows - 1;
    }
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

#[inline]
fn get_pixel(fb: &Framebuffer, x: usize, y: usize) -> u32 {
    if x >= fb.width || y >= fb.height {
        return 0;
    }
    let (x, y) = mapped(fb, x, y);
    let offset = y * fb.pitch + x * fb.bytes_per_pixel;
    unsafe { core::ptr::read_volatile((fb.address + offset) as *const u32) }
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

/// Draw a string at an arbitrary pixel position (used for static chrome).
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

/// Draw the window chrome: a title bar, the console background, and a dock.
fn draw_desktop() {
    let framebuffer = FRAMEBUFFER.lock();
    let Some(fb) = framebuffer.as_ref() else {
        return;
    };
    let (width, height) = (fb.width, fb.height);

    clear_locked(fb, BG);

    // Title bar with an accent stripe and a hairline separator.
    fill_rect_locked(fb, 0, 0, width, HEADER_H, PANEL);
    fill_rect_locked(fb, 0, 0, width, 4, ACCENT);
    fill_rect_locked(fb, 0, HEADER_H - 1, width, 1, TITLEBAR);
    draw_text(fb, MARGIN_X, (HEADER_H - 16) / 2, "ZENITH OS", FG);
    let version = "v0.1.0";
    let version_x = width.saturating_sub(MARGIN_X + version.len() * COL_PITCH);
    draw_text(fb, version_x, (HEADER_H - 16) / 2, version, MUTED);

    // Dock at the bottom, centred, with a subtle accent divider.
    let dock_size = (height / 12).min(72);
    let dock_y = height.saturating_sub(dock_size + 16);
    let dock_w = dock_size * 7;
    let dock_x = (width.saturating_sub(dock_w)) / 2;
    fill_rect_locked(fb, dock_x, dock_y, dock_w, dock_size, PANEL);
    fill_rect_locked(fb, dock_x, dock_y, dock_w, 2, ACCENT);
    for index in 0..6 {
        let icon = if index % 2 == 0 { TITLEBAR } else { ACCENT };
        fill_rect_locked(fb, dock_x + 8 + index * (dock_size + 4), dock_y + 10,
                         dock_size - 16, dock_size - 20, icon);
    }
}
