//! Coherent CMOS wall-clock snapshots. Firmware time is used as supplied;
//! there is no timezone database or network clock synchronization.
use crate::io::{inb, outb};

fn register(index: u8) -> u8 {
    unsafe {
        outb(0x70, index | 0x80);
        let value = inb(0x71);
        outb(0x70, index); // Re-enable NMI after the access.
        value
    }
}

#[derive(Clone, Copy)]
pub struct DateTime {
    pub year: u16,
    pub month: u8,
    pub day: u8,
    pub hour: u8,
    pub minute: u8,
    pub second: u8,
}

fn sample() -> Option<[u8; 8]> {
    if register(0x0a) & 0x80 != 0 { return None; }
    let data = [register(0), register(2), register(4), register(7),
        register(8), register(9), register(0x32), register(0x0b)];
    if register(0x0a) & 0x80 != 0 { None } else { Some(data) }
}

/// Return two matching snapshots or report an unavailable RTC.
pub fn now() -> Option<DateTime> {
    crate::interrupt::without_interrupts(|| {
        for _ in 0..100 {
            let Some(a) = sample() else { continue };
            if sample() != Some(a) { continue; }
            let decode = |v: u8| if a[7] & 4 != 0 { v } else { (v & 15) + (v >> 4) * 10 };
            let mut hour = decode(a[2] & 0x7f);
            if a[7] & 2 == 0 { hour = hour % 12 + if a[2] & 0x80 != 0 { 12 } else { 0 }; }
            // Conventional century register when present; otherwise CMOS
            // provides only two digits and this OS uses the 2000–2099 epoch.
            let century = decode(a[6]);
            let dt = DateTime {
                year: u16::from(if (19..=99).contains(&century) { century } else { 20 }) * 100 + u16::from(decode(a[5])),
                month: decode(a[4]), day: decode(a[3]),
                hour, minute: decode(a[1]), second: decode(a[0]),
            };
            return ((1..=12).contains(&dt.month) && (1..=31).contains(&dt.day)
                && dt.hour < 24 && dt.minute < 60 && dt.second < 60).then_some(dt);
        }
        None
    })
}
