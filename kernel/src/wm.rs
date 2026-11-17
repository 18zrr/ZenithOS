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

use crate::font::FONT;
use crate::framebuffer as fb;
use crate::{diagnostics, e1000, limine, mouse, rtc, start_menu, timer};

const MAX_WINDOWS: usize = 8;
const TITLEBAR_H: usize = 36;
const TASKBAR_H: usize = 48;
const PAD: usize = 14;
const COL_PITCH: usize = 16;
const ROW_PITCH: usize = 20;

const WALL_TOP: u32 = 0x000A_1728;
const WALL_BOTTOM: u32 = 0x001A_3A5E;
const WINDOW_BG: u32 = 0x001B_1B1B;
const TITLE_ACTIVE: u32 = 0x002E_2E2E;
const TITLE_IDLE: u32 = 0x0023_2323;
const BORDER_ACTIVE: u32 = 0x0048_5868;
const BORDER_IDLE: u32 = 0x0037_3B40;
const TASKBAR_BG: u32 = 0x001C_2430;
const TASKBAR_LINE: u32 = 0x0036_465A;
const TASKBAR_HOVER: u32 = 0x002A_3644;
const MENU_BG: u32 = 0x0022_2B38;
const MENU_BORDER: u32 = 0x0041_5163;
const MENU_TILE: u32 = 0x002A_3442;
const MENU_TILE_HOVER: u32 = 0x0034_4254;
const MENU_TILE_SELECTED: u32 = 0x003E_5874;
const SEARCH_BG: u32 = 0x0016_1E27;
const FG: u32 = 0x00F3_F3F3;
const MUTED: u32 = 0x00A0_A8B0;
const DIM: u32 = 0x0070_7880;
const CLOSE_HOVER: u32 = 0x00E8_1111;
const SHADOW: u32 = 0x0006_0D16;

const ACCENTS: [u32; 4] = [0x0000_78D4, 0x0003_8387, 0x0087_64B8, 0x00CA_5010];

static DIRTY: AtomicBool = AtomicBool::new(false);
static NEXT_FRAME: AtomicU64 = AtomicU64::new(0);
static LAST_CARET: AtomicBool = AtomicBool::new(false);
static LAST_MINUTE: AtomicU64 = AtomicU64::new(u64::MAX);

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum WindowKind {
    Terminal,
    Monitor,
    Settings,
    About,
}

