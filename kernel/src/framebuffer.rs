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
    {
        let mut cursor = TEXT.lock();
        cursor.column = 0;
        cursor.row = 0;
    }
    clear(TEXT_BACKGROUND);
    draw_desktop();
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

const TEXT_CELL: usize = 16;      // 8x8 font scaled 2x
const TEXT_BACKGROUND: u32 = 0x002D3436;
const TEXT_COLUMNS: usize = 80;
const TEXT_ROWS: usize = 25;

/// Clear one character cell so old glyph pixels cannot bleed through.
fn clear_cell(fb: &Framebuffer, column: usize, row: usize) {
    let start_x = column * TEXT_CELL;
    let start_y = row * TEXT_CELL;
    for y in start_y..start_y + TEXT_CELL {
        for x in start_x..start_x + TEXT_CELL {
            put_pixel(fb, x, y, TEXT_BACKGROUND);
        }
    }
}

/// Scroll the console up by one text row and clear the newly exposed row.
fn scroll_up(fb: &Framebuffer) {
    for y in 0..fb.height.saturating_sub(TEXT_CELL) {
        for x in 0..fb.width {
            let pixel = get_pixel(fb, x, y + TEXT_CELL);
            put_pixel(fb, x, y, pixel);
        }
    }
    let last_row = fb.height.saturating_sub(TEXT_CELL);
    for y in last_row..fb.height {
        for x in 0..fb.width {
            put_pixel(fb, x, y, TEXT_BACKGROUND);
        }
    }
}

/// Write one character to the framebuffer console (2x scaled 8x8 glyph).
pub fn putc(byte: u8) {
    let mut cursor = TEXT.lock();
    let framebuffer = FRAMEBUFFER.lock();

    if byte == b'\n' {
        cursor.column = 0;
        cursor.row += 1;
    } else if byte == b'\r' {
        cursor.column = 0;
    } else {
        if let Some(fb) = framebuffer.as_ref() {
            // Clear the whole cell first, so overlapping text cannot accumulate.
            clear_cell(fb, cursor.column, cursor.row);
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
                                cursor.column * TEXT_CELL + column * 2 + dx,
                                cursor.row * TEXT_CELL + row * 2 + dy,
                                cursor.color,
                            );
                        }
                    }
                }
            }
        }
        cursor.column += 1;
    }

    if cursor.column >= TEXT_COLUMNS {
        cursor.column = 0;
        cursor.row += 1;
    }
    if cursor.row >= TEXT_ROWS {
        if let Some(fb) = framebuffer.as_ref() {
            scroll_up(fb);
        }
        cursor.row = TEXT_ROWS - 1;
    }
}

/// Write a string to the framebuffer console.
pub fn puts(text: &str) {
    for &byte in text.as_bytes() {
        putc(byte);
    }
}

/// Fill the entire screen with one colour.
pub fn clear(color: u32) {
    if let Some(fb) = FRAMEBUFFER.lock().as_ref() {
        for y in 0..fb.height {
            for x in 0..fb.width {
                put_pixel(fb, x, y, color);
            }
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
        for row in y..y.saturating_add(height) {
            for col in x..x.saturating_add(width) {
                put_pixel(fb, col, row, color);
            }
        }
    }
}

/// Draw a minimal desktop: background, top bar, a window and a dock.
fn draw_desktop() {
    let (width, height) = {
        let fb = FRAMEBUFFER.lock();
        match fb.as_ref() {
            Some(fb) => (fb.width, fb.height),
            None => return,
        }
    };

    const BACKGROUND: u32 = 0x002D3436;
    const TITLEBAR: u32 = 0x002D8FB0;
    const PANEL: u32 = 0x001E2729;
    const ACCENT: u32 = 0x00E17055;

    clear(BACKGROUND);

    let bar_height = (height / 18).max(24);
    fill_rect(0, 0, width, bar_height, PANEL);
    fill_rect(0, 0, width, 4, ACCENT);

    let window_x = width / 8;
    let window_y = height / 6;
    let window_w = width / 2;
    let window_h = height / 2;
    fill_rect(window_x, window_y, window_w, window_h, PANEL);
    fill_rect(window_x, window_y, window_w, bar_height / 2, TITLEBAR);

    let dock_size = (height / 12).min(72);
    let dock_y = height.saturating_sub(dock_size + 16);
    let dock_w = dock_size * 7;
    let dock_x = (width.saturating_sub(dock_w)) / 2;
    fill_rect(dock_x, dock_y, dock_w, dock_size, PANEL);
    for index in 0..6 {
        let icon = if index % 2 == 0 { TITLEBAR } else { ACCENT };
        fill_rect(dock_x + 8 + index * (dock_size + 4), dock_y + 8,
                  dock_size - 16, dock_size - 16, icon);
    }
}
