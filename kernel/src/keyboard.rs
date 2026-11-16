//! PS/2 keyboard driver (scancode set 1) using IRQ1.

use crate::io::inb;
use crate::sync::SpinLock;

const DATA_PORT: u16 = 0x60;

struct Buffer {
    data: [u8; 256],
    head: usize,
    tail: usize,
}

static BUFFER: SpinLock<Buffer> = SpinLock::new(Buffer {
    data: [0; 256],
    head: 0,
    tail: 0,
});

/// Translate a scancode set 1 make-code to ASCII (0 = no printable mapping).
fn scancode_to_ascii(code: u8) -> u8 {
    match code {
        0x02 => b'1',
        0x03 => b'2',
        0x04 => b'3',
        0x05 => b'4',
        0x06 => b'5',
        0x07 => b'6',
        0x08 => b'7',
        0x09 => b'8',
        0x0A => b'9',
        0x0B => b'0',
        0x0C => b'-',
        0x0D => b'=',
        0x0E => 0x08, // backspace
        0x0F => b'\t',
        0x10 => b'q',
        0x11 => b'w',
        0x12 => b'e',
        0x13 => b'r',
        0x14 => b't',
        0x15 => b'y',
        0x16 => b'u',
        0x17 => b'i',
        0x18 => b'o',
        0x19 => b'p',
        0x1A => b'[',
        0x1B => b']',
        0x1C => b'\n', // enter
        0x1E => b'a',
        0x1F => b's',
        0x20 => b'd',
        0x21 => b'f',
        0x22 => b'g',
        0x23 => b'h',
        0x24 => b'j',
        0x25 => b'k',
        0x26 => b'l',
        0x27 => b';',
        0x28 => b'\'',
        0x29 => b'`',
        0x2B => b'\\',
        0x2C => b'z',
        0x2D => b'x',
        0x2E => b'c',
        0x2F => b'v',
        0x30 => b'b',
        0x31 => b'n',
        0x32 => b'm',
        0x33 => b',',
        0x34 => b'.',
        0x35 => b'/',
        0x39 => b' ',
        _ => 0,
    }
}

/// IRQ1 handler: read the scancode and buffer printable characters.
pub fn irq() {
    let code = unsafe { inb(DATA_PORT) };
    if code & 0x80 != 0 {
        return; // key release
    }
    let ascii = scancode_to_ascii(code);
    if ascii != 0 {
        let mut buffer = BUFFER.lock();
        let head = buffer.head;
        let tail = buffer.tail;
        let next = (tail + 1) % buffer.data.len();
        if next != head {
            buffer.data[tail] = ascii;
            buffer.tail = next;
        }
    }
}

/// Return the next buffered character, if any.
pub fn read_char() -> Option<u8> {
    let mut buffer = BUFFER.lock();
    if buffer.head == buffer.tail {
        return None;
    }
    let value = buffer.data[buffer.head];
    buffer.head = (buffer.head + 1) % buffer.data.len();
    Some(value)
}
