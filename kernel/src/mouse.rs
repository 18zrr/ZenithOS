//! PS/2 mouse (IRQ12) and shared USB HID pointer state.
//!
//! The 8042 controller is programmed for a three-byte stream and upgraded to
//! four-byte IntelliMouse packets when the device supports a wheel or the two
//! extra side buttons. USB reports are fed in by the UHCI driver. Both paths
//! push into one queue that the window manager drains from the shell task.

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
const MOUSE_SET_SAMPLE: u8 = 0xF3;
const MOUSE_GET_ID: u8 = 0xF1;

pub const EVENT_MOVE: u8 = 0;
pub const EVENT_BUTTON_DOWN: u8 = 1;
pub const EVENT_BUTTON_UP: u8 = 2;
pub const EVENT_WHEEL: u8 = 3;

pub const BUTTON_LEFT: u8 = 1;
pub const BUTTON_RIGHT: u8 = 2;
pub const BUTTON_MIDDLE: u8 = 4;
pub const BUTTON_BACK: u8 = 8;
pub const BUTTON_FORWARD: u8 = 16;

const QUEUE_CAPACITY: usize = 128;
const DEFAULT_WIDTH: i32 = 1024;
const DEFAULT_HEIGHT: i32 = 768;

#[derive(Clone, Copy)]
pub struct MouseEvent {
    pub kind: u8,
    pub button: u8,
    pub x: i32,
    pub y: i32,
    pub scroll: i32,
}

