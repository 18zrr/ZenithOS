//! PS/2 mouse driver using IRQ12 and the 8042 controller.
//!
//! The controller is programmed for a standard three-byte packet stream at the
//! default 100 Hz report rate. Button edges and motion are queued for the
//! window manager, which drains the queue from the shell task.

use core::sync::atomic::{AtomicBool, Ordering};

use crate::io::{inb, outb};
use crate::sync::SpinLock;

const DATA_PORT: u16 = 0x60;
const STATUS_PORT: u16 = 0x64;

const STATUS_OUTPUT_FULL: u8 = 0x01;
const STATUS_INPUT_FULL: u8 = 0x02;
const STATUS_AUX_DATA: u8 = 0x20;

const CMD_DISABLE_KEYBOARD: u8 = 0xAD;
const CMD_ENABLE_KEYBOARD: u8 = 0xAE;
const CMD_DISABLE_AUX: u8 = 0xA7;
const CMD_ENABLE_AUX: u8 = 0xA8;
const CMD_READ_CONFIG: u8 = 0x20;
const CMD_WRITE_CONFIG: u8 = 0x60;
const CMD_WRITE_AUX: u8 = 0xD4;

const MOUSE_SET_DEFAULTS: u8 = 0xF6;
const MOUSE_ENABLE_REPORTING: u8 = 0xF4;

pub const EVENT_MOVE: u8 = 0;
pub const EVENT_BUTTON_DOWN: u8 = 1;
pub const EVENT_BUTTON_UP: u8 = 2;

pub const BUTTON_LEFT: u8 = 1;
pub const BUTTON_RIGHT: u8 = 2;
pub const BUTTON_MIDDLE: u8 = 4;

const QUEUE_CAPACITY: usize = 128;
const DEFAULT_WIDTH: i32 = 1024;
const DEFAULT_HEIGHT: i32 = 768;

#[derive(Clone, Copy)]
pub struct MouseEvent {
    pub kind: u8,
    pub button: u8,
    pub x: i32,
    pub y: i32,
}

struct Mouse {
    present: bool,
    usb: bool,
    packet: [u8; 3],
    packet_index: usize,
    x: i32,
    y: i32,
    width: i32,
    height: i32,
    buttons: u8,
    events: [MouseEvent; QUEUE_CAPACITY],
    head: usize,
    tail: usize,
}

impl Mouse {
    const fn new() -> Self {
        Mouse {
            present: false,
            usb: false,
            packet: [0; 3],
            packet_index: 0,
            x: DEFAULT_WIDTH / 2,
            y: DEFAULT_HEIGHT / 2,
            width: DEFAULT_WIDTH,
            height: DEFAULT_HEIGHT,
            buttons: 0,
            events: [MouseEvent {
                kind: EVENT_MOVE,
                button: 0,
                x: 0,
                y: 0,
            }; QUEUE_CAPACITY],
            head: 0,
            tail: 0,
        }
    }

    fn push(&mut self, kind: u8, button: u8) {
        let next = (self.tail + 1) % QUEUE_CAPACITY;
        if next == self.head {
            // Drop the oldest event rather than the newest: input latency
            // matters more than perfect history.
            self.head = (self.head + 1) % QUEUE_CAPACITY;
        }
        self.events[self.tail] = MouseEvent {
            kind,
            button,
            x: self.x,
            y: self.y,
        };
        self.tail = next;
    }

    fn clamp(&mut self) {
        self.x = self.x.clamp(0, (self.width - 1).max(0));
        self.y = self.y.clamp(0, (self.height - 1).max(0));
    }

    fn button_mask(&self, flags: u8) -> u8 {
        let mut mask = 0;
        if flags & 0x01 != 0 {
            mask |= BUTTON_LEFT;
        }
        if flags & 0x02 != 0 {
            mask |= BUTTON_RIGHT;
        }
        if flags & 0x04 != 0 {
            mask |= BUTTON_MIDDLE;
        }
        mask
    }

