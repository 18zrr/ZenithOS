//! Windows 11-inspired compositing window manager.
//!
//! A cached wallpaper and a full-screen frame buffer live in normal RAM. Every
//! dirty frame is composed there (windows, taskbar, Start menu, cursor) and
//! then blitted to the linear framebuffer in one pass, which avoids the
//! per-pixel flicker of drawing directly to VRAM.
//!
//! Interaction is keyboard plus PS/2 mouse: click to focus, drag title bars,
//! minimise/maximise/close, use the taskbar and the Start menu.

use core::mem;
use core::sync::atomic::{AtomicBool, AtomicU64, Ordering};

use alloc::vec;
use alloc::vec::Vec;

use crate::display;
use crate::browser_bridge as browser;
use crate::font::{blend, COL_PITCH, FONT, GLYPH_HEIGHT, GLYPH_WIDTH, ROW_PITCH};
use crate::framebuffer as fb;
use crate::keyboard::{Key, KeyEvent};
use crate::logo;
use crate::{diagnostics, e1000, limine, mouse, rtc, start_menu, timer};

const MAX_WINDOWS: usize = 8;
const TITLEBAR_H: usize = 40;
/// Preallocated compositor surface, large enough for 4K. All three full-screen
/// buffers are allocated once so a mode change never needs to grow the bump
/// heap or leak the previous resolution's surfaces.
const MAX_SURFACE_PIXELS: usize = 3840 * 2160;
const DISPLAY_CARD_H: usize = 214;
const DISPLAY_PRESETS: [(usize, usize, &str); 4] = [
    (1280, 720, "720p"),
    (1920, 1080, "1080p"),
    (2560, 1440, "1440p"),
    (3840, 2160, "4K"),
];
const CUSTOM_CAPACITY: usize = 12;
/// Space reserved for the floating dock, including its bottom margin.
const TASKBAR_H: usize = 80;
const DOCK_H: usize = 64;
const DOCK_MARGIN: usize = 8;

const WALL_TOP: u32 = 0x0006_0C18;
const WALL_BOTTOM: u32 = 0x0014_2742;
const WINDOW_BG: u32 = 0x0011_1721;
const TERM_BG: u32 = 0x0008_0D16;
const TERM_LINE: u32 = 0x0023_3042;
const TITLE_ACTIVE: u32 = 0x001C_2635;
const TITLE_IDLE: u32 = 0x0015_1D29;
const BORDER_ACTIVE: u32 = 0x0046_5E7C;
const BORDER_IDLE: u32 = 0x0026_313F;
const TASKBAR_BG: u32 = 0x0017_212E;
const TASKBAR_LINE: u32 = 0x0038_4A60;
const TASKBAR_HOVER: u32 = 0x0028_3648;
const MENU_BG: u32 = 0x0017_202C;
const MENU_BORDER: u32 = 0x0039_4C62;
const MENU_TILE: u32 = 0x001E_2836;
const MENU_TILE_HOVER: u32 = 0x002B_3849;
const MENU_TILE_SELECTED: u32 = 0x0032_4A66;
const SEARCH_BG: u32 = 0x0011_1923;
const CARD_BG: u32 = 0x0018_2130;
const CARD_LINE: u32 = 0x0028_3546;
const FG: u32 = 0x00F5_F7FA;
const MUTED: u32 = 0x009A_A7B8;
const DIM: u32 = 0x006B_778A;
const CLOSE_HOVER: u32 = 0x00E8_1111;
const SHADOW: u32 = 0x0002_060C;
const STATUS_OK: u32 = 0x0055_C08A;
const STATUS_WARN: u32 = 0x00E0_A030;

const ACCENTS: [u32; 4] = [0x0000_78D4, 0x0003_8387, 0x0087_64B8, 0x00CA_5010];
const ACCENT_NAMES: [&str; 4] = ["Azure", "Teal", "Violet", "Amber"];

static DIRTY: AtomicBool = AtomicBool::new(false);
static SAFE_MODE: AtomicBool = AtomicBool::new(false);
static NEXT_FRAME: AtomicU64 = AtomicU64::new(0);
static NEXT_BROWSER_CHECK: AtomicU64 = AtomicU64::new(0);
static LAST_CARET: AtomicBool = AtomicBool::new(false);
static LAST_MINUTE: AtomicU64 = AtomicU64::new(u64::MAX);

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum WindowKind {
    Browser,
    Terminal,
    Monitor,
    Settings,
    About,
}

impl WindowKind {
    fn title(self) -> &'static str {
        match self {
            WindowKind::Browser => "Browser - Chromium companion",
            WindowKind::Terminal => "Terminal",
            WindowKind::Monitor => "System Monitor",
            WindowKind::Settings => "Settings",
            WindowKind::About => "About Zenith OS",
        }
    }

    fn color(self) -> u32 {
        match self {
            WindowKind::Browser => 0x0025_83BB,
            WindowKind::Terminal => 0x0000_78D4,
            WindowKind::Monitor => 0x004E_96C2,
            WindowKind::Settings => 0x0071_BA78,
            WindowKind::About => 0x008A_4A9E,
        }
    }

    fn letter(self) -> u8 {
        match self {
            WindowKind::Browser => b'B',
            WindowKind::Terminal => b'T',
            WindowKind::Monitor => b'M',
            WindowKind::Settings => b'S',
            WindowKind::About => b'A',
        }
    }
}

const TASKBAR_KINDS: [WindowKind; 4] = [
    WindowKind::Terminal,
    WindowKind::Monitor,
    WindowKind::Settings,
    WindowKind::About,
];

#[derive(Clone, Copy)]
struct Window {
    kind: WindowKind,
    x: i32,
    y: i32,
    w: usize,
    h: usize,
    minimized: bool,
    maximized: bool,
    restore: (i32, i32, usize, usize),
}

#[derive(Clone, Copy)]
struct Drag {
    window: usize,
    offset_x: i32,
    offset_y: i32,
}

#[derive(Clone, Copy)]
struct Resize {
    window: usize,
    start_x: i32,
    start_y: i32,
    start_w: usize,
    start_h: usize,
}

struct Wm {
    initialized: bool,
    width: usize,
    height: usize,
    wallpaper: Vec<u32>,
    frame: Vec<u32>,
    presented: Vec<u32>,
    windows: [Option<Window>; MAX_WINDOWS],
    order: [usize; MAX_WINDOWS],
    order_len: usize,
    accent: u32,
    dragging: Option<Drag>,
    resizing: Option<Resize>,
    browser_buttons: u8,
    display_input: [u8; CUSTOM_CAPACITY],
    display_input_len: usize,
    display_input_active: bool,
    display_status: [u8; 64],
    display_status_len: usize,
}

impl Wm {
    const fn new() -> Self {
        Wm {
            initialized: false,
            width: 0,
            height: 0,
            wallpaper: Vec::new(),
            frame: Vec::new(),
            presented: Vec::new(),
            windows: [None; MAX_WINDOWS],
            order: [0; MAX_WINDOWS],
            order_len: 0,
            accent: ACCENTS[0],
            dragging: None,
            resizing: None,
            browser_buttons: 0,
            display_input: [0; CUSTOM_CAPACITY],
            display_input_len: 0,
            display_input_active: false,
            display_status: [0; 64],
            display_status_len: 0,
        }
    }
}

static WM: crate::sync::SpinLock<Wm> = crate::sync::SpinLock::new(Wm::new());

/// An action that must run outside the window-manager lock.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Action {
    None,
    Installer,
    /// Apply a live display mode outside the window-manager lock.
    DisplayMode(usize, usize),
}

// ---------------------------------------------------------------------------
// Small geometry and drawing helpers
// ---------------------------------------------------------------------------

#[derive(Clone, Copy)]
struct Rect {
    x: i32,
    y: i32,
    w: usize,
    h: usize,
}

impl Rect {
    const fn new(x: i32, y: i32, w: usize, h: usize) -> Self {
        Rect { x, y, w, h }
    }

    fn contains(&self, x: i32, y: i32) -> bool {
        self.w != 0
            && self.h != 0
            && x >= self.x
            && y >= self.y
            && x < self.x + self.w as i32
            && y < self.y + self.h as i32
    }

    fn right(&self) -> i32 {
        self.x + self.w as i32
    }
}

fn fill(
    frame: &mut [u32],
    stride: usize,
    height: usize,
    x: i32,
    y: i32,
    w: usize,
    h: usize,
    color: u32,
) {
    if w == 0 || h == 0 {
        return;
    }
    let x0 = x.max(0) as usize;
    let y0 = y.max(0) as usize;
    let x1 = ((x as i64 + w as i64).clamp(0, stride as i64)) as usize;
    let y1 = ((y as i64 + h as i64).clamp(0, height as i64)) as usize;
    if x0 >= x1 || y0 >= y1 || x0 >= stride || y0 >= height {
        return;
    }
    for row in y0..y1 {
        let start = row * stride;
        for column in x0..x1 {
            frame[start + column] = color;
        }
    }
}

fn rounded_rect(
    frame: &mut [u32],
    stride: usize,
    height: usize,
    x: i32,
    y: i32,
    w: usize,
    h: usize,
    radius: usize,
    color: u32,
) {
    if w == 0 || h == 0 {
        return;
    }
    let r = radius.min(w / 2).min(h / 2);
    if r == 0 {
        fill(frame, stride, height, x, y, w, h, color);
        return;
    }

    // Straight bands: the middle spans the full width, while the top and
    // bottom strips stop short of the rounded corners.
    fill(
        frame,
        stride,
        height,
        x,
        y + r as i32,
        w,
        h.saturating_sub(r * 2),
        color,
    );
    let middle_w = w.saturating_sub(r * 2);
    if middle_w > 0 {
        fill(frame, stride, height, x + r as i32, y, middle_w, r, color);
        fill(
            frame,
            stride,
            height,
            x + r as i32,
            y + (h - r) as i32,
            middle_w,
            r,
            color,
        );
    }

    // Corner blocks with one-pixel coverage anti-aliasing. Pixel centres are
    // measured against the corner circle, then the colour is blended by the
    // fraction of the pixel that falls inside the shape. Integer square roots
    // keep the kernel free of floating-point support.
    for (corner_x, corner_y) in [
        (x, y),
        (x + (w - r) as i32, y),
        (x, y + (h - r) as i32),
        (x + (w - r) as i32, y + (h - r) as i32),
    ] {
        for dy in 0..r {
            let py = corner_y + dy as i32;
            if py < 0 || py as usize >= height {
                continue;
            }
            let fy = (2 * r) as i64 - 2 * dy as i64 - 1;
            for dx in 0..r {
                let px = corner_x + dx as i32;
                if px < 0 || px as usize >= stride {
                    continue;
                }
                let fx = (2 * r) as i64 - 2 * dx as i64 - 1;
                let distance = isqrt(((fx * fx + fy * fy) as u64) << 8) as i64;
                let alpha = ((((2 * r as i64 + 1) * 16) - distance) * 255 / 32)
                    .clamp(0, 255) as u8;
                if alpha == 0 {
                    continue;
                }
                let index = py as usize * stride + px as usize;
                if alpha >= 254 {
                    frame[index] = color;
                } else {
                    frame[index] = blend(frame[index], color, alpha);
                }
            }
        }
    }
}