struct Mouse {
    present: bool,
    usb: bool,
    packet: [u8; 4],
    packet_index: usize,
    wheel: bool,
    explorer: bool,
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
            packet: [0; 4],
            packet_index: 0,
            wheel: false,
            explorer: false,
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
                scroll: 0,
            }; QUEUE_CAPACITY],
            head: 0,
            tail: 0,
        }
    }

    fn push(&mut self, kind: u8, button: u8, scroll: i32) {
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
            scroll,
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
        if flags & 0x08 != 0 {
            mask |= BUTTON_BACK;
        }
        if flags & 0x10 != 0 {
            mask |= BUTTON_FORWARD;
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
        let packet_len = if self.wheel { 4 } else { 3 };
        if self.packet_index < packet_len {
            return;
        }
        self.packet_index = 0;

        let flags = self.packet[0];
        if flags & 0xC0 != 0 {
            return; // X/Y overflow; discard the packet
        }

        let dx = self.packet[1] as i32 - if flags & 0x10 != 0 { 256 } else { 0 };
        let dy = self.packet[2] as i32 - if flags & 0x20 != 0 { 256 } else { 0 };
        let mut buttons = self.button_mask(flags);
        let mut scroll = 0i32;
        if self.wheel {
            let extra = self.packet[3];
            if self.explorer {
                buttons |= self.button_mask((extra >> 1) & 0x18);
                let nibble = extra & 0x0F;
                scroll = if nibble & 0x08 != 0 {
                    (nibble as i32) - 16
                } else {
                    nibble as i32
                };
            } else {
                scroll = extra as i8 as i32;
            }
        }
        // PS/2 reports positive Y upwards; the screen grows downwards.
        self.apply(buttons, dx, -dy, scroll);
    }

    /// Apply a decoded report. USB HID and PS/2 share the button-bit layout,
    /// but USB reports positive Y downwards.
    fn apply(&mut self, buttons: u8, dx: i32, dy: i32, wheel: i32) {
        self.x += dx;
        self.y += dy;
        self.clamp();

        let pressed = buttons & !self.buttons;
        let released = self.buttons & !buttons;
        self.buttons = buttons;

        if dx != 0 || dy != 0 || pressed != 0 || released != 0 {
            self.push(EVENT_MOVE, 0, 0);
        }
        for button in [
            BUTTON_LEFT,
            BUTTON_RIGHT,
            BUTTON_MIDDLE,
            BUTTON_BACK,
            BUTTON_FORWARD,
        ] {
            if pressed & button != 0 {
                self.push(EVENT_BUTTON_DOWN, button, 0);
            }
            if released & button != 0 {
                self.push(EVENT_BUTTON_UP, button, 0);
            }
        }
        if wheel != 0 {
            self.push(EVENT_WHEEL, 0, wheel);
        }
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

/// Detect the extended packet format. Returns `(wheel, explorer_buttons)`.
fn detect_extensions() -> (bool, bool) {
    // Magic sample-rate sequence selects the IntelliMouse protocol family.
    for rate in [200u8, 100, 80] {
        if !(write_aux(MOUSE_SET_SAMPLE) && write_data(rate) && read_ack()) {
            return (false, false);
        }
    }
    if !(write_aux(MOUSE_GET_ID) && read_ack()) {
        return (false, false);
    }
    let id = match read_data() {
        Some(value) => value,
        None => return (false, false),
    };
    match id {
        3 => (true, false),
        // IntelliMouse Explorer: five buttons plus a four-bit wheel.
        4 => (true, true),
        _ => {
            // Put the device back into a clean three-byte stream.
            let _ = write_aux(MOUSE_SET_SAMPLE) && write_data(100) && read_ack();
            (false, false)
        }
    }
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

    // Defaults, then try the IntelliMouse sample-rate handshake.
    let present = write_aux(MOUSE_SET_DEFAULTS) && read_ack();
    let (wheel, explorer) = if present {
        detect_extensions()
    } else {
        (false, false)
    };
    let reporting = write_aux(MOUSE_ENABLE_REPORTING) && read_ack();

    write_command(CMD_ENABLE_KEYBOARD);
    flush_output();

    let mut mouse = MOUSE.lock();
    mouse.present = present && reporting;
    mouse.wheel = wheel;
    mouse.explorer = explorer;
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

/// Current button mask (left 1, right 2, middle 4, back 8, forward 16).
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

const BUTTON_NAMES: [&str; 5] = ["left", "right", "middle", "back", "forward"];

fn describe(mouse: &Mouse) -> &'static str {
    match (mouse.usb, mouse.present, mouse.wheel) {
        (true, true, _) => "USB HID + PS/2, 5-button, wheel",
        (true, false, _) => "USB HID, 5-button, wheel",
        (false, true, true) if mouse.explorer => "PS/2 IntelliMouse, 5-button, wheel",
        (false, true, true) => "PS/2 IntelliMouse, wheel",
        (false, true, false) => "PS/2, 3-button",
        (false, false, _) => "not detected",
    }
}

/// Human-readable pointing-device source for the monitor and settings windows.
pub fn source() -> &'static str {
    crate::interrupt::without_interrupts(|| describe(&MOUSE.lock()))
}

/// Feed a HID boot-mouse report (`buttons`, dx, dy, optional wheel).
pub fn usb_report(report: &[u8], length: usize) {
    if length < 3 || report.len() < 3 {
        return;
    }
    let buttons = report[0] & 0x1F;
    let wheel = if length >= 4 { report[3] as i8 as i32 } else { 0 };
    usb_input(buttons, report[1] as i8 as i32, report[2] as i8 as i32, wheel);
}

/// Feed an already-decoded HID report. `buttons` uses the same bit order as
/// `BUTTON_LEFT` through `BUTTON_FORWARD`; `dx`/`dy` are screen deltas.
pub fn usb_input(buttons: u8, dx: i32, dy: i32, wheel: i32) {
    crate::interrupt::without_interrupts(|| {
        let mut mouse = MOUSE.lock();
        mouse.usb = true;
        mouse.apply(buttons & 0x1F, dx, dy, wheel);
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

#[allow(dead_code)]
fn button_name(button: u8) -> &'static str {
    for (index, name) in BUTTON_NAMES.iter().enumerate() {
        if button == 1 << index {
            return name;
        }
    }
    "unknown"
}