    fn feed(&mut self, byte: u8) {
        // The first packet byte always has bit 3 set.
        if self.packet_index == 0 && byte & 0x08 == 0 {
            return;
        }
        self.packet[self.packet_index] = byte;
        self.packet_index += 1;
        if self.packet_index < 3 {
            return;
        }
        self.packet_index = 0;

        let flags = self.packet[0];
        if flags & 0xC0 != 0 {
            return; // X/Y overflow; discard the packet
        }

        let dx = self.packet[1] as i32 - if flags & 0x10 != 0 { 256 } else { 0 };
        let dy = self.packet[2] as i32 - if flags & 0x20 != 0 { 256 } else { 0 };
        // PS/2 reports positive Y upwards; the screen grows downwards.
        self.apply(self.button_mask(flags), dx, -dy);
    }

    /// Apply a decoded report. USB HID and PS/2 share the button-bit layout,
    /// but USB reports positive Y downwards.
    fn apply(&mut self, buttons: u8, dx: i32, dy: i32) {
        self.x += dx;
        self.y += dy;
        self.clamp();

        let pressed = buttons & !self.buttons;
        let released = self.buttons & !buttons;
        self.buttons = buttons;

        if dx != 0 || dy != 0 || pressed != 0 || released != 0 {
            self.push(EVENT_MOVE, 0);
        }
        for button in [BUTTON_LEFT, BUTTON_RIGHT, BUTTON_MIDDLE] {
            if pressed & button != 0 {
                self.push(EVENT_BUTTON_DOWN, button);
            }
            if released & button != 0 {
                self.push(EVENT_BUTTON_UP, button);
            }
        }
    }

    /// Decode a USB HID boot-mouse report (buttons, dx, dy, optional wheel).
    fn usb_report(&mut self, report: &[u8], length: usize) {
        if length < 3 || report.len() < 3 {
            return;
        }
        let buttons = self.button_mask(report[0]);
        let dx = report[1] as i8 as i32;
        let dy = report[2] as i8 as i32;
        self.apply(buttons, dx, dy);
    }
}

static MOUSE: SpinLock<Mouse> = SpinLock::new(Mouse::new());
static INITIALIZED: AtomicBool = AtomicBool::new(false);

fn wait_write() -> bool {
    for _ in 0..100_000 {
        if unsafe { inb(STATUS_PORT) } & STATUS_INPUT_FULL == 0 {
            return true;
        }
    }
    false
}

fn wait_read() -> bool {
    for _ in 0..100_000 {
        if unsafe { inb(STATUS_PORT) } & STATUS_OUTPUT_FULL != 0 {
            return true;
        }
    }
    false
}

fn write_command(command: u8) -> bool {
    if !wait_write() {
        return false;
    }
    unsafe { outb(STATUS_PORT, command) };
    true
}

fn write_data(data: u8) -> bool {
    if !wait_write() {
        return false;
    }
    unsafe { outb(DATA_PORT, data) };
    true
}

fn read_data() -> Option<u8> {
    if !wait_read() {
        return None;
    }
    Some(unsafe { inb(DATA_PORT) })
}

fn flush_output() {
    for _ in 0..32 {
        if unsafe { inb(STATUS_PORT) } & STATUS_OUTPUT_FULL == 0 {
            break;
        }
        unsafe { inb(DATA_PORT) };
    }
}

fn write_aux(data: u8) -> bool {
    write_command(CMD_WRITE_AUX) && write_data(data)
}

fn read_ack() -> bool {
    for _ in 0..8 {
        match read_data() {
            Some(0xFA) => return true,
            Some(0xFE) => return false,
            Some(_) => continue,
            None => return false,
        }
    }
    false
}

fn io_delay() {
    // Port 0x80 is a convenient short delay on x86 chipsets.
    unsafe { outb(0x80, 0) };
}

