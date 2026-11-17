//! Kernel diagnostics and logging.
//!
//! Output goes to both COM1 serial (forwarded by QEMU) and the VGA text
//! console, giving a visible screen even before a framebuffer driver exists.

use core::arch::asm;
use crate::io::outb;
use crate::vga;

const COM1: u16 = 0x3F8;

/// Initialise COM1 to 8N1 with interrupts disabled, and clear the VGA screen.
pub fn init() {
    unsafe {
        outb(COM1 + 1, 0x00);
        outb(COM1 + 3, 0x80);
        outb(COM1 + 0, 0x03);
        outb(COM1 + 1, 0x00);
        outb(COM1 + 3, 0x03);
        outb(COM1 + 2, 0x00);
        outb(COM1 + 4, 0x00);
    }
    vga::clear();
}

/// Write a string to COM1 only (not the on-screen consoles).
pub fn serial_puts(message: &str) {
    for &byte in message.as_bytes() {
        if byte == b'\n' {
            putc(b'\r');
        }
        putc(byte);
    }
}

/// Print a u64 in decimal to COM1 only.
pub fn serial_put_dec(mut value: u64) {
    if value == 0 {
        putc(b'0');
        return;
    }
    let mut buffer = [0u8; 20];
    let mut start = buffer.len();
    while value > 0 {
        start -= 1;
        buffer[start] = b'0' + (value % 10) as u8;
        value /= 10;
    }
    for &byte in &buffer[start..] {
        putc(byte);
    }
}

/// Print a u64 as hexadecimal to COM1 only.
pub fn serial_put_hex(value: u64) {
    const DIGITS: &[u8; 16] = b"0123456789abcdef";
    let mut buffer = [0u8; 16];
    for i in 0..16 {
        buffer[i] = DIGITS[((value >> ((15 - i) * 4)) & 0xF) as usize];
    }
    let mut start = 0;
    while start < 15 && buffer[start] == b'0' {
        start += 1;
    }
    serial_puts("0x");
    for &byte in &buffer[start..] {
        putc(byte);
    }
}

/// Clear the visible consoles and repaint the desktop chrome.
pub fn clear_screen() {
    vga::clear();
    crate::framebuffer::clear_screen();
}

/// Write one byte to COM1, waiting for room in the transmit FIFO.
pub fn putc(byte: u8) {
    unsafe {
        loop {
            let status: u8;
            asm!("in al, dx", in("dx") COM1 + 5, out("al") status, options(nostack, preserves_flags));
            if status & 0x20 != 0 {
                break;
            }
        }
        outb(COM1, byte);
    }
}

/// Write a string, translating `\n` to CRLF for serial terminals.
pub fn puts(message: &str) {
    for &byte in message.as_bytes() {
        if byte == b'\n' {
            putc(b'\r');
        }
        putc(byte);
    }
    vga::puts(message);
    crate::framebuffer::puts(message);
}

/// Print a u64 as hexadecimal with a `0x` prefix.
pub fn put_hex(value: u64) {
    const DIGITS: &[u8; 16] = b"0123456789abcdef";
    let mut buffer = [0u8; 18];
    buffer[0] = b'0';
    buffer[1] = b'x';
    for i in 0..16 {
        let shift = (15 - i) * 4;
        buffer[2 + i] = DIGITS[((value >> shift) & 0xF) as usize];
    }
    // Skip leading zeroes but keep at least one digit.
    let mut start = 2;
    while start < 17 && buffer[start] == b'0' {
        start += 1;
    }
    putc(b'0');
    putc(b'x');
    for &byte in &buffer[start..] {
        putc(byte);
    }
    vga::puts("0x");
    for &byte in &buffer[start..] {
        vga::putc(byte);
    }
}

/// Print a u64 in decimal.
pub fn put_dec(value: u64) {
    if value == 0 {
        putc(b'0');
        vga::putc(b'0');
        crate::framebuffer::putc(b'0');
        return;
    }
    let mut buffer = [0u8; 20];
    let mut remaining = value;
    let mut start = buffer.len();
    while remaining > 0 {
        start -= 1;
        buffer[start] = b'0' + (remaining % 10) as u8;
        remaining /= 10;
    }
    for &byte in &buffer[start..] {
        putc(byte);
        vga::putc(byte);
        crate::framebuffer::putc(byte);
    }
}

/// Write a debug-prefixed line.
pub fn debug(message: &str) {
    puts("[debug] ");
    puts(message);
    puts("\n");
}

/// Write a plain line.
pub fn println(message: &str) {
    puts(message);
    puts("\n");
}
