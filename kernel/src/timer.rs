//! Programmable Interval Timer (PIT) channel 0 at ~100 Hz.

use core::sync::atomic::{AtomicU64, Ordering};

use crate::io::outb;

const PIT_CHANNEL0: u16 = 0x40;
const PIT_COMMAND: u16 = 0x43;
const DIVISOR: u16 = 11_931; // 1_193_182 Hz / 100 Hz

static TICKS: AtomicU64 = AtomicU64::new(0);

/// Program the PIT for periodic mode 3 interrupts.
pub fn init() {
    unsafe {
        outb(PIT_COMMAND, 0x36); // channel 0, low/high byte, mode 3, binary
        outb(PIT_CHANNEL0, DIVISOR as u8);
        outb(PIT_CHANNEL0, (DIVISOR >> 8) as u8);
    }
}

/// Timer IRQ handler: advance the tick counter.
pub fn tick() {
    TICKS.fetch_add(1, Ordering::Relaxed);
}

/// Number of timer ticks since boot.
pub fn ticks() -> u64 {
    TICKS.load(Ordering::Relaxed)
}
