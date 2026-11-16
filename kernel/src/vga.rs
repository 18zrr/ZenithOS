//! VGA text-mode console (80x25) at physical address 0xB8000.
//!
//! The boot page tables identity-map this address, so it can be written
//! directly without a framebuffer driver.

use crate::sync::SpinLock;

const VGA_ADDR: usize = 0xB8000;
const WIDTH: usize = 80;
const HEIGHT: usize = 25;
const DEFAULT_ATTR: u8 = 0x07; // light grey on black

struct Console {
    row: usize,
    col: usize,
    attr: u8,
}

static CONSOLE: SpinLock<Console> = SpinLock::new(Console {
    row: 0,
    col: 0,
    attr: DEFAULT_ATTR,
});

#[inline]
fn write_cell(row: usize, col: usize, byte: u8, attr: u8) {
    let index = row * WIDTH + col;
    unsafe {
        let cell = (VGA_ADDR + index * 2) as *mut u16;
        core::ptr::write_volatile(cell, u16::from_le_bytes([byte, attr]));
    }
}

/// Clear the screen and reset the cursor.
pub fn clear() {
    let mut console = CONSOLE.lock();
    for row in 0..HEIGHT {
        for col in 0..WIDTH {
            write_cell(row, col, b' ', DEFAULT_ATTR);
        }
    }
    console.row = 0;
    console.col = 0;
}

/// Scroll the whole screen up by one line.
fn scroll(console: &mut Console) {
    for row in 1..HEIGHT {
        for col in 0..WIDTH {
            let cell = unsafe {
                core::ptr::read_volatile((VGA_ADDR + (row * WIDTH + col) * 2) as *const u16)
            };
            unsafe {
                core::ptr::write_volatile(
                    (VGA_ADDR + ((row - 1) * WIDTH + col) * 2) as *mut u16,
                    cell,
                );
            }
        }
    }
    for col in 0..WIDTH {
        write_cell(HEIGHT - 1, col, b' ', console.attr);
    }
    console.row = HEIGHT - 1;
}

/// Write one byte to the VGA console, handling newline, carriage return and
/// backspace.
pub fn putc(byte: u8) {
    let mut console = CONSOLE.lock();
    match byte {
        b'\n' => {
            console.col = 0;
            console.row += 1;
        }
        b'\r' => console.col = 0,
        0x08 => {
            if console.col > 0 {
                console.col -= 1;
                write_cell(console.row, console.col, b' ', console.attr);
            }
        }
        other => {
            write_cell(console.row, console.col, other, console.attr);
            console.col += 1;
        }
    }

    if console.col >= WIDTH {
        console.col = 0;
        console.row += 1;
    }
    if console.row >= HEIGHT {
        scroll(&mut console);
    }
}

/// Write a string to the VGA console.
pub fn puts(text: &str) {
    for &byte in text.as_bytes() {
        putc(byte);
    }
}