/// Blend a solid colour into a clipped rectangle.
fn blend_rect(
    frame: &mut [u32],
    stride: usize,
    height: usize,
    x: i32,
    y: i32,
    w: usize,
    h: usize,
    color: u32,
    alpha: u8,
) {
    if w == 0 || h == 0 || alpha == 0 {
        return;
    }
    let x0 = x.max(0) as usize;
    let y0 = y.max(0) as usize;
    let x1 = ((x as i64 + w as i64).clamp(0, stride as i64)) as usize;
    let y1 = ((y as i64 + h as i64).clamp(0, height as i64)) as usize;
    for row in y0..y1 {
        let start = row * stride;
        for column in x0..x1 {
            frame[start + column] = blend(frame[start + column], color, alpha);
        }
    }
}

/// Soft drop shadow built from translucent edge bands, so the cost stays
/// proportional to the window perimeter rather than its area.
fn soft_shadow(
    frame: &mut [u32],
    stride: usize,
    height: usize,
    x: i32,
    y: i32,
    w: usize,
    h: usize,
) {
    const SHADOW_COLOR: u32 = SHADOW;
    for (spread, alpha) in [(12i32, 22u8), (6, 34u8)] {
        let left = x + 4 - spread;
        let right = x + 4 + w as i32;
        blend_rect(frame, stride, height, left, y + 10, spread as usize, h, SHADOW_COLOR, alpha);
        blend_rect(frame, stride, height, right, y + 10, spread as usize, h, SHADOW_COLOR, alpha);
        blend_rect(frame, stride, height, x + 10, y + 6 - spread, w.saturating_sub(10), spread as usize, SHADOW_COLOR, alpha);
        blend_rect(frame, stride, height, x + 10, y + 6 + h as i32, w.saturating_sub(10), spread as usize, SHADOW_COLOR, alpha);
    }
}

/// Integer square root, used by the rounded-rectangle coverage calculation.
fn isqrt(value: u64) -> u64 {
    if value == 0 {
        return 0;
    }
    let mut x = value;
    let mut y = (x + 1) / 2;
    while y < x {
        x = y;
        y = (x + value / x) / 2;
    }
    x
}

fn put(frame: &mut [u32], stride: usize, height: usize, x: i32, y: i32, color: u32) {
    if x < 0 || y < 0 {
        return;
    }
    let (x, y) = (x as usize, y as usize);
    if x < stride && y < height {
        frame[y * stride + x] = color;
    }
}

fn draw_glyph(
    frame: &mut [u32],
    stride: usize,
    height: usize,
    x: i32,
    y: i32,
    byte: u8,
    color: u32,
) {
    let glyph = &FONT[(byte as usize) & 0x7F];
    for row in 0..GLYPH_HEIGHT {
        let py = y + row as i32;
        if py < 0 || py as usize >= height {
            continue;
        }
        for column in 0..GLYPH_WIDTH {
            let alpha = glyph[row][column];
            if alpha == 0 {
                continue;
            }
            let px = x + column as i32;
            if px < 0 || px as usize >= stride {
                continue;
            }
            let offset = py as usize * stride + px as usize;
            frame[offset] = blend(frame[offset], color, alpha);
        }
    }
}

fn draw_text(
    frame: &mut [u32],
    stride: usize,
    height: usize,
    x: i32,
    y: i32,
    max_x: i32,
    text: &str,
    color: u32,
) {
    for (index, byte) in text.bytes().enumerate() {
        let glyph_x = x + index as i32 * COL_PITCH as i32;
        if glyph_x + COL_PITCH as i32 > max_x {
            break;
        }
        draw_glyph(frame, stride, height, glyph_x, y, byte, color);
    }
}

fn draw_text_centered(
    frame: &mut [u32],
    stride: usize,
    height: usize,
    center_x: i32,
    y: i32,
    text: &str,
    color: u32,
) {
    let width = text.len() as i32 * COL_PITCH as i32;
    draw_text(
        frame,
        stride,
        height,
        center_x - width / 2,
        y,
        center_x + width / 2 + 1,
        text,
        color,
    );
}

fn draw_line(
    frame: &mut [u32],
    stride: usize,
    height: usize,
    x0: i32,
    y0: i32,
    x1: i32,
    y1: i32,
    color: u32,
) {
    let dx = (x1 - x0).abs();
    let dy = -(y1 - y0).abs();
    let sx = if x0 < x1 { 1 } else { -1 };
    let sy = if y0 < y1 { 1 } else { -1 };
    let mut error = dx + dy;
    let (mut x, mut y) = (x0, y0);
    loop {
        put(frame, stride, height, x, y, color);
        if x == x1 && y == y1 {
            break;
        }
        let doubled = error * 2;
        if doubled >= dy {
            error += dy;
            x += sx;
        }
        if doubled <= dx {
            error += dx;
            y += sy;
        }
    }
}

struct TextBuffer {
    bytes: [u8; 56],
    len: usize,
}

impl TextBuffer {
    fn new() -> Self {
        TextBuffer {
            bytes: [0; 56],
            len: 0,
        }
    }

    fn push(&mut self, byte: u8) {
        if self.len < self.bytes.len() {
            self.bytes[self.len] = byte;
            self.len += 1;
        }
    }

    fn push_str(&mut self, text: &str) {
        for &byte in text.as_bytes() {
            self.push(byte);
        }
    }

    fn push_dec(&mut self, mut value: u64) {
        if value == 0 {
            self.push(b'0');
            return;
        }
        let mut digits = [0u8; 20];
        let mut index = digits.len();
        while value > 0 {
            index -= 1;
            digits[index] = b'0' + (value % 10) as u8;
            value /= 10;
        }
        for &digit in &digits[index..] {
            self.push(digit);
        }
    }

    fn push_signed(&mut self, value: i32) {
        if value < 0 {
            self.push(b'-');
            self.push_dec(value.unsigned_abs() as u64);
        } else {
            self.push_dec(value as u64);
        }
    }

    fn push_time(&mut self, hour: u8, minute: u8) {
        if hour < 10 {
            self.push(b'0');
        }
        self.push_dec(hour as u64);
        self.push(b':');
        if minute < 10 {
            self.push(b'0');
        }
        self.push_dec(minute as u64);
    }

    fn push_hex_rgb(&mut self, value: u32) {
        const DIGITS: &[u8; 16] = b"0123456789ABCDEF";
        self.push(b'#');
        for shift in [16u32, 8, 0] {
            let byte = ((value >> shift) & 0xFF) as usize;
            self.push(DIGITS[byte >> 4]);
            self.push(DIGITS[byte & 0x0F]);
        }
    }

    fn as_str(&self) -> &str {
        core::str::from_utf8(&self.bytes[..self.len]).unwrap_or("")
    }
}

// ---------------------------------------------------------------------------
// Public entry points
// ---------------------------------------------------------------------------

/// Whether the compositor owns the screen.
pub fn initialized() -> bool {
    WM.lock().initialized
}

/// Reduce visual effects for the "safe graphics" Limine entry.
pub fn set_safe_mode(enabled: bool) {
    SAFE_MODE.store(enabled, Ordering::Relaxed);
}

/// Whether the compositor is running with reduced effects.
pub fn safe_mode() -> bool {
    SAFE_MODE.load(Ordering::Relaxed)
}

/// Mark the desktop as needing a repaint.
pub fn invalidate() {
    DIRTY.store(true, Ordering::Release);
}

/// Allocate the compositor surfaces and open the initial terminal window.
pub fn init() {
    let (width, height) = fb::dimensions();
    if width == 0 || height == 0 {
        return;
    }

    mouse::set_bounds(width, height);

    let mut wm = WM.lock();
    if wm.initialized {
        return;
    }
    wm.width = width;
    wm.height = height;
    wm.wallpaper = vec![0u32; MAX_SURFACE_PIXELS];
    fill_wallpaper(&mut wm.wallpaper, width, height);
    wm.frame = vec![0u32; MAX_SURFACE_PIXELS];
    let pixels = width.saturating_mul(height);
    let wallpaper = mem::take(&mut wm.wallpaper);
    wm.frame[..pixels].copy_from_slice(&wallpaper[..pixels]);
    wm.wallpaper = wallpaper;
    // Keep the capacity but start empty, so the first composite is a full
    // present; the damage path then compares only the active viewport.
    wm.presented = vec![0u32; MAX_SURFACE_PIXELS];
    wm.presented.clear();
    wm.initialized = true;
    wm.open_window(WindowKind::Terminal);
    drop(wm);

    diagnostics::serial_puts("[wm] compositor ");
    diagnostics::serial_put_dec(width as u64);
    diagnostics::serial_puts("x");
    diagnostics::serial_put_dec(height as u64);
    diagnostics::serial_puts(", mouse=");
    diagnostics::serial_put_dec(mouse::present() as u64);
    diagnostics::serial_puts("\n");

    invalidate();
}

/// Compose and present one frame immediately (used for boot and resets).
pub fn render_now() {
    if !initialized() {
        return;
    }
    render();
}

/// Apply a display mode requested from Settings. Runs outside the
/// window-manager lock so the compositor can rebuild itself afterwards.
pub fn apply_display(width: usize, height: usize) -> Result<(), &'static str> {
    match display::set_mode(width, height) {
        Ok(()) => {
            on_display_change(width, height);
            let mut wm = WM.lock();
            if wm.initialized {
                let mut text = TextBuffer::new();
                text.push_str("Applied ");
                text.push_dec(width as u64);
                text.push(b'x');
                text.push_dec(height as u64);
                wm.set_display_status(text.as_str());
            }
            drop(wm);
            invalidate();
            Ok(())
        }
        Err(message) => {
            let mut wm = WM.lock();
            if wm.initialized {
                wm.set_display_status(message);
            }
            drop(wm);
            invalidate();
            Err(message)
        }
    }
}

/// Rebuild the cached wallpaper and clamp windows after a mode change.
fn on_display_change(width: usize, height: usize) {
    let pixels = width.saturating_mul(height);
    if width == 0 || height == 0 || pixels > MAX_SURFACE_PIXELS {
        return;
    }

    mouse::set_bounds(width, height);

    let mut wm = WM.lock();
    if !wm.initialized || wm.wallpaper.len() < pixels || wm.frame.len() < pixels {
        return;
    }
    wm.width = width;
    wm.height = height;
    fill_wallpaper(&mut wm.wallpaper, width, height);
    let wallpaper = mem::take(&mut wm.wallpaper);
    wm.frame[..pixels].copy_from_slice(&wallpaper[..pixels]);
    wm.wallpaper = wallpaper;
    wm.presented.clear();

    let available_h = height.saturating_sub(TASKBAR_H);
    for slot in wm.windows.iter_mut() {
        let Some(window) = slot else {
            continue;
        };
        if window.maximized {
            window.x = 8;
            window.y = 8;
            window.w = width.saturating_sub(16);
            window.h = available_h.saturating_sub(16);
            continue;
        }
        window.w = window.w.min(width.saturating_sub(16)).max(320);
        window.h = window.h.min(available_h.saturating_sub(16)).max(220);
        let max_x = (width as i32 - window.w as i32).max(8);
        let max_y = (available_h as i32 - window.h as i32).max(8);
        window.x = window.x.clamp(8, max_x);
        window.y = window.y.clamp(8, max_y);
    }
    drop(wm);

    invalidate();
    render();
}

