//! Linear framebuffer graphics, discovered from the Multiboot2 info structure.

use core::ptr::read_unaligned;
use core::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

use crate::diagnostics;
use crate::font::{blend, COL_PITCH, FONT, GLYPH_HEIGHT, GLYPH_WIDTH, ROW_PITCH};
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

/// Set when the display mapping changes and the next composite must be
/// presented in full, bypassing the row-damage optimisation.
static DISPLAY_CHANGED: AtomicBool = AtomicBool::new(false);

pub(crate) fn take_display_changed() -> bool {
    DISPLAY_CHANGED.swap(false, Ordering::AcqRel)
}

/// Repaint the desktop and reset the console to the top-left.
pub fn clear_screen() {
    if recovery_active() {
        recovery_clear();
        return;
    }
    reset_cursor();
    *SHADOW.lock() = [[0; SHADOW_COLS]; SHADOW_ROWS];
    reset_history();
    crate::wm::render_now();
}

/// Show the finished desktop and enable on-screen console output.
pub fn boot_complete() {
    reset_cursor();
    *SHADOW.lock() = [[0; SHADOW_COLS]; SHADOW_ROWS];
    reset_history();
    crate::wm::render_now();
    CONSOLE_READY.store(true, Ordering::Release);
}

/// Set framebuffer orientation (used to correct VMs with a rotated display).
pub fn set_orientation(flip_x: bool, flip_y: bool) {
    FLIP_X.store(flip_x, Ordering::Relaxed);
    FLIP_Y.store(flip_y, Ordering::Relaxed);
    DISPLAY_CHANGED.store(true, Ordering::Release);
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

const SCROLLBACK_ROWS: usize = 480;

struct Scrollback {
    rows: [[u8; SHADOW_COLS]; SCROLLBACK_ROWS],
    start: usize,
    len: usize,
    offset: usize,
}

impl Scrollback {
    const fn new() -> Self {
        Scrollback {
            rows: [[0; SHADOW_COLS]; SCROLLBACK_ROWS],
            start: 0,
            len: 0,
            offset: 0,
        }
    }

    fn push(&mut self, row: [u8; SHADOW_COLS]) {
        let slot = (self.start + self.len) % SCROLLBACK_ROWS;
        self.rows[slot] = row;
        if self.len < SCROLLBACK_ROWS {
            self.len += 1;
        } else {
            self.start = (self.start + 1) % SCROLLBACK_ROWS;
        }
    }

    fn row(&self, index: usize) -> &[u8; SHADOW_COLS] {
        &self.rows[(self.start + index) % SCROLLBACK_ROWS]
    }
}

static SCROLLBACK: SpinLock<Scrollback> = SpinLock::new(Scrollback::new());

fn reset_history() {
    // Callers hold no locks; SHADOW is reset separately.
    let mut history = SCROLLBACK.lock();
    history.start = 0;
    history.len = 0;
    history.offset = 0;
}

/// Scroll the terminal view through saved history. Positive values move
/// backwards in time, negative values return towards the live prompt.
pub fn scroll_lines(lines: i32) {
    if lines == 0 {
        return;
    }
    crate::interrupt::without_interrupts(|| {
        let mut history = SCROLLBACK.lock();
        if lines > 0 {
            history.offset = (history.offset + lines as usize).min(history.len);
        } else {
            history.offset = history.offset.saturating_sub((-lines) as usize);
        }
    });
    crate::wm::invalidate();
}

/// Number of lines currently scrolled back from the live prompt.
pub fn scroll_offset() -> usize {
    crate::interrupt::without_interrupts(|| SCROLLBACK.lock().offset)
}


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
    for row in 0..GLYPH_HEIGHT {
        let py = y + row;
        if py >= height {
            break;
        }
        for column in 0..GLYPH_WIDTH {
            let alpha = glyph[row][column];
            if alpha == 0 {
                continue;
            }
            let px = x + column;
            if px >= stride {
                continue;
            }
            let offset = py * stride + px;
            frame[offset] = blend(frame[offset], color, alpha);
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
    let history = SCROLLBACK.lock();
    let offset = history.offset.min(history.len);
    let columns = (clip_w / COL_PITCH).min(SHADOW_COLS);
    let rows = (clip_h / ROW_PITCH).min(SHADOW_ROWS);
    for row in 0..rows {
        let source = if row < offset {
            history.row(history.len - offset + row)
        } else if row - offset < SHADOW_ROWS {
            &shadow[row - offset]
        } else {
            break;
        };
        for column in 0..columns {
            let byte = source[column];
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

fn scroll_up(
    shadow: &mut [[u8; SHADOW_COLS]; SHADOW_ROWS],
    history: &mut Scrollback,
) {
    history.push(shadow[0]);
    history.offset = 0;
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
    let mut history = SCROLLBACK.lock();
    history.offset = 0;

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
        scroll_up(&mut shadow, &mut history);
        cursor.row = rows.saturating_sub(1);
    }
    drop(history);
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

#[inline]
fn blend_pixel(fb: &Framebuffer, x: usize, y: usize, color: u32, alpha: u8) {
    if alpha == 0 || x >= fb.width || y >= fb.height {
        return;
    }
    let (x, y) = mapped(fb, x, y);
    let offset = y * fb.pitch + x * fb.bytes_per_pixel;
    let destination = (fb.address + offset) as *mut u32;
    unsafe {
        let dst = core::ptr::read_volatile(destination);
        core::ptr::write_volatile(destination, blend(dst, color, alpha));
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

fn draw_byte(fb: &Framebuffer, x: usize, y: usize, byte: u8, color: u32) {
    let glyph = &FONT[(byte as usize) & 0x7F];
    for (row, alpha_row) in glyph.iter().enumerate() {
        for (column, &alpha) in alpha_row.iter().enumerate() {
            if alpha != 0 {
                blend_pixel(fb, x + column, y + row, color, alpha);
            }
        }
    }
}

fn draw_text(fb: &Framebuffer, x: usize, y: usize, text: &str, color: u32) {
    for (index, byte) in text.bytes().enumerate() {
        draw_byte(fb, x + index * COL_PITCH, y, byte, color);
    }
}

/// Copy a composited frame into the linear framebuffer.
///
/// The frame is `width * height` pixels in `0x00RRGGBB` form. Row copies use
/// 64-bit stores and honour the runtime display orientation.
pub fn present(frame: &[u32]) {
    present_rows(frame, 0, usize::MAX);
}

/// Copy only a range of source rows. The window manager uses this to present
/// damage runs instead of repainting unchanged scanlines.
pub fn present_rows(frame: &[u32], y_start: usize, y_end: usize) {
    let framebuffer = FRAMEBUFFER.lock();
    let Some(fb) = framebuffer.as_ref() else {
        return;
    };
    if fb.bytes_per_pixel != 4 || frame.len() < fb.width.saturating_mul(fb.height) {
        return;
    }
    let flip_x = FLIP_X.load(Ordering::Relaxed);
    let flip_y = FLIP_Y.load(Ordering::Relaxed);
    let y_end = y_end.min(fb.height);
    let y_start = y_start.min(y_end);

    if flip_y {
        for y in 0..fb.height {
            let source_y = fb.height - 1 - y;
            if source_y < y_start || source_y >= y_end {
                continue;
            }
            let source = &frame[source_y * fb.width..(source_y + 1) * fb.width];
            copy_row(fb, source, y, flip_x);
        }
    } else {
        for y in y_start..y_end {
            let source = &frame[y * fb.width..(y + 1) * fb.width];
            copy_row(fb, source, y, flip_x);
        }
    }
}

fn copy_row(fb: &Framebuffer, source: &[u32], y: usize, flip_x: bool) {
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

// ---- Recovery console ----------------------------------------------------

const RECOVERY_MAX_COLS: usize = 120;
const RECOVERY_MAX_ROWS: usize = 40;
const RECOVERY_HEADER: usize = 58;
const RECOVERY_FOOTER: usize = 30;
const RECOVERY_BG: u32 = 0x0007_0D16;
const RECOVERY_FG: u32 = 0x00E6_EDF5;
const RECOVERY_MUTED: u32 = 0x0080_90A4;
const RECOVERY_ACCENT: u32 = 0x005A_A8F0;

struct RecoveryConsole {
    active: bool,
    cols: usize,
    rows: usize,
    row: usize,
    col: usize,
    lines: [[u8; RECOVERY_MAX_COLS]; RECOVERY_MAX_ROWS],
}

static RECOVERY: SpinLock<RecoveryConsole> = SpinLock::new(RecoveryConsole {
    active: false,
    cols: 80,
    rows: 26,
    row: 0,
    col: 0,
    lines: [[0; RECOVERY_MAX_COLS]; RECOVERY_MAX_ROWS],
});

fn mix_color(a: u32, b: u32, t: usize, scale: usize) -> u32 {
    let t = t.min(scale) as u64;
    let scale = scale.max(1) as u64;
    let channel = |left: u32, right: u32, shift: u32| -> u32 {
        let l = ((left >> shift) & 0xFF) as u64;
        let r = ((right >> shift) & 0xFF) as u64;
        ((l * (scale - t) + r * t) / scale) as u32
    };
    (channel(a, b, 16) << 16) | (channel(a, b, 8) << 8) | channel(a, b, 0)
}

/// Whether the text recovery console owns the display.
pub fn recovery_active() -> bool {
    RECOVERY.lock().active
}

/// Console cell origin in screen pixels.
fn recovery_cell(cols: usize, rows: usize, fb: &Framebuffer, row: usize, col: usize) -> (usize, usize) {
    let width = cols * COL_PITCH;
    let height = rows * ROW_PITCH;
    let x = fb.width.saturating_sub(width) / 2 + col * COL_PITCH;
    let y = RECOVERY_HEADER + fb.height.saturating_sub(RECOVERY_HEADER + RECOVERY_FOOTER + height) / 2
        + row * ROW_PITCH;
    (x, y)
}

fn recovery_paint_chrome(fb: &Framebuffer) {
    // Deep, slightly graded background.
    for y in 0..fb.height {
        let color = mix_color(0x0003_0810, 0x000C_1828, y, fb.height.max(1));
        fill_rect_locked(fb, 0, y, fb.width, 1, color);
    }

    fill_rect_locked(fb, 0, 0, fb.width, RECOVERY_HEADER - 3, 0x0009_1220);
    fill_rect_locked(fb, 0, RECOVERY_HEADER - 3, fb.width, 2, RECOVERY_ACCENT);
    fill_rect_locked(fb, 0, RECOVERY_HEADER - 1, fb.width, 1, 0x0024_3B58);

    // Logo tile with a stylised Z.
    fill_rect_locked(fb, 22, 15, 28, 28, 0x0013_2A44);
    fill_rect_locked(fb, 22, 15, 28, 2, RECOVERY_ACCENT);
    draw_text(fb, 22 + (28 - GLYPH_WIDTH) / 2, 19, "Z", RECOVERY_FG);
    draw_text(fb, 62, 17, "ZENITH OS", RECOVERY_FG);
    draw_text(fb, 62 + 10 * COL_PITCH, 17, " RECOVERY CONSOLE", RECOVERY_ACCENT);
    let version = "v0.1.0";
    let version_x = fb.width.saturating_sub(22 + version.len() * COL_PITCH);
    draw_text(fb, version_x, 17, version, RECOVERY_MUTED);

    let footer_y = fb.height.saturating_sub(RECOVERY_FOOTER);
    fill_rect_locked(fb, 0, footer_y, fb.width, 1, 0x0020_3248);
    fill_rect_locked(fb, 0, footer_y + 1, fb.width, RECOVERY_FOOTER - 1, 0x0008_101C);
    draw_text(
        fb,
        22,
        footer_y + 6,
        "Type help for commands  |  F12 rotates the display  |  reboot for the desktop",
        RECOVERY_MUTED,
    );
}

fn recovery_repaint_locked(fb: &Framebuffer, console: &RecoveryConsole) {
    recovery_paint_chrome(fb);
    for row in 0..console.rows {
        for col in 0..console.cols {
            let byte = console.lines[row][col];
            if byte == 0 {
                continue;
            }
            let (x, y) = recovery_cell(console.cols, console.rows, fb, row, col);
            draw_byte(fb, x, y, byte, RECOVERY_FG);
        }
    }
    // Cursor underline.
    if console.row < console.rows && console.col < console.cols {
        let (x, y) = recovery_cell(console.cols, console.rows, fb, console.row, console.col);
        fill_rect_locked(fb, x, y + GLYPH_HEIGHT - 2, 8, 2, RECOVERY_ACCENT);
    }
}

/// Clear the recovery console and repaint its chrome.
pub fn recovery_clear() {
    let mut console = RECOVERY.lock();
    if !console.active {
        return;
    }
    console.lines = [[0; RECOVERY_MAX_COLS]; RECOVERY_MAX_ROWS];
    console.row = 0;
    console.col = 0;
    let framebuffer = FRAMEBUFFER.lock();
    if let Some(fb) = framebuffer.as_ref() {
        recovery_repaint_locked(fb, &console);
    }
}

/// Repaint the recovery console after a display transformation.
pub fn recovery_repaint() {
    let console = RECOVERY.lock();
    if !console.active {
        return;
    }
    let framebuffer = FRAMEBUFFER.lock();
    if let Some(fb) = framebuffer.as_ref() {
        recovery_repaint_locked(fb, &console);
    }
}

fn recovery_scroll_locked(fb: &Framebuffer, console: &mut RecoveryConsole) {
    for row in 0..console.rows.saturating_sub(1) {
        console.lines[row] = console.lines[row + 1];
    }
    console.lines[console.rows.saturating_sub(1)] = [0; RECOVERY_MAX_COLS];
    console.row = console.rows.saturating_sub(1);
    recovery_repaint_locked(fb, console);
}

fn recovery_erase_cursor(fb: &Framebuffer, console: &RecoveryConsole) {
    if console.row >= console.rows || console.col >= console.cols {
        return;
    }
    let (x, y) = recovery_cell(console.cols, console.rows, fb, console.row, console.col);
    fill_rect_locked(fb, x, y, COL_PITCH, ROW_PITCH, RECOVERY_BG);
    let byte = console.lines[console.row][console.col];
    if byte != 0 {
        draw_byte(fb, x, y, byte, RECOVERY_FG);
    }
}

fn recovery_draw_cursor(fb: &Framebuffer, console: &RecoveryConsole) {
    if console.row >= console.rows || console.col >= console.cols {
        return;
    }
    let (x, y) = recovery_cell(console.cols, console.rows, fb, console.row, console.col);
    fill_rect_locked(fb, x, y + GLYPH_HEIGHT - 2, 8, 2, RECOVERY_ACCENT);
}

/// Write one byte to the recovery console.
pub fn recovery_putc(byte: u8) {
    let mut console = RECOVERY.lock();
    if !console.active {
        return;
    }
    let framebuffer = FRAMEBUFFER.lock();
    let Some(fb) = framebuffer.as_ref() else {
        return;
    };
    recovery_erase_cursor(fb, &console);

    match byte {
        b'\n' => {
            console.col = 0;
            console.row += 1;
        }
        b'\r' => console.col = 0,
        0x08 => {
            if console.col > 0 {
                let row = console.row;
                let col = console.col - 1;
                console.col = col;
                console.lines[row][col] = b' ';
                let (x, y) = recovery_cell(console.cols, console.rows, fb, row, col);
                fill_rect_locked(fb, x, y, COL_PITCH, ROW_PITCH, RECOVERY_BG);
            }
        }
        b'\t' => {
            for _ in 0..4 {
                recovery_putc_locked(fb, &mut console, b' ');
            }
        }
        byte if (0x20..=0x7E).contains(&byte) => {
            recovery_putc_locked(fb, &mut console, byte);
        }
        _ => {}
    }

    if console.row >= console.rows {
        recovery_scroll_locked(fb, &mut console);
    }
    recovery_draw_cursor(fb, &console);
}

fn recovery_putc_locked(fb: &Framebuffer, console: &mut RecoveryConsole, byte: u8) {
    console.lines[console.row][console.col] = byte;
    let (x, y) = recovery_cell(console.cols, console.rows, fb, console.row, console.col);
    draw_byte(fb, x, y, byte, RECOVERY_FG);
    console.col += 1;
    if console.col >= console.cols {
        console.col = 0;
        console.row += 1;
        if console.row >= console.rows {
            recovery_scroll_locked(fb, console);
        }
    }
}

/// Write a string to the recovery console.
pub fn recovery_puts(message: &str) {
    for &byte in message.as_bytes() {
        recovery_putc(byte);
    }
}

/// Start the text recovery console on the graphical framebuffer.
pub fn recovery_start() {
    let mut console = RECOVERY.lock();
    let framebuffer = FRAMEBUFFER.lock();
    let Some(fb) = framebuffer.as_ref() else {
        return;
    };
    console.cols = (fb.width / COL_PITCH).clamp(40, RECOVERY_MAX_COLS);
    console.rows = ((fb.height.saturating_sub(RECOVERY_HEADER + RECOVERY_FOOTER)) / ROW_PITCH)
        .clamp(8, RECOVERY_MAX_ROWS);
    console.row = 0;
    console.col = 0;
    console.lines = [[0; RECOVERY_MAX_COLS]; RECOVERY_MAX_ROWS];
    console.active = true;
    recovery_paint_chrome(fb);
}

// ---- Boot splash ---------------------------------------------------------

const SPLASH_TOP: u32 = 0x0004_0710;
const SPLASH_BOTTOM: u32 = 0x000A_1424;
const SPLASH_FG: u32 = 0x00F5_F2FF;
const SPLASH_MUTED: u32 = 0x008A_87AA;
const SPLASH_ACCENT: u32 = 0x008A_6CFF;
const SPLASH_TRACK: u32 = 0x0015_1E2E;

fn fill_round_rect_locked(
    fb: &Framebuffer,
    x: usize,
    y: usize,
    width: usize,
    height: usize,
    radius: usize,
    color: u32,
) {
    if width == 0 || height == 0 {
        return;
    }
    let radius = radius.min(width / 2).min(height / 2);
    for dy in 0..height {
        let inset = if dy < radius {
            radius - dy
        } else if dy >= height.saturating_sub(radius) {
            dy - (height - radius) + 1
        } else {
            0
        };
        fill_rect_locked(
            fb,
            x + inset,
            y + dy,
            width.saturating_sub(inset * 2),
            1,
            color,
        );
    }
}

fn splash_bar(fb: &Framebuffer) -> (usize, usize, usize) {
    let bar_w = 420usize.min(fb.width.saturating_sub(120));
    let x = (fb.width.saturating_sub(bar_w)) / 2;
    let y = fb.height.saturating_sub(126);
    (x, y, bar_w)
}

fn centered_text(fb: &Framebuffer, y: usize, message: &str, color: u32) {
    let width = message.len() * COL_PITCH;
    let x = fb.width.saturating_sub(width) / 2;
    draw_text(fb, x, y, message, color);
}

fn spaced_width(message: &str, extra: usize) -> usize {
    if message.is_empty() {
        return 0;
    }
    message.len() * COL_PITCH + (message.len() - 1) * extra
}

fn draw_spaced(fb: &Framebuffer, x: usize, y: usize, message: &str, color: u32, extra: usize) {
    for (index, byte) in message.bytes().enumerate() {
        draw_byte(fb, x + index * (COL_PITCH + extra), y, byte, color);
    }
}

fn centered_spaced_text(
    fb: &Framebuffer,
    y: usize,
    message: &str,
    color: u32,
    extra: usize,
) {
    let width = spaced_width(message, extra);
    draw_spaced(fb, fb.width.saturating_sub(width) / 2, y, message, color, extra);
}

/// Paint the branded boot splash. Called during real initialisation, so the
/// progress bar reflects work actually being done rather than a delay.
pub fn splash_init() {
    let framebuffer = FRAMEBUFFER.lock();
    let Some(fb) = framebuffer.as_ref() else {
        return;
    };
    let (width, height) = (fb.width, fb.height);

    for y in 0..height {
        let color = mix_color(SPLASH_TOP, SPLASH_BOTTOM, y, height.max(1));
        fill_rect_locked(fb, 0, y, width, 1, color);
    }
    // A barely visible hairline of accent at the very top.
    fill_rect_locked(fb, 0, 0, width, 2, 0x0032_2768);

    let size = 150usize.min(width / 5).min(height / 4);
    let logo_x = (width.saturating_sub(size)) / 2;
    let logo_y = (height * 34 / 100).saturating_sub(size / 2).max(24);
    draw_splash_mark(fb, logo_x, logo_y, size);

    let word_y = logo_y + size + 20;
    centered_spaced_text(fb, word_y, "ZENITH OS", SPLASH_FG, 2);
    centered_spaced_text(
        fb,
        word_y + 30,
        "INDEPENDENT x86_64 OPERATING SYSTEM",
        SPLASH_MUTED,
        0,
    );
    let line_w = 54usize;
    fill_round_rect_locked(
        fb,
        (width.saturating_sub(line_w)) / 2,
        word_y + 62,
        line_w,
        2,
        2,
        SPLASH_ACCENT,
    );

    let (bar_x, bar_y, bar_w) = splash_bar(fb);
    fill_round_rect_locked(fb, bar_x, bar_y, bar_w, 7, 3, SPLASH_TRACK);
    centered_text(fb, bar_y - 34, "Starting Zenith OS...", SPLASH_MUTED);

    let footer_y = height.saturating_sub(32);
    draw_text(fb, 22, footer_y, "Zenith OS 0.1.0", SPLASH_MUTED);
    let right = "Limine native";
    draw_text(
        fb,
        width.saturating_sub(22 + right.len() * COL_PITCH),
        footer_y,
        right,
        SPLASH_MUTED,
    );
}

fn draw_splash_mark(fb: &Framebuffer, x: usize, y: usize, size: usize) {
    // Outer bloom first, then the mark itself with a screen blend.
    let radius = (size as i32 * 5) / 4;
    let cx = x as i32 + size as i32 / 2;
    let cy = y as i32 + size as i32 / 2;
    for oy in -radius..=radius {
        for ox in -radius..=radius {
            let distance = ox * ox + oy * oy;
            if distance >= radius * radius {
                continue;
            }
            let alpha = ((radius * radius - distance) as u64 * 26 / (radius * radius) as u64) as u8;
            let px = cx + ox;
            let py = cy + oy;
            if px < 0 || py < 0 {
                continue;
            }
            blend_pixel(fb, px as usize, py as usize, 0x0055_3CD8, alpha);
        }
    }

    let scale = (128u64 << 16) / size as u64;
    for dy in 0..size {
        let sample_y = (dy as u64 * scale + scale / 2).saturating_sub(1 << 15);
        let y0 = ((sample_y >> 16) as usize).min(127);
        let y1 = (y0 + 1).min(127);
        let fy = ((sample_y >> 8) & 0xFF) as u32;
        for dx in 0..size {
            let sample_x = (dx as u64 * scale + scale / 2).saturating_sub(1 << 15);
            let x0 = ((sample_x >> 16) as usize).min(127);
            let x1 = (x0 + 1).min(127);
            let fx = ((sample_x >> 8) & 0xFF) as u32;
            let (r, g, b) = crate::logo::bilinear_rgb(x0, y0, x1, y1, fx, fy);
            if r == 0 && g == 0 && b == 0 {
                continue;
            }
            let px = x + dx;
            let py = y + dy;
            let destination = unsafe {
                core::ptr::read_volatile(
                    (fb.address + py * fb.pitch + px * fb.bytes_per_pixel) as *const u32,
                )
            };
            let screen = |d: u32, s: u32| 255 - ((255 - d) * (255 - s)) / 255;
            let value = (screen((destination >> 16) & 0xFF, r) << 16)
                | (screen((destination >> 8) & 0xFF, g) << 8)
                | screen(destination & 0xFF, b);
            put_pixel(fb, px, py, value);
        }
    }
}

/// Update the splash progress bar (0..=100) and status message.
pub fn splash_step(status: &str, progress: usize) {
    let framebuffer = FRAMEBUFFER.lock();
    let Some(fb) = framebuffer.as_ref() else {
        return;
    };
    let (bar_x, bar_y, bar_w) = splash_bar(fb);
    // Clear only the status/progress band, leaving the mark and wordmark.
    for y in bar_y.saturating_sub(42)..(bar_y + 26).min(fb.height) {
        let color = mix_color(SPLASH_TOP, SPLASH_BOTTOM, y, fb.height.max(1));
        fill_rect_locked(fb, 0, y, fb.width, 1, color);
    }
    centered_text(fb, bar_y - 34, status, SPLASH_MUTED);

    fill_round_rect_locked(fb, bar_x, bar_y, bar_w, 7, 3, SPLASH_TRACK);
    let percent = progress.min(100);
    let filled = bar_w * percent / 100;
    if filled > 0 {
        fill_round_rect_locked(fb, bar_x, bar_y, filled.max(7), 7, 3, SPLASH_ACCENT);
        // Luminous leading edge.
        let shine_x = bar_x + filled.saturating_sub(18);
        let shine_w = filled.min(18).max(4);
        fill_round_rect_locked(fb, shine_x, bar_y + 1, shine_w, 5, 2, 0x00C8_B6FF);
    }
    let mut label = [0u8; 4];
    let mut length = 0usize;
    if percent >= 100 {
        label[0] = b'1';
        label[1] = b'0';
        label[2] = b'0';
        length = 3;
    } else {
        if percent >= 10 {
            label[length] = b'0' + (percent / 10) as u8;
            length += 1;
        }
        label[length] = b'0' + (percent % 10) as u8;
        length += 1;
    }
    label[length] = b'%';
    length += 1;
    let text = core::str::from_utf8(&label[..length]).unwrap_or("");
    draw_text(
        fb,
        bar_x + (bar_w.saturating_sub(text.len() * COL_PITCH)) / 2,
        bar_y + 14,
        text,
        SPLASH_MUTED,
    );
}
