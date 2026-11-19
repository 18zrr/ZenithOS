//! Display and input for the optional QEMU Chromium companion.
//! The 8 MiB ivshmem BAR is shared with tools/run_browser.py, not guest RAM.
use crate::{keyboard::{Key, KeyEvent}, pci};
use core::ptr::{read_volatile, write_volatile};
use core::sync::atomic::{fence, AtomicUsize, Ordering};

pub const WIDTH: usize = 960;
pub const HEIGHT: usize = 600;
const MAGIC: u32 = u32::from_le_bytes(*b"ZWB1");
static BASE: AtomicUsize = AtomicUsize::new(0);

// All offsets here are in u32 words. Producer publishes pixels before seq,
// consumer acknowledges after drawing. Only the inactive buffer is writable.
const SEQ: usize = 1;
const ACK: usize = 2;
const HEAD: usize = 3;
const TAIL: usize = 4;
const EVENTS: usize = 16;
const PIXELS: usize = 1024;

pub fn init() {
    for slot in 0..32 {
        if pci::read(0, slot, 0, 0) != 0x1110_1af4 { continue; }
        let bar = pci::read(0, slot, 0, 0x18);
        // SeaBIOS maps BAR2 in the low 4 GiB identity map.
        if bar & 7 != 4 || pci::read(0, slot, 0, 0x1c) != 0 { continue; }
        let base = (bar & !15) as usize;
        if base == 0 || base % (8 << 20) != 0 { continue; }
        let command = pci::read16(0, slot, 0, 4);
        pci::write16(0, slot, 0, 4, command | 2);
        if unsafe { read_volatile(base as *const u32) } == MAGIC {
            BASE.store(base, Ordering::Release);
            crate::diagnostics::debug("[browser] Chromium companion connected");
            return;
        }
    }
}

fn base() -> Option<*mut u32> {
    let ptr = BASE.load(Ordering::Acquire) as *mut u32;
    if ptr.is_null() || unsafe { read_volatile(ptr) } != MAGIC { None } else { Some(ptr) }
}

pub fn changed() -> bool {
    base().is_some_and(|p| unsafe { read_volatile(p.add(SEQ)) != read_volatile(p.add(ACK)) })
}

pub fn draw(frame: &mut [u32], stride: usize, height: usize, x: i32, y: i32,
            w: usize, h: usize) -> bool {
    let Some(p) = base() else { return false };
    unsafe {
        let seq = read_volatile(p.add(SEQ));
        fence(Ordering::Acquire);
        let pixels = p.add(PIXELS + (seq as usize & 1) * WIDTH * HEIGHT);
        let left = x.max(0) as usize;
        let right = (x + w as i32).clamp(0, stride as i32) as usize;
        for dy in y.max(0)..(y + h as i32).min(height as i32) {
            let row = pixels.add((dy - y) as usize * HEIGHT / h * WIDTH);
            for dx in left..right {
                frame[dy as usize * stride + dx] =
                    read_volatile(row.add((dx as i32 - x) as usize * WIDTH / w));
            }
        }
        fence(Ordering::Release);
        write_volatile(p.add(ACK), seq);
    }
    true
}

fn send(event: u32) {
    let Some(p) = base() else { return };
    unsafe {
        let head = read_volatile(p.add(HEAD)) & 255;
        let next = (head + 1) & 255;
        if next == read_volatile(p.add(TAIL)) { return; }
        write_volatile(p.add(EVENTS + head as usize), event);
        fence(Ordering::Release);
        write_volatile(p.add(HEAD), next);
    }
}

pub fn key(event: KeyEvent) {
    // X11 keysyms; printable ASCII has identical values.
    let sym = match event.key {
        Key::Char(byte) => byte as u32, Key::Backspace => 0xff08,
        Key::Tab => 0xff09, Key::Enter => 0xff0d, Key::Escape => 0xff1b,
        Key::Home => 0xff50, Key::Left => 0xff51, Key::Up => 0xff52,
        Key::Right => 0xff53, Key::Down => 0xff54, Key::PageUp => 0xff55,
        Key::PageDown => 0xff56, Key::End => 0xff57, Key::Insert => 0xff63,
        Key::Delete => 0xffff, Key::F(n) => 0xffbd + n as u32,
    };
    send(0x1000_0000 | sym | ((event.shift as u32) << 16)
        | ((event.ctrl as u32) << 17) | ((event.alt as u32) << 18));
}

// Motion/button events carry absolute browser coordinates, so a dropped
// motion event never accumulates positional drift.
pub fn pointer(kind: u32, button: u32, x: usize, y: usize) {
    send((kind << 28) | (button << 20) | ((y as u32) << 10) | x as u32);
}