/// Keyboard handling for the Settings custom-resolution field. Returns an
/// action that the caller applies after releasing the window-manager lock.
pub fn settings_key(event: KeyEvent) -> Option<Action> {
    if !event.pressed {
        return None;
    }
    let mut wm = WM.lock();
    if !wm.initialized || !wm.settings_focused() || !wm.display_input_active {
        return None;
    }

    match event.key {
        Key::Char(byte) if byte.is_ascii_digit() || byte == b'x' || byte == b'X' => {
            if wm.display_input_len < CUSTOM_CAPACITY {
                let index = wm.display_input_len;
                wm.display_input[index] = byte;
                wm.display_input_len += 1;
                invalidate();
            }
            None
        }
        Key::Backspace => {
            if wm.display_input_len > 0 {
                wm.display_input_len -= 1;
                invalidate();
            }
            None
        }
        Key::Escape => {
            wm.display_input_active = false;
            wm.set_display_status("Custom resolution cancelled");
            invalidate();
            None
        }
        Key::Enter => {
            let input = &wm.display_input[..wm.display_input_len];
            match parse_dimensions(input) {
                Some((w, h)) => match display::validate(w, h) {
                    Ok(()) => {
                        wm.display_input_active = false;
                        Some(Action::DisplayMode(w, h))
                    }
                    Err(message) => {
                        wm.set_display_status(message);
                        invalidate();
                        None
                    }
                },
                None => {
                    wm.set_display_status("Type WIDTHxHEIGHT, for example 1920x1080");
                    invalidate();
                    None
                }
            }
        }
        _ => None,
    }
}

fn parse_dimensions(bytes: &[u8]) -> Option<(usize, usize)> {
    let separator = bytes
        .iter()
        .position(|byte| *byte == b'x' || *byte == b'X')?;
    let width = parse_decimal(&bytes[..separator])?;
    let height = parse_decimal(&bytes[separator + 1..])?;
    if width == 0 || height == 0 {
        return None;
    }
    Some((width, height))
}

fn parse_decimal(bytes: &[u8]) -> Option<usize> {
    if bytes.is_empty() {
        return None;
    }
    let mut value = 0usize;
    for byte in bytes {
        if !byte.is_ascii_digit() {
            return None;
        }
        value = value.checked_mul(10)?.checked_add((*byte - b'0') as usize)?;
    }
    Some(value)
}

/// Called from the shell loop. Rates-limit redraws to roughly 100 Hz.
pub fn tick() {
    if !initialized() {
        return;
    }

    let now = timer::ticks();
    if now >= NEXT_BROWSER_CHECK.load(Ordering::Relaxed) {
        NEXT_BROWSER_CHECK.store(now + 4, Ordering::Relaxed);
        if browser::changed() {
            let wm = WM.lock();
            if wm.find(WindowKind::Browser).and_then(|i| wm.windows[i]).is_some_and(|w| !w.minimized) {
                invalidate();
            }
        }
    }
    let caret_phase = terminal_focused() && (now / 25) % 2 == 0;
    if caret_phase != LAST_CARET.load(Ordering::Relaxed) {
        LAST_CARET.store(caret_phase, Ordering::Relaxed);
        invalidate();
    }

    // Repaint when the wall-clock minute changes, even with no input events.
    if now % 50 == 0 {
        let (hour, minute, _) = rtc::time();
        let value = hour as u64 * 60 + minute as u64;
        if value != LAST_MINUTE.load(Ordering::Relaxed) {
            LAST_MINUTE.store(value, Ordering::Relaxed);
            invalidate();
        }
    }

    if !DIRTY.load(Ordering::Acquire) {
        return;
    }
    if now < NEXT_FRAME.load(Ordering::Relaxed) {
        return;
    }
    NEXT_FRAME.store(now + 1, Ordering::Relaxed);
    render();
}

/// Toggle the Start menu (F1 or the taskbar button).
pub fn toggle_menu() {
    if !initialized() {
        return;
    }
    start_menu::toggle();
    invalidate();
}

/// Keyboard handling while the Start menu is open. Returns an action that must
/// be executed after the caller releases all window-manager locks.
pub fn menu_key(event: KeyEvent) -> Action {
    if !start_menu::is_open() || !event.pressed {
        return Action::None;
    }

    match event.key {
        Key::Escape => {
            start_menu::close();
            invalidate();
            Action::None
        }
        Key::Up => {
            start_menu::move_selection(-2);
            invalidate();
            Action::None
        }
        Key::Down => {
            start_menu::move_selection(2);
            invalidate();
            Action::None
        }
        Key::Left => {
            start_menu::move_selection(-1);
            invalidate();
            Action::None
        }
        Key::Right => {
            start_menu::move_selection(1);
            invalidate();
            Action::None
        }
        Key::Enter => activate_selected(),
        Key::Backspace => {
            start_menu::backspace();
            invalidate();
            Action::None
        }
        Key::Char(byte) if (0x20..=0x7E).contains(&byte) => {
            start_menu::type_char(byte);
            invalidate();
            Action::None
        }
        _ => Action::None,
    }
}

/// True when the Settings custom-resolution field owns keyboard input.
pub fn settings_input_active() -> bool {
    let wm = WM.lock();
    wm.initialized && wm.settings_focused() && wm.display_input_active
}

/// Raise and focus the terminal window. Returns false when no terminal exists.
pub fn focus_terminal() -> bool {
    let mut wm = WM.lock();
    if !wm.initialized {
        return false;
    }
    let Some(index) = wm.find(WindowKind::Terminal) else {
        return false;
    };
    if let Some(window) = wm.windows[index] {
        if window.minimized {
            let mut window = window;
            window.minimized = false;
            wm.windows[index] = Some(window);
        }
    }
    wm.focus_index(index);
    drop(wm);
    invalidate();
    true
}

/// True when the terminal is the topmost visible window, so typed characters
/// belong to the shell.
pub fn terminal_focused() -> bool {
    let wm = WM.lock();
    if !wm.initialized {
        return true;
    }
    match wm.top_visible() {
        Some(index) => wm
            .windows
            .get(index)
            .and_then(|window| *window)
            .map(|window| window.kind == WindowKind::Terminal)
            .unwrap_or(false),
        None => false,
    }
}

pub fn open_browser() {
    let mut wm = WM.lock();
    if wm.initialized {
        wm.open_window(WindowKind::Browser);
    } else {
        drop(wm);
        diagnostics::println("Browser requires the graphical desktop.");
    }
}

pub fn browser_key(event: KeyEvent) -> bool {
    let wm = WM.lock();
    if wm.top_visible().and_then(|i| wm.windows[i]).is_some_and(|w| w.kind == WindowKind::Browser) {
        browser::key(event);
        true
    } else {
        false
    }
}

/// Drain queued mouse events and update the desktop. Runs in the shell task.
pub fn pump() -> Action {
    let mut wm = WM.lock();
    if !wm.initialized {
        return Action::None;
    }

    let mut action = Action::None;
    while let Some(event) = mouse::take_event() {
        if wm.browser_pointer(event) { continue; }
        match event.kind {
            mouse::EVENT_MOVE => {
                wm.handle_move(event.x, event.y);
                invalidate();
            }
            mouse::EVENT_BUTTON_DOWN if event.button == mouse::BUTTON_LEFT => {
                action = wm.handle_click(event.x, event.y);
            }
            mouse::EVENT_BUTTON_UP if event.button == mouse::BUTTON_LEFT => {
                if wm.dragging.is_some() || wm.resizing.is_some() {
                    wm.dragging = None;
                    wm.resizing = None;
                    invalidate();
                }
            }
            mouse::EVENT_WHEEL => {
                if wm.terminal_at(event.x, event.y) {
                    fb::scroll_lines(event.scroll * 3);
                    invalidate();
                }
            }
            _ => {}
        }
        if action == Action::Installer {
            break;
        }
    }
    action
}

fn activate_selected() -> Action {
    let Some(app) = start_menu::selected_app() else {
        return Action::None;
    };
    start_menu::close();
    invalidate();
    WM.lock().launch(app)
}

// ---------------------------------------------------------------------------
// Compositor
// ---------------------------------------------------------------------------

fn render() {
    DIRTY.store(false, Ordering::Release);

    let mut wm = WM.lock();
    if !wm.initialized {
        return;
    }
    let mut frame = mem::take(&mut wm.frame);
    let stride = wm.width;
    let height = wm.height;
    let pixels = stride.saturating_mul(height);
    if frame.len() < pixels || wm.wallpaper.len() < pixels {
        wm.frame = frame;
        return;
    }

    frame[..pixels].copy_from_slice(&wm.wallpaper[..pixels]);

    let (mouse_x, mouse_y) = mouse::position();
    wm.draw_windows(&mut frame, mouse_x, mouse_y);
    wm.draw_taskbar(&mut frame, mouse_x, mouse_y);
    if start_menu::is_open() {
        wm.draw_start_menu(&mut frame, mouse_x, mouse_y);
    }
    draw_cursor(&mut frame, wm.width, wm.height, mouse_x, mouse_y);

    let full = fb::take_display_changed() || wm.presented.len() != pixels;
    if full {
        fb::present(&frame[..pixels]);
        wm.presented.clear();
        wm.presented.extend_from_slice(&frame[..pixels]);
    } else {
        // Present and remember contiguous runs of changed rows. Idle frames
        // therefore leave VRAM untouched, while a moving cursor costs only the
        // rows it crosses.
        let mut y = 0usize;
        while y < height {
            let start = y * stride;
            let end = start + stride;
            if frame[start..end] == wm.presented[start..end] {
                y += 1;
                continue;
            }
            let run_start = y;
            y += 1;
            while y < height {
                let row = y * stride;
                if frame[row..row + stride] == wm.presented[row..row + stride] {
                    break;
                }
                y += 1;
            }
            fb::present_rows(&frame, run_start, y);
            let range = run_start * stride..y * stride;
            wm.presented[range.clone()].copy_from_slice(&frame[range]);
        }
    }
    wm.frame = frame;
}

impl Wm {
    fn find(&self, kind: WindowKind) -> Option<usize> {
        self.windows
            .iter()
            .position(|window| window.map(|window| window.kind == kind).unwrap_or(false))
    }

    fn top_visible(&self) -> Option<usize> {
        for position in (0..self.order_len).rev() {
            let index = self.order[position];
            if let Some(window) = self.windows[index] {
                if !window.minimized {
                    return Some(index);
                }
            }
        }
        None
    }

    fn settings_focused(&self) -> bool {
        self.top_visible()
            .and_then(|index| self.windows.get(index).and_then(|window| *window))
            .map(|window| window.kind == WindowKind::Settings)
            .unwrap_or(false)
    }

    fn set_display_status(&mut self, text: &str) {
        let length = text.len().min(self.display_status.len() - 1);
        self.display_status[..length].copy_from_slice(&text.as_bytes()[..length]);
        self.display_status_len = length;
    }

    fn focus_index(&mut self, index: usize) {
        if let Some(position) = self.order[..self.order_len]
            .iter()
            .position(|&entry| entry == index)
        {
            for offset in position..self.order_len.saturating_sub(1) {
                self.order[offset] = self.order[offset + 1];
            }
            if self.order_len > 0 {
                self.order[self.order_len - 1] = index;
            }
        }
    }

    fn open_window(&mut self, kind: WindowKind) {
        if let Some(index) = self.find(kind) {
            if let Some(mut window) = self.windows[index] {
                window.minimized = false;
                self.windows[index] = Some(window);
            }
            self.focus_index(index);
            invalidate();
            return;
        }
        let Some(slot) = self.windows.iter().position(|window| window.is_none()) else {
            return;
        };
        let window = self.default_window(kind);
        self.windows[slot] = Some(window);
        if self.order_len < MAX_WINDOWS {
            self.order[self.order_len] = slot;
            self.order_len += 1;
        }
        self.focus_index(slot);
        invalidate();
    }

