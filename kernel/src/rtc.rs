//! CMOS real-time clock reader for the taskbar clock.

use crate::io::{inb, outb};

const CMOS_INDEX: u16 = 0x70;
const CMOS_DATA: u16 = 0x71;

fn read_register(register: u8) -> u8 {
    unsafe {
        // Bit 7 disables NMI while the register is read.
        outb(CMOS_INDEX, register | 0x80);
        inb(CMOS_DATA)
    }
}

fn bcd_to_binary(value: u8) -> u8 {
    (value & 0x0F) + ((value >> 4) * 10)
}

fn update_in_progress() -> bool {
    read_register(0x0A) & 0x80 != 0
}

/// Read the wall clock as `(hour, minute, second)` in 24-hour form.
pub fn time() -> (u8, u8, u8) {
    // Wait for the RTC to finish its update cycle so the fields are coherent.
    for _ in 0..100_000 {
        if !update_in_progress() {
            break;
        }
    }

    let status_b = read_register(0x0B);
    let binary = status_b & 0x04 != 0;
    let twenty_four_hour = status_b & 0x02 != 0;

    let mut raw_hour = read_register(0x04);
    let raw_minute = read_register(0x02);
    let raw_second = read_register(0x00);

    let pm = !twenty_four_hour && raw_hour & 0x80 != 0;
    raw_hour &= 0x7F;

    let mut hour = if binary { raw_hour } else { bcd_to_binary(raw_hour) };
    let minute = if binary { raw_minute } else { bcd_to_binary(raw_minute) };
    let second = if binary { raw_second } else { bcd_to_binary(raw_second) };

    if !twenty_four_hour {
        if hour == 12 {
            hour = 0;
        }
        if pm {
            hour += 12;
        }
    }

    (hour % 24, minute % 60, second % 60)
}