/// Initialise the controller and enable mouse reporting. Must run with
/// interrupts disabled, after the PIC has been remapped.
pub fn init() -> bool {
    if INITIALIZED.load(Ordering::Acquire) {
        return MOUSE.lock().present;
    }

    write_command(CMD_DISABLE_KEYBOARD);
    write_command(CMD_DISABLE_AUX);
    flush_output();

    if !write_command(CMD_READ_CONFIG) {
        return false;
    }
    let mut config = match read_data() {
        Some(value) => value,
        None => return false,
    };
    // Enable IRQ1 and IRQ12, and clear both clock-disable bits.
    config |= 0x03;
    config &= !0x30;
    write_command(CMD_WRITE_CONFIG);
    write_data(config);
    io_delay();

    write_command(CMD_ENABLE_AUX);
    io_delay();

    // Defaults, then continuous reporting.
    let present = write_aux(MOUSE_SET_DEFAULTS) && read_ack();
    let reporting = write_aux(MOUSE_ENABLE_REPORTING) && read_ack();

    write_command(CMD_ENABLE_KEYBOARD);
    flush_output();

    let mut mouse = MOUSE.lock();
    mouse.present = present && reporting;
    mouse.packet_index = 0;
    mouse.buttons = 0;
    INITIALIZED.store(true, Ordering::Release);
    mouse.present
}

/// Center the cursor and constrain it to the display.
pub fn set_bounds(width: usize, height: usize) {
    crate::interrupt::without_interrupts(|| {
        let mut mouse = MOUSE.lock();
        let width = width.max(1) as i32;
        let height = height.max(1) as i32;
        if mouse.width != width || mouse.height != height {
            mouse.x = mouse.x.clamp(0, width - 1);
            mouse.y = mouse.y.clamp(0, height - 1);
            mouse.width = width;
            mouse.height = height;
        }
    });
}

/// Current cursor position in screen pixels.
pub fn position() -> (i32, i32) {
    crate::interrupt::without_interrupts(|| {
        let mouse = MOUSE.lock();
        (mouse.x, mouse.y)
    })
}

/// Current button mask (1 left, 2 right, 4 middle).
pub fn buttons() -> u8 {
    crate::interrupt::without_interrupts(|| MOUSE.lock().buttons)
}

/// Whether any pointing device (PS/2 or USB) was detected.
pub fn present() -> bool {
    crate::interrupt::without_interrupts(|| {
        let mouse = MOUSE.lock();
        mouse.present || mouse.usb
    })
}

/// Mark a USB HID mouse as ready. Its reports share the window-manager queue.
pub fn usb_ready() {
    crate::interrupt::without_interrupts(|| {
        MOUSE.lock().usb = true;
    });
}

/// Whether a USB HID mouse is active.
pub fn usb_present() -> bool {
    crate::interrupt::without_interrupts(|| MOUSE.lock().usb)
}

/// Human-readable pointing-device source for the System Monitor window.
pub fn source() -> &'static str {
    crate::interrupt::without_interrupts(|| {
        let mouse = MOUSE.lock();
        match (mouse.usb, mouse.present) {
            (true, true) => "USB HID + PS/2",
            (true, false) => "USB HID, 3-button",
            (false, true) => "PS/2, 3-button",
            (false, false) => "not detected",
        }
    })
}

/// Feed a USB HID boot-mouse report into the shared input queue.
pub fn usb_report(report: &[u8], length: usize) {
    crate::interrupt::without_interrupts(|| {
        MOUSE.lock().usb_report(report, length);
    });
}

/// Remove the next queued event.
pub fn take_event() -> Option<MouseEvent> {
    crate::interrupt::without_interrupts(|| {
        let mut mouse = MOUSE.lock();
        if mouse.head == mouse.tail {
            return None;
        }
        let event = mouse.events[mouse.head];
        mouse.head = (mouse.head + 1) % QUEUE_CAPACITY;
        Some(event)
    })
}

/// Discard queued input (used while a modal surface owns the screen).
pub fn flush_events() {
    crate::interrupt::without_interrupts(|| {
        let mut mouse = MOUSE.lock();
        mouse.head = mouse.tail;
    });
}

/// IRQ12 handler: drain one auxiliary byte and assemble a packet.
pub fn irq() {
    let status = unsafe { inb(STATUS_PORT) };
    if status & STATUS_OUTPUT_FULL == 0 {
        return;
    }
    let byte = unsafe { inb(DATA_PORT) };
    if status & STATUS_AUX_DATA == 0 {
        // A keyboard byte arrived on the mouse line; hand it back to the
        // keyboard queue so no keystroke is lost.
        crate::keyboard::inject(byte);
        return;
    }
    MOUSE.lock().feed(byte);
}