    fn close_window(&mut self, index: usize) {
        self.windows[index] = None;
        if let Some(position) = self.order[..self.order_len]
            .iter()
            .position(|&entry| entry == index)
        {
            for offset in position..self.order_len.saturating_sub(1) {
                self.order[offset] = self.order[offset + 1];
            }
            self.order_len = self.order_len.saturating_sub(1);
        }
        if self.dragging.map(|drag| drag.window) == Some(index) {
            self.dragging = None;
        }
        if self.resizing.map(|resize| resize.window) == Some(index) {
            self.resizing = None;
        }
        invalidate();
    }

    fn default_window(&self, kind: WindowKind) -> Window {
        let cascade = (self.order_len as i32 % 4) * 26;
        let available_h = self.height.saturating_sub(TASKBAR_H);
        let (w, h) = match kind {
            WindowKind::Browser => (
                (browser::WIDTH + 4).min(self.width.saturating_sub(24)),
                (browser::HEIGHT + TITLEBAR_H + 4).min(available_h.saturating_sub(16)),
            ),
            WindowKind::Terminal => (
                760usize.min(self.width.saturating_sub(120)).max(360),
                500usize.min(available_h.saturating_sub(96)).max(260),
            ),
            WindowKind::Monitor => (
                560usize.min(self.width.saturating_sub(100)).max(380),
                460usize.min(available_h.saturating_sub(60)).max(300),
            ),
            WindowKind::Settings => (
                600usize.min(self.width.saturating_sub(100)).max(380),
                580usize.min(available_h.saturating_sub(20)).max(320),
            ),
            WindowKind::About => (
                480usize.min(self.width.saturating_sub(100)).max(340),
                360usize.min(available_h.saturating_sub(80)).max(280),
            ),
        };
        let (x, y) = match kind {
            WindowKind::Terminal => (
                ((self.width.saturating_sub(w)) / 2) as i32 - 36,
                58,
            ),
            _ => (
                ((self.width.saturating_sub(w)) / 2) as i32 + cascade,
                ((available_h.saturating_sub(h)) / 2) as i32 + cascade,
            ),
        };
        Window {
            kind,
            x: x.max(8),
            y: y.max(8),
            w,
            h,
            minimized: false,
            maximized: false,
            restore: (x, y, w, h),
        }
    }

    fn handle_move(&mut self, x: i32, y: i32) {
        if let Some(resize) = self.resizing {
            let Some(mut window) = self.windows[resize.window] else {
                self.resizing = None;
                return;
            };
            let max_w = (self.width as i32 - window.x).max(320) as usize;
            let max_h = (self.height.saturating_sub(TASKBAR_H) as i32 - window.y).max(220) as usize;
            window.w =
                (resize.start_w as i32 + (x - resize.start_x)).clamp(320, max_w as i32) as usize;
            window.h =
                (resize.start_h as i32 + (y - resize.start_y)).clamp(220, max_h as i32) as usize;
            self.windows[resize.window] = Some(window);
            invalidate();
            return;
        }

        let Some(drag) = self.dragging else {
            return;
        };
        let Some(mut window) = self.windows[drag.window] else {
            self.dragging = None;
            return;
        };
        if window.maximized {
            return;
        }
        let new_x = x - drag.offset_x;
        let new_y = y - drag.offset_y;
        let min_x = 96 - window.w as i32;
        let max_x = self.width as i32 - 96;
        let max_y = (self.height.saturating_sub(TASKBAR_H)) as i32 - TITLEBAR_H as i32;
        window.x = new_x.clamp(min_x, max_x);
        window.y = new_y.clamp(0, max_y);
        self.windows[drag.window] = Some(window);
        invalidate();
    }

    fn browser_pointer(&mut self, event: mouse::MouseEvent) -> bool {
        if self.dragging.is_some() || self.resizing.is_some() { return false; }
        let Some(index) = self.find(WindowKind::Browser) else { return false };
        let window = self.windows[index].unwrap();
        let rect = browser_view(&window);
        if self.browser_buttons == 0 && (!rect.contains(event.x, event.y)
            || !matches!(self.hit_test(event.x, event.y), Hit::Body(i) if i == index)
            || start_menu::is_open()) { return false; }
        let x = ((event.x - rect.x).clamp(0, rect.w as i32 - 1) as usize) * browser::WIDTH / rect.w;
        let y = ((event.y - rect.y).clamp(0, rect.h as i32 - 1) as usize) * browser::HEIGHT / rect.h;
        let button = match event.button {
            mouse::BUTTON_LEFT => 1, mouse::BUTTON_MIDDLE => 2, mouse::BUTTON_RIGHT => 3,
            mouse::BUTTON_BACK => 8, mouse::BUTTON_FORWARD => 9, _ => 0,
        };
        match event.kind {
            mouse::EVENT_MOVE => browser::pointer(2, 0, x, y),
            mouse::EVENT_BUTTON_DOWN => {
                self.focus_index(index);
                self.browser_buttons |= event.button;
                browser::pointer(3, button, x, y);
            }
            mouse::EVENT_BUTTON_UP => {
                self.browser_buttons &= !event.button;
                browser::pointer(4, button, x, y);
            }
            mouse::EVENT_WHEEL => {
                for _ in 0..event.scroll.unsigned_abs().min(8) {
                    browser::pointer(5, if event.scroll > 0 { 4 } else { 5 }, x, y);
                }
            }
            _ => return false,
        }
        invalidate();
        true
    }

    fn handle_click(&mut self, x: i32, y: i32) -> Action {
        self.dragging = None;
        self.resizing = None;
        let hit = self.hit_test(x, y);

        if start_menu::is_open() {
            match hit {
                Hit::MenuApp(app) => {
                    start_menu::select_app(app);
                    start_menu::close();
                    invalidate();
                    return self.launch(app);
                }
                Hit::MenuInside => return Action::None,
                Hit::StartButton => {
                    start_menu::close();
                    invalidate();
                    return Action::None;
                }
                _ => {
                    start_menu::close();
                    invalidate();
                }
            }
        }

        match hit {
            Hit::StartButton => {
                start_menu::open();
                invalidate();
            }
            Hit::TaskbarApp(icon) => {
                let kind = TASKBAR_KINDS[icon.min(TASKBAR_KINDS.len() - 1)];
                if let Some(index) = self.find(kind) {
                    let window = self.windows[index].unwrap();
                    if window.minimized {
                        if let Some(mut window) = self.windows[index] {
                            window.minimized = false;
                            self.windows[index] = Some(window);
                        }
                        self.focus_index(index);
                    } else if self.top_visible() == Some(index) {
                        if let Some(mut window) = self.windows[index] {
                            window.minimized = true;
                            self.windows[index] = Some(window);
                        }
                        self.dragging = None;
                    } else {
                        self.focus_index(index);
                    }
                } else {
                    self.open_window(kind);
                }
                invalidate();
            }
            Hit::Titlebar(index) => {
                self.focus_index(index);
                if let Some(window) = self.windows[index] {
                    if window.maximized {
                        self.dragging = None;
                    } else {
                        self.dragging = Some(Drag {
                            window: index,
                            offset_x: x - window.x,
                            offset_y: y - window.y,
                        });
                    }
                }
                invalidate();
            }
            Hit::Minimize(index) => {
                if let Some(mut window) = self.windows[index] {
                    window.minimized = true;
                    self.windows[index] = Some(window);
                }
                self.dragging = None;
                self.resizing = None;
                self.focus_next_visible();
                invalidate();
            }
            Hit::Maximize(index) => {
                if let Some(mut window) = self.windows[index] {
                    if window.maximized {
                        let (rx, ry, rw, rh) = window.restore;
                        window.x = rx;
                        window.y = ry;
                        window.w = rw;
                        window.h = rh;
                        window.maximized = false;
                    } else {
                        window.restore = (window.x, window.y, window.w, window.h);
                        window.x = 8;
                        window.y = 8;
                        window.w = self.width.saturating_sub(16);
                        window.h = self.height.saturating_sub(TASKBAR_H + 16);
                        window.maximized = true;
                    }
                    self.windows[index] = Some(window);
                }
                self.dragging = None;
                invalidate();
            }
            Hit::Close(index) => self.close_window(index),
            Hit::Resize(index) => {
                self.focus_index(index);
                if let Some(window) = self.windows[index] {
                    self.resizing = Some(Resize {
                        window: index,
                        start_x: x,
                        start_y: y,
                        start_w: window.w,
                        start_h: window.h,
                    });
                }
                invalidate();
            }
            Hit::Body(index) => {
                self.focus_index(index);
                if let Some(window) = self.windows[index] {
                    if window.kind == WindowKind::Settings {
                        if let Some(swatch) = settings_swatch_at(&window, x, y) {
                            self.accent = ACCENTS[swatch.min(ACCENTS.len() - 1)];
                        }
                    }
                }
                invalidate();
            }
            Hit::Swatch(swatch) => {
                self.accent = ACCENTS[swatch.min(ACCENTS.len() - 1)];
                invalidate();
            }
            Hit::DisplayPreset(index) => {
                let (width, height, _) = DISPLAY_PRESETS[index.min(DISPLAY_PRESETS.len() - 1)];
                self.display_input_active = false;
                invalidate();
                return Action::DisplayMode(width, height);
            }
            Hit::DisplayCustom => {
                self.display_input_active = true;
                self.set_display_status("Type WIDTHxHEIGHT, then press Enter");
                invalidate();
            }
            Hit::DisplayApply => {
                match parse_dimensions(&self.display_input[..self.display_input_len]) {
                    Some((width, height)) => match display::validate(width, height) {
                        Ok(()) => {
                            self.display_input_active = false;
                            invalidate();
                            return Action::DisplayMode(width, height);
                        }
                        Err(message) => {
                            self.set_display_status(message);
                            invalidate();
                        }
                    },
                    None => {
                        self.set_display_status("Type WIDTHxHEIGHT, for example 1920x1080");
                        invalidate();
                    }
                }
            }
            Hit::DisplayRefresh => {
                self.set_display_status("Refresh rate is controlled by the host display");
                invalidate();
            }
            Hit::MenuApp(app) => {
                // Only reachable when the menu was already closed above.
                invalidate();
                return self.launch(app);
            }
            Hit::MenuInside | Hit::Taskbar | Hit::Desktop => {}
        }
        Action::None
    }

    fn focus_next_visible(&mut self) {
        if let Some(index) = self.top_visible() {
            self.focus_index(index);
        }
    }

    /// Whether the point lies inside a visible terminal window.
    fn terminal_at(&self, x: i32, y: i32) -> bool {
        for position in 0..self.order_len {
            let index = self.order[position];
            if let Some(window) = self.windows[index] {
                if !window.minimized
                    && window.kind == WindowKind::Terminal
                    && window_contains(&window, x, y)
                {
                    return true;
                }
            }
        }
        false
    }

    fn launch(&mut self, app: usize) -> Action {
        match app {
            0 => self.open_window(WindowKind::Terminal),
            1 => self.open_window(WindowKind::Monitor),
            2 => self.open_window(WindowKind::Settings),
            3 => self.open_window(WindowKind::About),
            4 => return Action::Installer,
            5 => self.open_window(WindowKind::Browser),
            _ => {}
        }
        invalidate();
        Action::None
    }