impl WindowKind {
    fn title(self) -> &'static str {
        match self {
            WindowKind::Terminal => "Terminal",
            WindowKind::Monitor => "System Monitor",
            WindowKind::Settings => "Settings",
            WindowKind::About => "About Zenith OS",
        }
    }

    fn color(self) -> u32 {
        match self {
            WindowKind::Terminal => 0x0000_78D4,
            WindowKind::Monitor => 0x004E_96C2,
            WindowKind::Settings => 0x0071_BA78,
            WindowKind::About => 0x008A_4A9E,
        }
    }

    fn letter(self) -> u8 {
        match self {
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
    windows: [Option<Window>; MAX_WINDOWS],
    order: [usize; MAX_WINDOWS],
    order_len: usize,
    accent: u32,
    dragging: Option<Drag>,
    resizing: Option<Resize>,
}

impl Wm {
    const fn new() -> Self {
        Wm {
            initialized: false,
            width: 0,
            height: 0,
            wallpaper: Vec::new(),
            frame: Vec::new(),
            windows: [None; MAX_WINDOWS],
            order: [0; MAX_WINDOWS],
            order_len: 0,
            accent: ACCENTS[0],
            dragging: None,
            resizing: None,
        }
    }
}

static WM: crate::sync::SpinLock<Wm> = crate::sync::SpinLock::new(Wm::new());

/// An action that must run outside the window-manager lock.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Action {
    None,
    Installer,
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
    for dy in 0..h {
        let inset = if dy < r {
            r - dy
        } else if h > r && dy >= h - r {
            dy - (h - r) + 1
        } else {
            0
        };
        fill(
            frame,
            stride,
            height,
            x + inset as i32,
            y + dy as i32,
            w.saturating_sub(inset * 2),
            1,
            color,
        );
    }
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
    for row in 0..8 {
        for column in 0..8 {
            if glyph[row] & (1 << (7 - column)) == 0 {
                continue;
            }
            for dy in 0..2 {
                for dx in 0..2 {
                    put(
                        frame,
                        stride,
                        height,
                        x + (column * 2 + dx) as i32,
                        y + (row * 2 + dy) as i32,
                        color,
                    );
                }
            }
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
    wm.wallpaper = build_wallpaper(width, height);
    let mut frame = vec![0u32; width.saturating_mul(height)];
    frame.copy_from_slice(&wm.wallpaper);
    wm.frame = frame;
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

/// Called from the shell loop. Rates-limit redraws to roughly 50 Hz.
pub fn tick() {
    if !initialized() {
        return;
    }

    let now = timer::ticks();
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
    NEXT_FRAME.store(now + 2, Ordering::Relaxed);
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
pub fn menu_key(scancode: Option<u8>, ascii: Option<u8>) -> Action {
    if !start_menu::is_open() {
        return Action::None;
    }

    match scancode {
        Some(0x01) => {
            start_menu::close();
            invalidate();
            Action::None
        }
        Some(0x48) => {
            start_menu::move_selection(-2);
            invalidate();
            Action::None
        }
        Some(0x50) => {
            start_menu::move_selection(2);
            invalidate();
            Action::None
        }
        Some(0x4B) => {
            start_menu::move_selection(-1);
            invalidate();
            Action::None
        }
        Some(0x4D) => {
            start_menu::move_selection(1);
            invalidate();
            Action::None
        }
        _ => match ascii {
            Some(b'\r') | Some(b'\n') => activate_selected(),
            Some(0x08) => {
                start_menu::backspace();
                invalidate();
                Action::None
            }
            Some(byte) if (0x20..=0x7E).contains(&byte) => {
                start_menu::type_char(byte);
                invalidate();
                Action::None
            }
            _ => Action::None,
        },
    }
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

/// Drain queued mouse events and update the desktop. Runs in the shell task.
pub fn pump() -> Action {
    let mut wm = WM.lock();
    if !wm.initialized {
        return Action::None;
    }

    let mut action = Action::None;
    while let Some(event) = mouse::take_event() {
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
    match app {
        0 => {
            WM.lock().open_window(WindowKind::Terminal);
            Action::None
        }
        1 => {
            WM.lock().open_window(WindowKind::Monitor);
            Action::None
        }
        2 => {
            WM.lock().open_window(WindowKind::Settings);
            Action::None
        }
        3 => {
            WM.lock().open_window(WindowKind::About);
            Action::None
        }
        4 => Action::Installer,
        _ => Action::None,
    }
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
    if frame.len() < wm.width.saturating_mul(wm.height) {
        wm.frame = frame;
        return;
    }

    frame.copy_from_slice(&wm.wallpaper);

    let (mouse_x, mouse_y) = mouse::position();
    wm.draw_windows(&mut frame, mouse_x, mouse_y);
    wm.draw_taskbar(&mut frame, mouse_x, mouse_y);
    if start_menu::is_open() {
        wm.draw_start_menu(&mut frame, mouse_x, mouse_y);
    }
    draw_cursor(&mut frame, wm.width, wm.height, mouse_x, mouse_y);

    fb::present(&frame);
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
            WindowKind::Terminal => (
                760usize.min(self.width.saturating_sub(120)).max(360),
                500usize.min(available_h.saturating_sub(96)).max(260),
            ),
            WindowKind::Monitor => (
                560usize.min(self.width.saturating_sub(100)).max(380),
                360usize.min(available_h.saturating_sub(80)).max(260),
            ),
            WindowKind::Settings => (
                600usize.min(self.width.saturating_sub(100)).max(380),
                440usize.min(available_h.saturating_sub(80)).max(280),
            ),
            WindowKind::About => (
                480usize.min(self.width.saturating_sub(100)).max(340),
                320usize.min(available_h.saturating_sub(80)).max(240),
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

    fn launch(&mut self, app: usize) -> Action {
        match app {
            0 => self.open_window(WindowKind::Terminal),
            1 => self.open_window(WindowKind::Monitor),
            2 => self.open_window(WindowKind::Settings),
            3 => self.open_window(WindowKind::About),
            4 => return Action::Installer,
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
        if y >= taskbar.ty {
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

        // Drop shadow, border and surfaces.
        if !window.maximized {
            rounded_rect(frame, stride, height, x + 7, y + 9, w, h, 12, SHADOW);
        }
        let border = if active { BORDER_ACTIVE } else { BORDER_IDLE };
        rounded_rect(frame, stride, height, x, y, w, h, 10, border);
        let title_bg = if active { TITLE_ACTIVE } else { TITLE_IDLE };
        rounded_rect(frame, stride, height, x + 1, y + 1, w - 2, h - 2, 9, title_bg);
        rounded_rect(
            frame,
            stride,
            height,
            x + 1,
            y + TITLEBAR_H as i32,
            w - 2,
            h - TITLEBAR_H - 1,
            9,
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
        if active {
            fill(
                frame,
                stride,
                height,
                x + 10,
                y,
                w.saturating_sub(20),
                2,
                self.accent,
            );
        }

        // App icon and title.
        let icon_x = x + 12;
        let icon_y = y + (TITLEBAR_H as i32 - 18) / 2;
        rounded_rect(frame, stride, height, icon_x, icon_y, 18, 18, 5, window.kind.color());
        draw_glyph(
            frame,
            stride,
            height,
            icon_x + 1,
            icon_y + 1,
            window.kind.letter(),
            0x00FF_FFFF,
        );
        let title_color = if active { FG } else { MUTED };
        draw_text(
            frame,
            stride,
            height,
            x + 38,
            y + (TITLEBAR_H as i32 - 16) / 2,
            x + w as i32 - 160,
            window.kind.title(),
            title_color,
        );

        // Window controls with hover feedback.
        let controls = control_rects(window);
        for (control, glyph) in controls.iter().zip(["min", "max", "close"].iter()) {
            let hovered = control.contains(mouse_x, mouse_y);
            let background = if hovered {
                if *glyph == "close" {
                    CLOSE_HOVER
                } else {
                    0x0038_3838
                }
            } else {
                title_bg
            };
            if hovered || *glyph == "close" {
                fill(
                    frame,
                    stride,
                    height,
                    control.x,
                    control.y,
                    control.w,
                    control.h,
                    background,
                );
            }
            let center_x = control.x + control.w as i32 / 2;
            let center_y = control.y + control.h as i32 / 2;
            let color = if *glyph == "close" { 0x00FF_FFFF } else { MUTED };
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
        let left = (window.x + 1 + PAD as i32).max(0);
        let top = (window.y + TITLEBAR_H as i32 + 12).max(0);
        let right = (window.x + window.w as i32 - PAD as i32).min(stride as i32);
        let bottom = (window.y + window.h as i32 - 12).min(height as i32);
        if right <= left || bottom <= top {
            return;
        }

        let content_w = window.w.saturating_sub(2 + PAD * 2);
        let content_h = window
            .h
            .saturating_sub(TITLEBAR_H + 1 + 24);
        let columns = (content_w / COL_PITCH).clamp(1, 100);
        let rows = (content_h / ROW_PITCH).clamp(1, 40);
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

        if active && (timer::ticks() / 25) % 2 == 0 {
            let (column, row) = fb::console_cursor();
            if column < columns && row < rows {
                let caret_x = left + (column * COL_PITCH) as i32;
                let caret_y = top + (row * ROW_PITCH) as i32;
                if caret_x + 2 <= right && caret_y + 16 <= bottom {
                    fill(frame, stride, height, caret_x, caret_y, 2, 16, self.accent);
                }
            }
        }
    }

    fn draw_monitor(&self, frame: &mut [u32], window: &Window) {
        let stride = self.width;
        let height = self.height;
        let x = window.x + 20;
        let y = window.y + TITLEBAR_H as i32 + 18;
        let max_x = window.x + window.w as i32 - 20;

        draw_text(frame, stride, height, x, y, max_x, "System Monitor", self.accent);

        let (regions, usable) = limine::memory_summary();
        let heap_kib = crate::allocator::used() as u64 / 1024;
        let (mouse_x, mouse_y) = mouse::position();

        let mut value = TextBuffer::new();
        value.push_dec(timer::ticks() / 100);
        value.push_str(" s");
        self.monitor_row(frame, x, y + 40, max_x, "Uptime", value.as_str());

        let mut value = TextBuffer::new();
        value.push_dec(timer::ticks());
        self.monitor_row(frame, x, y + 68, max_x, "Timer ticks", value.as_str());

        let mut value = TextBuffer::new();
        value.push_dec(usable / (1024 * 1024));
        value.push_str(" MiB / ");
        value.push_dec(regions);
        value.push_str(" regions");
        self.monitor_row(frame, x, y + 96, max_x, "Usable memory", value.as_str());

        let mut value = TextBuffer::new();
        value.push_dec(heap_kib);
        value.push_str(" KiB");
        self.monitor_row(frame, x, y + 124, max_x, "Compositor heap", value.as_str());

        let mut value = TextBuffer::new();
        value.push_dec(self.width as u64);
        value.push(b'x');
        value.push_dec(self.height as u64);
        value.push_str("x32");
        self.monitor_row(frame, x, y + 152, max_x, "Display", value.as_str());

        let boot = if limine::is_active() { "Limine native" } else { "PVH direct" };
        self.monitor_row(frame, x, y + 180, max_x, "Boot path", boot);

        let mut value = TextBuffer::new();
        if mouse::present() {
            value.push_str(if mouse::usb_present() { "USB HID @ " } else { "PS/2 @ " });
            value.push_signed(mouse_x);
            value.push(b',');
            value.push_signed(mouse_y);
        } else {
            value.push_str("not detected");
        }
        self.monitor_row(frame, x, y + 208, max_x, "Mouse", value.as_str());

        // Heap bar: 16 MiB total.
        let bar_y = y + 246;
        fill(frame, stride, height, x, bar_y, 360, 10, 0x002A_2A2A);
        let filled = (crate::allocator::used() * 360 / (16 * 1024 * 1024)).min(360);
        fill(frame, stride, height, x, bar_y, filled, 10, self.accent);
        draw_text(
            frame,
            stride,
            height,
            x,
            bar_y + 18,
            max_x,
            "Heap usage (16 MiB pool)",
            DIM,
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
        draw_text(frame, self.width, self.height, x + 260, y, max_x, value, FG);
    }

    fn draw_settings(&self, frame: &mut [u32], window: &Window, mouse_x: i32, mouse_y: i32) {
        let stride = self.width;
        let height = self.height;
        let x = window.x + 20;
        let y = window.y + TITLEBAR_H as i32 + 18;
        let max_x = window.x + window.w as i32 - 20;

        draw_text(frame, stride, height, x, y, max_x, "Appearance", self.accent);
        self.monitor_row(frame, x, y + 40, max_x, "Theme", "Fluent Dark");
        self.monitor_row(frame, x, y + 68, max_x, "Accent", "");
        let mut accent = TextBuffer::new();
        accent.push_hex_rgb(self.accent);
        draw_text(
            frame,
            stride,
            height,
            x + 260,
            y + 68,
            max_x,
            accent.as_str(),
            FG,
        );

        let swatches = settings_swatches(window);
        for (index, swatch) in swatches.iter().enumerate() {
            let selected = ACCENTS[index] == self.accent;
            let border = if selected { FG } else { 0x004A_4A4A };
            rounded_rect(
                frame,
                stride,
                height,
                swatch.x - 2,
                swatch.y - 2,
                swatch.w + 4,
                swatch.h + 4,
                5,
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

        draw_text(frame, stride, height, x, y + 140, max_x, "Display", self.accent);
        let mut resolution = TextBuffer::new();
        resolution.push_dec(self.width as u64);
        resolution.push(b'x');
        resolution.push_dec(self.height as u64);
        resolution.push_str("x32");
        self.monitor_row(frame, x, y + 176, max_x, "Resolution", resolution.as_str());
        let orientation = match fb::orientation() {
            (false, false) => "Normal",
            (true, false) => "Mirror X",
            (true, true) => "Rotated 180",
            (false, true) => "Flip Y",
        };
        self.monitor_row(frame, x, y + 204, max_x, "Orientation", orientation);

        draw_text(frame, stride, height, x, y + 248, max_x, "Input", self.accent);
        self.monitor_row(
            frame,
            x,
            y + 284,
            max_x,
            "Mouse",
            if mouse::present() { mouse::source() } else { "not detected" },
        );
        self.monitor_row(frame, x, y + 312, max_x, "Keyboard", "PS/2 scancode set 1");
    }

    fn draw_about(&self, frame: &mut [u32], window: &Window) {
        let stride = self.width;
        let height = self.height;
        let center = window.x + window.w as i32 / 2;
        let x = window.x + 24;
        let y = window.y + TITLEBAR_H as i32 + 22;
        let max_x = window.x + window.w as i32 - 24;

        draw_text_centered(frame, stride, height, center, y, "ZENITH OS", self.accent);
        draw_text_centered(frame, stride, height, center, y + 30, "Version 0.1.0", FG);
        draw_text_centered(
            frame,
            stride,
            height,
            center,
            y + 58,
            "Independent x86_64 OS",
            MUTED,
        );
        draw_text_centered(
            frame,
            stride,
            height,
            center,
            y + 86,
            "Rust + assembly",
            MUTED,
        );

        let mut memory = TextBuffer::new();
        let (_, usable) = limine::memory_summary();
        memory.push_dec(usable / (1024 * 1024));
        memory.push_str(" MiB usable");

        let rows = [
            ("Boot", if limine::is_active() { "Limine native" } else { "PVH direct" }),
            ("Display", ""),
            ("Memory", memory.as_str()),
            ("License", "MIT"),
        ];
        for (index, (label, value)) in rows.iter().enumerate() {
            let row_y = y + 126 + index as i32 * 28;
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
        let ty = height.saturating_sub(TASKBAR_H) as i32;

        fill(frame, stride, height, 0, ty, stride, TASKBAR_H, TASKBAR_BG);
        fill(frame, stride, height, 0, ty, stride, 1, TASKBAR_LINE);

        let layout = TaskbarLayout::new(self.width, self.height);

        // Start button.
        if layout.start.contains(mouse_x, mouse_y) || start_menu::is_open() {
            rounded_rect(
                frame,
                stride,
                height,
                layout.start.x + 4,
                layout.start.y + 4,
                40,
                40,
                7,
                TASKBAR_HOVER,
            );
        }
        let icon_x = layout.start.x + 14;
        let icon_y = layout.start.y + 14;
        for (dx, dy) in [(0i32, 0i32), (11, 0), (0, 11), (11, 11)] {
            fill(
                frame,
                stride,
                height,
                icon_x + dx,
                icon_y + dy,
                8,
                8,
                self.accent,
            );
        }

        // Pinned applications.
        for (index, kind) in TASKBAR_KINDS.iter().enumerate() {
            let rect = layout.apps[index];
            let hovered = rect.contains(mouse_x, mouse_y);
            if hovered {
                rounded_rect(
                    frame,
                    stride,
                    height,
                    rect.x + 4,
                    rect.y + 4,
                    40,
                    40,
                    7,
                    TASKBAR_HOVER,
                );
            }
            rounded_rect(
                frame,
                stride,
                height,
                rect.x + 9,
                rect.y + 8,
                30,
                30,
                7,
                kind.color(),
            );
            draw_glyph(
                frame,
                stride,
                height,
                rect.x + 16,
                rect.y + 15,
                kind.letter(),
                0x00FF_FFFF,
            );

            let window = self.find(*kind).and_then(|index| self.windows[index]);
            if let Some(window) = window {
                let focused = self.top_visible() == self.find(*kind);
                let indicator = if focused { self.accent } else { 0x0055_6677 };
                fill(
                    frame,
                    stride,
                    height,
                    rect.x + 16,
                    rect.y + 43,
                    16,
                    2,
                    indicator,
                );
                let _ = window;
            }
        }

        // System tray: network link indicator, then the real-time clock.
        let link_up = e1000::link_up();
        let net_x = self.width as i32 - 150;
        let net_y = ty + 20;
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
            self.width as i32 - 112,
            ty + 16,
            self.width as i32 - 12,
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

        draw_text(
            frame,
            stride,
            height,
            panel.x + 24,
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
            "Zenith 0.1",
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
                rect.x + 21,
                rect.y + 24,
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
    Desktop,
}

struct TaskbarLayout {
    ty: i32,
    start: Rect,
    apps: [Rect; 4],
}

impl TaskbarLayout {
    fn new(width: usize, height: usize) -> Self {
        let ty = height.saturating_sub(TASKBAR_H) as i32;
        let group_width = 5 * 48;
        let group_x = width.saturating_sub(group_width) as i32 / 2;
        TaskbarLayout {
            ty,
            start: Rect::new(group_x, ty, 48, 48),
            apps: [
                Rect::new(group_x + 48, ty, 48, 48),
                Rect::new(group_x + 96, ty, 48, 48),
                Rect::new(group_x + 144, ty, 48, 48),
                Rect::new(group_x + 192, ty, 48, 48),
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
    let start_x = window.x + 20;
    let start_y = window.y + TITLEBAR_H as i32 + 18 + 96;
    let mut swatches = [Rect::new(0, 0, 0, 0); 4];
    for (index, swatch) in swatches.iter_mut().enumerate() {
        *swatch = Rect::new(start_x + index as i32 * 44, start_y, 32, 22);
    }
    swatches
}

fn settings_swatch_at(window: &Window, x: i32, y: i32) -> Option<usize> {
    settings_swatches(window)
        .iter()
        .position(|swatch| swatch.contains(x, y))
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

fn build_wallpaper(width: usize, height: usize) -> Vec<u32> {
    let mut buffer = vec![0u32; width.saturating_mul(height)];
    for y in 0..height {
        let color = mix(WALL_TOP, WALL_BOTTOM, y, height.max(1));
        let row = &mut buffer[y * width..(y + 1) * width];
        for pixel in row.iter_mut() {
            *pixel = color;
        }
    }

    // Soft blue and violet blooms, in the spirit of the Windows 11 wallpaper.
    glow(
        &mut buffer,
        width,
        height,
        (width as i64 * 32) / 100,
        (height as i64 * 30) / 100,
        (height as i64 * 55) / 100,
        0x003A_6EA8,
        34,
    );
    glow(
        &mut buffer,
        width,
        height,
        (width as i64 * 76) / 100,
        (height as i64 * 72) / 100,
        (height as i64 * 38) / 100,
        0x005A_3A88,
        22,
    );
    buffer
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
