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