    fn hit_test(&self, x: i32, y: i32) -> Hit {
        if start_menu::is_open() {
            let menu = MenuLayout::new(self.width, self.height);
            if menu.panel.contains(x, y) {
                for entry in 0..menu.count {
                    if menu.tiles[entry].contains(x, y) {
                        return Hit::MenuApp(menu.apps[entry]);
                    }
                }
                return Hit::MenuInside;
            }
        }

        let taskbar = TaskbarLayout::new(self.width, self.height);
        if taskbar.start.contains(x, y) {
            return Hit::StartButton;
        }
        for (index, rect) in taskbar.apps.iter().enumerate() {
            if rect.contains(x, y) {
                return Hit::TaskbarApp(index);
            }
        }
        if taskbar.panel.contains(x, y) {
            return Hit::Taskbar;
        }

        for position in (0..self.order_len).rev() {
            let index = self.order[position];
            let Some(window) = self.windows[index] else {
                continue;
            };
            if window.minimized || !window_contains(&window, x, y) {
                continue;
            }
            let [minimize, maximize, close] = control_rects(&window);
            if close.contains(x, y) {
                return Hit::Close(index);
            }
            if maximize.contains(x, y) {
                return Hit::Maximize(index);
            }
            if minimize.contains(x, y) {
                return Hit::Minimize(index);
            }
            if y < window.y + TITLEBAR_H as i32 {
                return Hit::Titlebar(index);
            }
            if !window.maximized
                && x >= window.x + window.w as i32 - 18
                && y >= window.y + window.h as i32 - 18
            {
                return Hit::Resize(index);
            }
            if window.kind == WindowKind::Settings {
                if let Some(swatch) = settings_swatch_at(&window, x, y) {
                    return Hit::Swatch(swatch);
                }
                if let Some(control) = display_control_at(&window, x, y) {
                    return control;
                }
            }
            return Hit::Body(index);
        }

        Hit::Desktop
    }

    // -- drawing ------------------------------------------------------------

    fn draw_windows(&self, frame: &mut [u32], mouse_x: i32, mouse_y: i32) {
        let active = self.top_visible();
        for position in 0..self.order_len {
            let index = self.order[position];
            let Some(window) = self.windows[index] else {
                continue;
            };
            if window.minimized {
                continue;
            }
            self.draw_window(frame, &window, active == Some(index), mouse_x, mouse_y);
        }
    }

    fn draw_window(
        &self,
        frame: &mut [u32],
        window: &Window,
        active: bool,
        mouse_x: i32,
        mouse_y: i32,
    ) {
        let (stride, height) = (self.width, self.height);
        let x = window.x;
        let y = window.y;
        let w = window.w;
        let h = window.h;
        if w < 80 || h < 80 {
            return;
        }

        // Drop shadow, border and surfaces. A perimeter shadow keeps the
        // wallpaper visible around the window instead of painting a black slab.
        if !window.maximized && !safe_mode() {
            soft_shadow(frame, stride, height, x, y, w, h);
        }
        let border = if active { BORDER_ACTIVE } else { BORDER_IDLE };
        rounded_rect(frame, stride, height, x, y, w, h, 12, border);
        let title_bg = if active { TITLE_ACTIVE } else { TITLE_IDLE };
        rounded_rect(frame, stride, height, x + 1, y + 1, w - 2, h - 2, 11, title_bg);
        rounded_rect(
            frame,
            stride,
            height,
            x + 1,
            y + TITLEBAR_H as i32,
            w - 2,
            h - TITLEBAR_H - 1,
            11,
            WINDOW_BG,
        );
        fill(
            frame,
            stride,
            height,
            x + 1,
            y + TITLEBAR_H as i32,
            w - 2,
            10,
            WINDOW_BG,
        );
        fill(
            frame,
            stride,
            height,
            x + 1,
            y + TITLEBAR_H as i32 - 1,
            w - 2,
            1,
            border,
        );
        // A faint sheen across the title bar separates it from the body.
        fill(
            frame,
            stride,
            height,
            x + 12,
            y + 1,
            w.saturating_sub(24),
            1,
            0x0032_4258,
        );
        if active {
            fill(
                frame,
                stride,
                height,
                x + 14,
                y,
                w.saturating_sub(28),
                2,
                self.accent,
            );
        }

        // App icon and title.
        let icon_x = x + 12;
        let icon_y = y + (TITLEBAR_H as i32 - 22) / 2;
        rounded_rect(frame, stride, height, icon_x, icon_y, 22, 22, 6, window.kind.color());
        draw_glyph(
            frame,
            stride,
            height,
            icon_x + (22 - GLYPH_WIDTH as i32) / 2,
            icon_y + (22 - GLYPH_HEIGHT as i32) / 2,
            window.kind.letter(),
            0x00FF_FFFF,
        );
        let title_color = if active { FG } else { MUTED };
        draw_text(
            frame,
            stride,
            height,
            x + 42,
            y + (TITLEBAR_H as i32 - GLYPH_HEIGHT as i32) / 2,
            x + w as i32 - 160,
            window.kind.title(),
            title_color,
        );

        // Window controls with rounded hover pills.
        let controls = control_rects(window);
        for (control, glyph) in controls.iter().zip(["min", "max", "close"].iter()) {
            let hovered = control.contains(mouse_x, mouse_y);
            if hovered {
                let background = if *glyph == "close" {
                    CLOSE_HOVER
                } else {
                    0x0032_3F50
                };
                rounded_rect(
                    frame,
                    stride,
                    height,
                    control.x + 5,
                    control.y + 5,
                    control.w.saturating_sub(10),
                    control.h.saturating_sub(10),
                    7,
                    background,
                );
            }
            let center_x = control.x + control.w as i32 / 2;
            let center_y = control.y + control.h as i32 / 2;
            let color = if *glyph == "close" || hovered {
                0x00FF_FFFF
            } else {
                MUTED
            };
            match *glyph {
                "min" => fill(frame, stride, height, center_x - 6, center_y, 12, 2, color),
                "max" => {
                    fill(frame, stride, height, center_x - 5, center_y - 5, 10, 1, color);
                    fill(frame, stride, height, center_x - 5, center_y + 5, 10, 1, color);
                    fill(frame, stride, height, center_x - 5, center_y - 5, 1, 11, color);
                    fill(frame, stride, height, center_x + 4, center_y - 5, 1, 11, color);
                }
                _ => {
                    draw_line(
                        frame,
                        stride,
                        height,
                        center_x - 5,
                        center_y - 5,
                        center_x + 5,
                        center_y + 5,
                        color,
                    );
                    draw_line(
                        frame,
                        stride,
                        height,
                        center_x + 5,
                        center_y - 5,
                        center_x - 5,
                        center_y + 5,
                        color,
                    );
                }
            }
        }

        match window.kind {
            WindowKind::Browser => {
                let r = browser_view(window);
                if !browser::draw(frame, stride, height, r.x, r.y, r.w, r.h) {
                    draw_text(frame, stride, height, x + 16, y + 60, x + w as i32 - 8,
                        "Start the VM with python3 tools/run_browser.py", FG);
                }
            }
            WindowKind::Terminal => self.draw_terminal(frame, window, active),
            WindowKind::Monitor => self.draw_monitor(frame, window),
            WindowKind::Settings => self.draw_settings(frame, window, mouse_x, mouse_y),
            WindowKind::About => self.draw_about(frame, window),
        }

        // Resize grip in the bottom-right corner.
        if !window.maximized {
            let right = x + w as i32 - 3;
            let bottom = y + h as i32 - 3;
            for step in 0..3 {
                let offset = 3 + step * 4;
                draw_line(
                    frame,
                    stride,
                    height,
                    right - offset,
                    bottom,
                    right,
                    bottom - offset,
                    0x005A_6068,
                );
            }
        }
    }

    fn draw_terminal(&self, frame: &mut [u32], window: &Window, active: bool) {
        let (stride, height) = (self.width, self.height);

        // Inset console panel: the terminal reads as a recessed dark well
        // inside the window rather than text painted directly on the chrome.
        let panel_x = window.x + 12;
        let panel_y = window.y + TITLEBAR_H as i32 + 10;
        let panel_w = window.w.saturating_sub(24);
        let panel_h = window.h.saturating_sub(TITLEBAR_H + 22);
        if panel_w < 40 || panel_h < 40 {
            return;
        }
        rounded_rect(
            frame, stride, height, panel_x, panel_y, panel_w, panel_h, 9, TERM_LINE,
        );
        rounded_rect(
            frame,
            stride,
            height,
            panel_x + 1,
            panel_y + 1,
            panel_w - 2,
            panel_h - 2,
            8,
            TERM_BG,
        );

        let left = panel_x + 10;
        let top = panel_y + 8;
        let right = panel_x + panel_w as i32 - 10;
        let bottom = panel_y + panel_h as i32 - 8;
        if right <= left || bottom <= top {
            return;
        }

        let columns = (((right - left) as usize) / COL_PITCH).clamp(1, 100);
        let rows = (((bottom - top) as usize) / ROW_PITCH).clamp(1, 40);
        fb::set_console_size(columns, rows);

        let clip_w = (right - left) as usize;
        let clip_h = (bottom - top) as usize;
        fb::draw_console_into(
            frame,
            stride,
            height,
            left as usize,
            top as usize,
            clip_w,
            clip_h,
            FG,
        );

        if active && fb::scroll_offset() == 0 && (timer::ticks() / 25) % 2 == 0 {
            let (column, row) = fb::console_cursor();
            if column < columns && row < rows {
                let caret_x = left + (column * COL_PITCH) as i32;
                let caret_y = top + (row * ROW_PITCH) as i32;
                if caret_x + 2 <= right && caret_y + GLYPH_HEIGHT as i32 <= bottom {
                    fill(
                        frame,
                        stride,
                        height,
                        caret_x,
                        caret_y,
                        2,
                        GLYPH_HEIGHT,
                        self.accent,
                    );
                }
            }
        }

        let offset = fb::scroll_offset();
        if offset > 0 {
            let label = "SCROLLBACK";
            let pill_w = label.len() * COL_PITCH + 20;
            let pill_x = right - pill_w as i32 - 10;
            let pill_y = bottom - 28;
            rounded_rect(
                frame,
                stride,
                height,
                pill_x,
                pill_y,
                pill_w,
                22,
                9,
                0x002A_3644,
            );
            draw_text(
                frame,
                stride,
                height,
                pill_x + 10,
                pill_y + 1,
                pill_x + pill_w as i32,
                label,
                self.accent,
            );
        }
    }

    /// Draw a rounded card surface with an accent section title.
    fn card(
        &self,
        frame: &mut [u32],
        x: i32,
        y: i32,
        w: usize,
        h: usize,
        title: &str,
    ) {
        let stride = self.width;
        let height = self.height;
        rounded_rect(frame, stride, height, x, y, w, h, 12, CARD_LINE);
        rounded_rect(
            frame,
            stride,
            height,
            x + 1,
            y + 1,
            w.saturating_sub(2),
            h.saturating_sub(2),
            11,
            CARD_BG,
        );
        fill(
            frame,
            stride,
            height,
            x + 18,
            y + 1,
            w.saturating_sub(36),
            1,
            0x002D_3C4F,
        );
        draw_text(frame, stride, height, x + 14, y + 12, x + w as i32 - 14, title, self.accent);
    }

    fn draw_monitor(&self, frame: &mut [u32], window: &Window) {
        let stride = self.width;
        let height = self.height;
        let x = window.x + 16;
        let y = window.y + TITLEBAR_H as i32 + 14;
        let card_w = window.w.saturating_sub(32);
        let max_x = x + card_w as i32 - 14;
        let label_x = x + 14;

        let (regions, usable) = limine::memory_summary();
        let heap_kib = crate::allocator::used() as u64 / 1024;
        let (mouse_x, mouse_y) = mouse::position();

        // System card.
        self.card(frame, x, y, card_w, 116, "System");
        let mut value = TextBuffer::new();
        value.push_dec(timer::ticks() / 100);
        value.push_str(" s");
        self.monitor_row(frame, label_x, y + 42, max_x, "Uptime", value.as_str());

        let mut value = TextBuffer::new();
        value.push_dec(timer::ticks());
        self.monitor_row(frame, label_x, y + 66, max_x, "Timer ticks", value.as_str());

        let boot = if limine::is_active() { "Limine native" } else { "PVH direct" };
        self.monitor_row(frame, label_x, y + 90, max_x, "Boot path", boot);

        // Memory card with a proportional usage bar.
        let memory_y = y + 126;
        self.card(frame, x, memory_y, card_w, 136, "Memory");
        let mut value = TextBuffer::new();
        value.push_dec(usable / (1024 * 1024));
        value.push_str(" MiB / ");
        value.push_dec(regions);
        value.push_str(" regions");
        self.monitor_row(frame, label_x, memory_y + 42, max_x, "Usable", value.as_str());

        let mut value = TextBuffer::new();
        value.push_dec(heap_kib);
        value.push_str(" KiB");
        self.monitor_row(frame, label_x, memory_y + 66, max_x, "Compositor heap", value.as_str());

        let bar_w = card_w.saturating_sub(28);
        let bar_y = memory_y + 94;
        rounded_rect(frame, stride, height, label_x, bar_y, bar_w, 8, 4, 0x0025_3140);
        let filled = (crate::allocator::used() * bar_w / crate::allocator::HEAP_SIZE).min(bar_w);
        if filled > 0 {
            rounded_rect(frame, stride, height, label_x, bar_y, filled, 8, 4, self.accent);
        }
        draw_text(
            frame,
            stride,
            height,
            label_x,
            bar_y + 14,
            max_x,
            "Heap usage of the 128 MiB pool",
            DIM,
        );

        // Display and input card.
        let input_y = memory_y + 146;
        self.card(frame, x, input_y, card_w, 116, "Display and input");

        let mut value = TextBuffer::new();
        value.push_dec(self.width as u64);
        value.push(b'x');
        value.push_dec(self.height as u64);
        value.push_str("x32");
        self.monitor_row(frame, label_x, input_y + 42, max_x, "Display", value.as_str());

        let mut value = TextBuffer::new();
        if mouse::present() {
            value.push_str(if mouse::usb_present() { "USB HID @ " } else { "PS/2 @ " });
            value.push_signed(mouse_x);
            value.push(b',');
            value.push_signed(mouse_y);
        } else {
            value.push_str("not detected");
        }
        self.monitor_row(frame, label_x, input_y + 66, max_x, "Mouse", value.as_str());
        self.monitor_row(
            frame,
            label_x,
            input_y + 90,
            max_x,
            "Keyboard",
            crate::keyboard::source(),
        );
    }

    fn monitor_row(
        &self,
        frame: &mut [u32],
        x: i32,
        y: i32,
        max_x: i32,
        label: &str,
        value: &str,
    ) {
        draw_text(frame, self.width, self.height, x, y, max_x, label, MUTED);
        draw_text(frame, self.width, self.height, x + 230, y, max_x, value, FG);
    }

    fn draw_settings(&self, frame: &mut [u32], window: &Window, mouse_x: i32, mouse_y: i32) {
        let stride = self.width;
        let height = self.height;
        let x = window.x + 16;
        let y = window.y + TITLEBAR_H as i32 + 14;
        let card_w = window.w.saturating_sub(32);
        let max_x = x + card_w as i32 - 14;
        let label_x = x + 14;

        // Appearance card.
        self.card(frame, x, y, card_w, 142, "Appearance");
        self.monitor_row(frame, label_x, y + 42, max_x, "Theme", "Fluent Dark");

        let accent_index = ACCENTS
            .iter()
            .position(|color| *color == self.accent)
            .unwrap_or(0);
        let mut accent = TextBuffer::new();
        accent.push_hex_rgb(self.accent);
        accent.push(b' ');
        accent.push_str(ACCENT_NAMES[accent_index]);
        self.monitor_row(frame, label_x, y + 66, max_x, "Accent", accent.as_str());

        let swatches = settings_swatches(window);
        for (index, swatch) in swatches.iter().enumerate() {
            let selected = ACCENTS[index] == self.accent;
            let border = if selected { FG } else { 0x004A_5B70 };
            rounded_rect(
                frame,
                stride,
                height,
                swatch.x - 2,
                swatch.y - 2,
                swatch.w + 4,
                swatch.h + 4,
                6,
                border,
            );
            rounded_rect(
                frame,
                stride,
                height,
                swatch.x,
                swatch.y,
                swatch.w,
                swatch.h,
                4,
                ACCENTS[index],
            );
            if swatch.contains(mouse_x, mouse_y) && !selected {
                rounded_rect(
                    frame,
                    stride,
                    height,
                    swatch.x - 3,
                    swatch.y - 3,
                    swatch.w + 6,
                    swatch.h + 6,
                    6,
                    MUTED,
                );
                rounded_rect(
                    frame,
                    stride,
                    height,
                    swatch.x,
                    swatch.y,
                    swatch.w,
                    swatch.h,
                    4,
                    ACCENTS[index],
                );
            }
        }

        // Display card: live mode selection for adapters with a guest-side
        // Bochs VBE interface, plus an honest notice for everything else.
        let controls = display_controls(window);
        let display_y = controls.display_y;
        self.card(frame, x, display_y, card_w, DISPLAY_CARD_H, "Display");

        let mut resolution = TextBuffer::new();
        resolution.push_dec(self.width as u64);
        resolution.push(b'x');
        resolution.push_dec(self.height as u64);
        resolution.push_str("x32  ");
        resolution.push_str(display::backend_name());
        self.monitor_row(
            frame,
            label_x,
            display_y + 40,
            max_x,
            "Current",
            resolution.as_str(),
        );

        draw_text(
            frame,
            stride,
            height,
            label_x,
            display_y + 64,
            max_x,
            "Resolution",
            MUTED,
        );

        for (index, preset) in controls.presets.iter().enumerate() {
            let (width, height_px, label) = DISPLAY_PRESETS[index];
            let selected = self.width == width && self.height == height_px;
            draw_button(
                frame,
                stride,
                height,
                preset,
                label,
                selected,
                preset.contains(mouse_x, mouse_y),
                self.accent,
            );
        }

        draw_text(
            frame,
            stride,
            height,
            label_x,
            display_y + 124,
            max_x,
            "Custom",
            MUTED,
        );

        let field_border = if self.display_input_active {
            self.accent
        } else {
            CARD_LINE
        };
        rounded_rect(
            frame,
            stride,
            height,
            controls.custom.x,
            controls.custom.y,
            controls.custom.w,
            controls.custom.h,
            5,
            field_border,
        );
        rounded_rect(
            frame,
            stride,
            height,
            controls.custom.x + 1,
            controls.custom.y + 1,
            controls.custom.w.saturating_sub(2),
            controls.custom.h.saturating_sub(2),
            4,
            SEARCH_BG,
        );
        let input = core::str::from_utf8(&self.display_input[..self.display_input_len])
            .unwrap_or("");
        let (field_text, field_color) = if input.is_empty() && !self.display_input_active {
            ("1920x1080", DIM)
        } else {
            (input, FG)
        };
        draw_text(
            frame,
            stride,
            height,
            controls.custom.x + 8,
            controls.custom.y + 3,
            controls.custom.x + controls.custom.w as i32 - 8,
            field_text,
            field_color,
        );
        draw_button(
            frame,
            stride,
            height,
            &controls.apply,
            "Apply",
            false,
            controls.apply.contains(mouse_x, mouse_y),
            self.accent,
        );

        self.monitor_row(
            frame,
            label_x,
            display_y + 154,
            max_x,
            "Refresh rate",
            if display::available() {
                "Host-controlled"
            } else {
                "Firmware-controlled"
            },
        );

        if self.display_status_len > 0 {
            let status = core::str::from_utf8(&self.display_status[..self.display_status_len])
                .unwrap_or("");
            let color = if status.starts_with("Applied") {
                STATUS_OK
            } else {
                STATUS_WARN
            };
            draw_text(
                frame,
                stride,
                height,
                label_x,
                display_y + 180,
                max_x,
                status,
                color,
            );
        }

        // Input card.
        let input_y = display_y + DISPLAY_CARD_H as i32 + 10;
        self.card(frame, x, input_y, card_w, 116, "Input");
        self.monitor_row(
            frame,
            label_x,
            input_y + 42,
            max_x,
            "Mouse",
            if mouse::present() { mouse::source() } else { "not detected" },
        );
        self.monitor_row(
            frame,
            label_x,
            input_y + 66,
            max_x,
            "Keyboard",
            if crate::keyboard::present() {
                crate::keyboard::source()
            } else {
                "not detected"
            },
        );
        self.monitor_row(
            frame,
            label_x,
            input_y + 90,
            max_x,
            "USB input",
            if crate::usb::present() {
                if crate::usb::keyboard_present() {
                    "Keyboard + mouse"
                } else {
                    "Boot mouse"
                }
            } else {
                "No UHCI controller"
            },
        );
    }

    fn draw_about(&self, frame: &mut [u32], window: &Window) {
        let stride = self.width;
        let height = self.height;
        let center = window.x + window.w as i32 / 2;
        let x = window.x + 24;
        let top = window.y + TITLEBAR_H as i32 + 12;
        let max_x = window.x + window.w as i32 - 24;

        // The brand mark anchors the window; text follows beneath it.
        logo::draw(
            frame,
            stride,
            height,
            center - 32,
            top,
            64,
        );
        let y = top + 78;
        draw_text_centered(frame, stride, height, center, y, "ZENITH OS", self.accent);
        draw_text_centered(frame, stride, height, center, y + 26, "Version 0.2.0", FG);
        draw_text_centered(
            frame,
            stride,
            height,
            center,
            y + 52,
            "Independent x86_64 operating system",
            MUTED,
        );

        let mut boot = TextBuffer::new();
        boot.push_str(if limine::is_active() {
            "Limine native"
        } else {
            "PVH direct"
        });
        boot.push_str(" - ");
        boot.push_str(limine::mode_name(limine::boot_mode()));

        let mut memory = TextBuffer::new();
        let (_, usable) = limine::memory_summary();
        memory.push_dec(usable / (1024 * 1024));
        memory.push_str(" MiB usable");

        let rows = [
            ("Boot", boot.as_str()),
            ("Display", ""),
            ("Memory", memory.as_str()),
            ("License", "MIT"),
        ];
        for (index, (label, value)) in rows.iter().enumerate() {
            let row_y = y + 82 + index as i32 * 24;
            draw_text(frame, stride, height, x, row_y, max_x, label, DIM);
            if *label == "Display" {
                let mut display = TextBuffer::new();
                display.push_dec(self.width as u64);
                display.push(b'x');
                display.push_dec(self.height as u64);
                draw_text(frame, stride, height, x + 150, row_y, max_x, display.as_str(), FG);
            } else {
                draw_text(frame, stride, height, x + 150, row_y, max_x, value, FG);
            }
        }
    }

    fn draw_taskbar(&self, frame: &mut [u32], mouse_x: i32, mouse_y: i32) {
        let stride = self.width;
        let height = self.height;
        let layout = TaskbarLayout::new(self.width, self.height);
        let panel = layout.panel;

        // Floating glass dock: a soft shadow, a one-pixel rim and a slightly
        // lighter inner surface. It hovers over the wallpaper instead of
        // spanning the screen edge-to-edge.
        if !safe_mode() {
            soft_shadow(frame, stride, height, panel.x, panel.y, panel.w, panel.h);
        }
        rounded_rect(
            frame,
            stride,
            height,
            panel.x,
            panel.y,
            panel.w,
            panel.h,
            18,
            TASKBAR_LINE,
        );
        rounded_rect(
            frame,
            stride,
            height,
            panel.x + 1,
            panel.y + 1,
            panel.w.saturating_sub(2),
            panel.h.saturating_sub(2),
            17,
            TASKBAR_BG,
        );
        fill(
            frame,
            stride,
            height,
            panel.x + 18,
            panel.y + 1,
            panel.w.saturating_sub(36),
            1,
            0x002C_3D52,
        );

        // Start button.
        if layout.start.contains(mouse_x, mouse_y) || start_menu::is_open() {
            rounded_rect(
                frame,
                stride,
                height,
                layout.start.x + 2,
                layout.start.y + 2,
                48,
                48,
                12,
                TASKBAR_HOVER,
            );
        }
        logo::draw(
            frame,
            stride,
            height,
            layout.start.x + 7,
            layout.start.y + 7,
            38,
        );

        // Pinned applications.
        for (index, kind) in TASKBAR_KINDS.iter().enumerate() {
            let rect = layout.apps[index];
            let hovered = rect.contains(mouse_x, mouse_y);
            if hovered {
                rounded_rect(
                    frame,
                    stride,
                    height,
                    rect.x + 2,
                    rect.y + 2,
                    48,
                    48,
                    12,
                    TASKBAR_HOVER,
                );
            }
            rounded_rect(
                frame,
                stride,
                height,
                rect.x + 9,
                rect.y + 7,
                34,
                34,
                9,
                kind.color(),
            );
            draw_glyph(
                frame,
                stride,
                height,
                rect.x + 9 + (34 - GLYPH_WIDTH as i32) / 2,
                rect.y + 7 + (34 - GLYPH_HEIGHT as i32) / 2,
                kind.letter(),
                0x00FF_FFFF,
            );

            if let Some(window) = self.find(*kind).and_then(|index| self.windows[index]) {
                let focused = self.top_visible() == self.find(*kind);
                let indicator = if focused { self.accent } else { 0x0055_6677 };
                rounded_rect(
                    frame,
                    stride,
                    height,
                    rect.x + 22,
                    rect.y + 45,
                    8,
                    4,
                    2,
                    indicator,
                );
                let _ = window;
            }
        }

        // Separator between the launcher cluster and the tray.
        let separator_x = layout.apps[3].x + 52 + 12;
        fill(
            frame,
            stride,
            height,
            separator_x,
            panel.y + 18,
            1,
            28,
            0x0030_4054,
        );

        // System tray: network link indicator, then the real-time clock.
        let link_up = e1000::link_up();
        let net_x = panel.right() - 126;
        let net_y = panel.y + 24;
        for (index, bar_height) in [5usize, 8, 11, 14].iter().enumerate() {
            let color = if link_up { 0x0060_C080 } else { 0x0050_5860 };
            fill(
                frame,
                stride,
                height,
                net_x + index as i32 * 4,
                net_y + 14 - *bar_height as i32,
                3,
                *bar_height,
                color,
            );
        }

        let (hour, minute, _) = rtc::time();
        let mut clock = TextBuffer::new();
        clock.push_time(hour, minute);
        draw_text(
            frame,
            stride,
            height,
            panel.right() - 16 - 5 * COL_PITCH as i32,
            panel.y + 22,
            panel.right() - 16,
            clock.as_str(),
            FG,
        );
    }

    fn draw_start_menu(&self, frame: &mut [u32], mouse_x: i32, mouse_y: i32) {
        let stride = self.width;
        let height = self.height;
        let menu = MenuLayout::new(self.width, self.height);
        let panel = menu.panel;

        rounded_rect(
            frame,
            stride,
            height,
            panel.x + 7,
            panel.y + 9,
            panel.w,
            panel.h,
            13,
            SHADOW,
        );
        rounded_rect(
            frame,
            stride,
            height,
            panel.x,
            panel.y,
            panel.w,
            panel.h,
            12,
            MENU_BORDER,
        );
        rounded_rect(
            frame,
            stride,
            height,
            panel.x + 1,
            panel.y + 1,
            panel.w - 2,
            panel.h - 2,
            11,
            MENU_BG,
        );

        logo::draw(frame, stride, height, panel.x + 20, panel.y + 10, 28);
        draw_text(
            frame,
            stride,
            height,
            panel.x + 56,
            panel.y + 18,
            panel.right(),
            "Start",
            FG,
        );
        draw_text(
            frame,
            stride,
            height,
            panel.right() - 24 - 10 * COL_PITCH as i32,
            panel.y + 20,
            panel.right() - 16,
            "Zenith 0.2",
            DIM,
        );

        // Search field.
        let search = menu.search;
        fill(
            frame,
            stride,
            height,
            search.x,
            search.y,
            search.w,
            search.h,
            SEARCH_BG,
        );
        fill(
            frame,
            stride,
            height,
            search.x,
            search.y + search.h as i32 - 1,
            search.w,
            1,
            self.accent,
        );
        // Magnifier.
        let glass_x = search.x + 20;
        let glass_y = search.y + 15;
        for (dx, dy) in [
            (-4i32, -4i32),
            (-3, -5),
            (-2, -5),
            (-1, -5),
            (0, -5),
            (1, -5),
            (2, -5),
            (3, -5),
            (4, -4),
            (5, -3),
            (5, -2),
            (5, -1),
            (5, 0),
            (5, 1),
            (5, 2),
            (5, 3),
            (4, 4),
            (3, 5),
            (2, 5),
            (1, 5),
            (0, 5),
            (-1, 5),
            (-2, 5),
            (-3, 5),
            (-4, 4),
            (-5, 3),
            (-5, 2),
            (-5, 1),
            (-5, 0),
            (-5, -1),
            (-5, -2),
            (-5, -3),
        ] {
            put(
                frame,
                stride,
                height,
                glass_x + dx,
                glass_y + dy,
                MUTED,
            );
        }
        draw_line(
            frame,
            stride,
            height,
            glass_x + 4,
            glass_y + 4,
            glass_x + 9,
            glass_y + 9,
            MUTED,
        );

        let (search_bytes, search_len) = start_menu::search_text();
        if search_len == 0 {
            draw_text(
                frame,
                stride,
                height,
                search.x + 38,
                search.y + 12,
                search.right() - 10,
                "Search apps",
                DIM,
            );
        } else {
            let text = core::str::from_utf8(&search_bytes[..search_len]).unwrap_or("");
            draw_text(
                frame,
                stride,
                height,
                search.x + 38,
                search.y + 12,
                search.right() - 10,
                text,
                FG,
            );
        }

        draw_text(
            frame,
            stride,
            height,
            panel.x + 24,
            panel.y + 108,
            panel.right(),
            "Pinned",
            MUTED,
        );

        if menu.count == 0 {
            draw_text(
                frame,
                stride,
                height,
                panel.x + 24,
                panel.y + 150,
                panel.right(),
                "No matching apps",
                DIM,
            );
        }

        let selected = start_menu::selection();
        for entry in 0..menu.count {
            let rect = menu.tiles[entry];
            let app = menu.apps[entry];
            let hovered = rect.contains(mouse_x, mouse_y);
            let background = if entry == selected {
                MENU_TILE_SELECTED
            } else if hovered {
                MENU_TILE_HOVER
            } else {
                MENU_TILE
            };
            rounded_rect(
                frame,
                stride,
                height,
                rect.x,
                rect.y,
                rect.w,
                rect.h,
                8,
                background,
            );
            let (_, color) = start_menu::APPS[app];
            rounded_rect(
                frame,
                stride,
                height,
                rect.x + 14,
                rect.y + 17,
                30,
                30,
                7,
                color,
            );
            draw_glyph(
                frame,
                stride,
                height,
                rect.x + 14 + (30 - GLYPH_WIDTH as i32) / 2,
                rect.y + 17 + (30 - GLYPH_HEIGHT as i32) / 2,
                start_menu::APPS[app].0.as_bytes()[0],
                0x00FF_FFFF,
            );
            draw_text(
                frame,
                stride,
                height,
                rect.x + 54,
                rect.y + 23,
                rect.right() - 8,
                start_menu::APPS[app].0,
                FG,
            );
        }

        let footer_y = panel.y + panel.h as i32 - 40;
        fill(
            frame,
            stride,
            height,
            panel.x + 20,
            footer_y,
            panel.w - 40,
            1,
            MENU_BORDER,
        );
        draw_text(
            frame,
            stride,
            height,
            panel.x + 24,
            footer_y + 12,
            panel.right() - 20,
            "F1 close",
            DIM,
        );
        draw_text(
            frame,
            stride,
            height,
            panel.right() - 24 - 10 * COL_PITCH as i32,
            footer_y + 12,
            panel.right() - 20,
            "Enter open",
            DIM,
        );
    }
}

// ---------------------------------------------------------------------------
// Layout structures and hit testing
// ---------------------------------------------------------------------------

enum Hit {
    StartButton,
    TaskbarApp(usize),
    Taskbar,
    MenuApp(usize),
    MenuInside,
    Titlebar(usize),
    Minimize(usize),
    Maximize(usize),
    Close(usize),
    Resize(usize),
    Body(usize),
    Swatch(usize),
    DisplayPreset(usize),
    DisplayCustom,
    DisplayApply,
    DisplayRefresh,
    Desktop,
}

struct TaskbarLayout {
    panel: Rect,
    start: Rect,
    apps: [Rect; 4],
}

impl TaskbarLayout {
    fn new(width: usize, height: usize) -> Self {
        let panel_y = height.saturating_sub(DOCK_H + DOCK_MARGIN) as i32;
        let panel_w = 430usize.min(width.saturating_sub(24));
        let panel_x = (width.saturating_sub(panel_w) / 2) as i32;
        let item = 52;
        let row_y = panel_y + (DOCK_H as i32 - item as i32) / 2;
        let group_x = panel_x + 12;
        TaskbarLayout {
            panel: Rect::new(panel_x, panel_y, panel_w, DOCK_H),
            start: Rect::new(group_x, row_y, item as usize, item as usize),
            apps: [
                Rect::new(group_x + 54, row_y, item as usize, item as usize),
                Rect::new(group_x + 108, row_y, item as usize, item as usize),
                Rect::new(group_x + 162, row_y, item as usize, item as usize),
                Rect::new(group_x + 216, row_y, item as usize, item as usize),
            ],
        }
    }
}

struct MenuLayout {
    panel: Rect,
    search: Rect,
    tiles: [Rect; start_menu::APP_COUNT],
    apps: [usize; start_menu::APP_COUNT],
    count: usize,
}

impl MenuLayout {
    fn new(width: usize, height: usize) -> Self {
        let panel_w = 470usize.min(width.saturating_sub(24));
        let panel_h = 440usize.min(height.saturating_sub(TASKBAR_H + 24));
        let panel_x = (width.saturating_sub(panel_w) / 2) as i32;
        let panel_y = (height.saturating_sub(TASKBAR_H + 8 + panel_h)) as i32;
        let mut apps = [0usize; start_menu::APP_COUNT];
        let count = start_menu::visible(&mut apps);
        let mut tiles = [Rect::new(0, 0, 0, 0); start_menu::APP_COUNT];
        let gap = 12usize;
        let tile_w = panel_w.saturating_sub(40 + gap) / 2;
        let tile_h = 62usize;
        for entry in 0..count {
            let column = entry % 2;
            let row = entry / 2;
            tiles[entry] = Rect::new(
                panel_x + 20 + (column * (tile_w + gap)) as i32,
                panel_y + 138 + (row * (tile_h + gap)) as i32,
                tile_w,
                tile_h,
            );
        }
        MenuLayout {
            panel: Rect::new(panel_x, panel_y, panel_w, panel_h),
            search: Rect::new(panel_x + 20, panel_y + 52, panel_w.saturating_sub(40), 40),
            tiles,
            apps,
            count,
        }
    }
}

fn window_contains(window: &Window, x: i32, y: i32) -> bool {
    x >= window.x
        && y >= window.y
        && x < window.x + window.w as i32
        && y < window.y + window.h as i32
}

fn browser_view(window: &Window) -> Rect {
    let w = window.w - 4;
    let h = window.h - TITLEBAR_H - 4;
    let vw = w.min(h * browser::WIDTH / browser::HEIGHT).max(1);
    let vh = (vw * browser::HEIGHT / browser::WIDTH).max(1);
    Rect::new(window.x + 2 + ((w - vw) / 2) as i32,
              window.y + TITLEBAR_H as i32 + 2 + ((h - vh) / 2) as i32, vw, vh)
}

fn control_rects(window: &Window) -> [Rect; 3] {
    let close = Rect::new(
        window.x + window.w as i32 - 46,
        window.y,
        46,
        TITLEBAR_H,
    );
    let maximize = Rect::new(close.x - 46, window.y, 46, TITLEBAR_H);
    let minimize = Rect::new(maximize.x - 46, window.y, 46, TITLEBAR_H);
    [minimize, maximize, close]
}

fn settings_swatches(window: &Window) -> [Rect; 4] {
    let start_x = window.x + 30;
    let start_y = window.y + TITLEBAR_H as i32 + 14 + 96;
    let mut swatches = [Rect::new(0, 0, 0, 0); 4];
    for (index, swatch) in swatches.iter_mut().enumerate() {
        *swatch = Rect::new(start_x + index as i32 * 46, start_y, 36, 22);
    }
    swatches
}

fn settings_swatch_at(window: &Window, x: i32, y: i32) -> Option<usize> {
    settings_swatches(window)
        .iter()
        .position(|swatch| swatch.contains(x, y))
}

struct DisplayControls {
    display_y: i32,
    presets: [Rect; 4],
    custom: Rect,
    apply: Rect,
    refresh: Rect,
}

fn display_controls(window: &Window) -> DisplayControls {
    let x = window.x + 16;
    let y = window.y + TITLEBAR_H as i32 + 14;
    let card_w = window.w.saturating_sub(32);
    let label_x = x + 14;
    let max_x = x + card_w as i32 - 14;
    let display_y = y + 152;

    let gap = 6usize;
    let available = (max_x - label_x).max(0) as usize;
    let button_w = available.saturating_sub(gap * 3) / 4;
    let mut presets = [Rect::new(0, 0, 0, 0); 4];
    for (index, preset) in presets.iter_mut().enumerate() {
        *preset = Rect::new(
            label_x + (index * (button_w + gap)) as i32,
            display_y + 84,
            button_w.max(30),
            26,
        );
    }

    let apply_w = 64usize;
    let apply_x = max_x - apply_w as i32;
    let field_x = label_x + 78;
    let field_w = (apply_x - 8 - field_x).max(40) as usize;
    let custom = Rect::new(field_x, display_y + 120, field_w, 24);
    let apply = Rect::new(apply_x, display_y + 120, apply_w, 24);
    let refresh = Rect::new(
        label_x,
        display_y + 154,
        (max_x - label_x).max(0) as usize,
        22,
    );

    DisplayControls {
        display_y,
        presets,
        custom,
        apply,
        refresh,
    }
}

fn display_control_at(window: &Window, x: i32, y: i32) -> Option<Hit> {
    let controls = display_controls(window);
    for (index, preset) in controls.presets.iter().enumerate() {
        if preset.contains(x, y) {
            return Some(Hit::DisplayPreset(index));
        }
    }
    if controls.custom.contains(x, y) {
        return Some(Hit::DisplayCustom);
    }
    if controls.apply.contains(x, y) {
        return Some(Hit::DisplayApply);
    }
    if controls.refresh.contains(x, y) {
        return Some(Hit::DisplayRefresh);
    }
    None
}

// ---------------------------------------------------------------------------
// Wallpaper and cursor
// ---------------------------------------------------------------------------

fn mix(a: u32, b: u32, t: usize, scale: usize) -> u32 {
    let t = t.min(scale) as u64;
    let scale = scale.max(1) as u64;
    let channel = |left: u32, right: u32, shift: u32| -> u32 {
        let l = ((left >> shift) & 0xFF) as u64;
        let r = ((right >> shift) & 0xFF) as u64;
        ((l * (scale - t) + r * t) / scale) as u32
    };
    (channel(a, b, 16) << 16) | (channel(a, b, 8) << 8) | channel(a, b, 0)
}

fn glow(
    buffer: &mut [u32],
    width: usize,
    height: usize,
    center_x: i64,
    center_y: i64,
    radius: i64,
    color: u32,
    strength: usize,
) {
    let radius_squared = radius * radius;
    let x0 = (center_x - radius).max(0) as usize;
    let x1 = ((center_x + radius) as usize).min(width);
    let y0 = (center_y - radius).max(0) as usize;
    let y1 = ((center_y + radius) as usize).min(height);
    for y in y0..y1 {
        let dy = y as i64 - center_y;
        for x in x0..x1 {
            let dx = x as i64 - center_x;
            let distance = dx * dx + dy * dy;
            if distance >= radius_squared {
                continue;
            }
            let falloff = ((radius_squared - distance) as u64 * strength as u64
                / radius_squared.max(1) as u64) as usize;
            let index = y * width + x;
            buffer[index] = mix(buffer[index], color, falloff, 100);
        }
    }
}

fn draw_button(
    frame: &mut [u32],
    stride: usize,
    height: usize,
    rect: &Rect,
    label: &str,
    selected: bool,
    hovered: bool,
    accent: u32,
) {
    let (fill_color, border, text_color) = if selected {
        (accent, FG, FG)
    } else if hovered {
        (MENU_TILE_HOVER, accent, FG)
    } else {
        (MENU_TILE, CARD_LINE, FG)
    };
    rounded_rect(
        frame, stride, height, rect.x, rect.y, rect.w, rect.h, 7, border,
    );
    rounded_rect(
        frame,
        stride,
        height,
        rect.x + 1,
        rect.y + 1,
        rect.w.saturating_sub(2),
        rect.h.saturating_sub(2),
        6,
        fill_color,
    );
    let text_width = label.len() as i32 * COL_PITCH as i32;
    let text_x = rect.x + (rect.w as i32 - text_width) / 2;
    draw_text(
        frame,
        stride,
        height,
        text_x.max(rect.x + 6),
        rect.y + 4,
        rect.x + rect.w as i32 - 4,
        label,
        text_color,
    );
}

fn fill_wallpaper(buffer: &mut [u32], width: usize, height: usize) {
    let pixels = width.saturating_mul(height);
    if width == 0 || height == 0 || buffer.len() < pixels {
        return;
    }
    let buffer = &mut buffer[..pixels];
    for y in 0..height {
        let color = mix(WALL_TOP, WALL_BOTTOM, y, height.max(1));
        let row = &mut buffer[y * width..(y + 1) * width];
        for pixel in row.iter_mut() {
            *pixel = color;
        }
    }

    // Soft blue and violet blooms, in the spirit of the Windows 11 wallpaper.
    // Safe graphics mode keeps the plain gradient to stay light on old GPUs.
    if !safe_mode() {
        glow(
            buffer,
            width,
            height,
            (width as i64 * 32) / 100,
            (height as i64 * 30) / 100,
            (height as i64 * 55) / 100,
            0x003A_6EA8,
            34,
        );
        glow(
            buffer,
            width,
            height,
            (width as i64 * 76) / 100,
            (height as i64 * 72) / 100,
            (height as i64 * 38) / 100,
            0x005A_3A88,
            22,
        );
    }

    // A dim brand watermark in the lower-right corner. It sits behind the
    // dock and window stack, so it reads as desktop texture rather than a
    // competing foreground element.
    let mark = (height / 5).clamp(120, 260);
    if width > mark + 48 && height > mark + 48 {
        logo::draw_dim(
            buffer,
            width,
            height,
            (width - mark - 36) as i32,
            (height - mark - height / 6) as i32,
            mark,
            30,
        );
    }

    dither(buffer, width, height);
}

/// Ordered 4x4 dithering: a ±1 LSB perturbation breaks up gradient banding
/// without changing the overall palette.
fn dither(buffer: &mut [u32], width: usize, height: usize) {
    const BAYER: [[i32; 4]; 4] = [
        [0, 8, 2, 10],
        [12, 4, 14, 6],
        [3, 11, 1, 9],
        [15, 7, 13, 5],
    ];
    for y in 0..height {
        let row = &mut buffer[y * width..(y + 1) * width];
        for (x, pixel) in row.iter_mut().enumerate() {
            let delta = (BAYER[y & 3][x & 3] - 8) / 8;
            if delta == 0 {
                continue;
            }
            let mut r = ((*pixel >> 16) & 0xFF) as i32 + delta;
            let mut g = ((*pixel >> 8) & 0xFF) as i32 + delta;
            let mut b = (*pixel & 0xFF) as i32 + delta;
            r = r.clamp(0, 255);
            g = g.clamp(0, 255);
            b = b.clamp(0, 255);
            *pixel = ((r as u32) << 16) | ((g as u32) << 8) | b as u32;
        }
    }
}

// A classic arrow cursor: '.' outline, '#' fill, ' ' transparent.
const CURSOR_ROWS: [&[u8]; 19] = [
    b"X.............",
    b"XX............",
    b"X#X...........",
    b"X##X..........",
    b"X###X.........",
    b"X####X........",
    b"X#####X.......",
    b"X######X......",
    b"X#######X.....",
    b"X########X....",
    b"X#########X...",
    b"X##########X..",
    b"X#####XXXXXX..",
    b"X##X##X.......",
    b"X#X.X##X......",
    b"XX...X##X.....",
    b"X.....X##X....",
    b"......X##X....",
    b".......XX.....",
];

fn draw_cursor(frame: &mut [u32], width: usize, height: usize, x: i32, y: i32) {
    let cursor_w = CURSOR_ROWS[0].len() as i32;
    let cursor_h = CURSOR_ROWS.len() as i32;
    let start_x = x.clamp(0, (width as i32 - cursor_w).max(0));
    let start_y = y.clamp(0, (height as i32 - cursor_h).max(0));
    for (row, row_data) in CURSOR_ROWS.iter().enumerate() {
        for (column, &pixel) in row_data.iter().enumerate() {
            let color = match pixel {
                b'X' => 0x0010_1010,
                b'#' => 0x00FF_FFFF,
                _ => continue,
            };
            put(
                frame,
                width,
                height,
                start_x + column as i32,
                start_y + row as i32,
                color,
            );
        }
    }
}
