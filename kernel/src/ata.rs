//! ATA PIO driver for the primary IDE bus (I/O ports 0x1F0-0x1F7).

use core::sync::atomic::{AtomicBool, Ordering};

use crate::diagnostics;
use crate::io::{inb, outb};

/// True when the primary master responded to a sector read during probe.
static PRESENT: AtomicBool = AtomicBool::new(false);

/// Whether a readable ATA disk was detected at boot.
pub fn present() -> bool {
    PRESENT.load(Ordering::Relaxed)
}

const DATA: u16 = 0x1F0;
const SECTOR_COUNT: u16 = 0x1F2;
const LBA_LOW: u16 = 0x1F3;
const LBA_MID: u16 = 0x1F4;
const LBA_HIGH: u16 = 0x1F5;
const DRIVE_HEAD: u16 = 0x1F6;
const STATUS: u16 = 0x1F7;
const COMMAND: u16 = 0x1F7;

const STATUS_BSY: u8 = 1 << 7;
const STATUS_DRQ: u8 = 1 << 3;
const STATUS_ERR: u8 = 1 << 0;

/// Wait until BSY clears, with a bounded loop.
unsafe fn wait_not_busy() -> bool {
    for _ in 0..100_000 {
        if inb(STATUS) & STATUS_BSY == 0 {
            return true;
        }
        core::hint::spin_loop();
    }
    false
}

/// Read one 512-byte sector into `buffer`.
pub fn read_sector(lba: u32, buffer: &mut [u8; 512]) -> Result<(), &'static str> {
    unsafe {
        if !wait_not_busy() {
            return Err("ATA busy timeout");
        }
        // Wait for the device to become ready after reset/handoff.
        for _ in 0..100_000 {
            let status = inb(STATUS);
            if status & (STATUS_BSY | 0x40) == 0x40 {
                break;
            }
            core::hint::spin_loop();
        }

        // Select master drive with LBA addressing.
        outb(DRIVE_HEAD, 0xE0 | (((lba >> 24) & 0x0F) as u8));
        outb(SECTOR_COUNT, 1);
        outb(LBA_LOW, lba as u8);
        outb(LBA_MID, (lba >> 8) as u8);
        outb(LBA_HIGH, (lba >> 16) as u8);
        outb(COMMAND, 0x20); // READ SECTORS

        if !wait_not_busy() {
            return Err("ATA read timeout");
        }
        let status = inb(STATUS);
        if status & STATUS_ERR != 0 || status & STATUS_DRQ == 0 {
            return Err("ATA read error");
        }

        for word in 0..256 {
            let value = inw(DATA);
            buffer[word * 2] = value as u8;
            buffer[word * 2 + 1] = (value >> 8) as u8;
        }
    }
    Ok(())
}

/// Write one 512-byte sector at `lba`.
pub fn write_sector(lba: u32, data: &[u8; 512]) -> Result<(), &'static str> {
    unsafe {
        if !wait_not_busy() {
            return Err("ATA busy timeout");
        }
        outb(DRIVE_HEAD, 0xE0 | (((lba >> 24) & 0x0F) as u8));
        outb(SECTOR_COUNT, 1);
        outb(LBA_LOW, lba as u8);
        outb(LBA_MID, (lba >> 8) as u8);
        outb(LBA_HIGH, (lba >> 16) as u8);
        outb(COMMAND, 0x30); // WRITE SECTORS

        if !wait_not_busy() {
            return Err("ATA write timeout");
        }
        if inb(STATUS) & STATUS_ERR != 0 {
            return Err("ATA write error");
        }

        for word in 0..256 {
            let value = u16::from_le_bytes([data[word * 2], data[word * 2 + 1]]);
            outw(DATA, value);
        }

        if !wait_not_busy() {
            return Err("ATA write completion timeout");
        }
        if inb(STATUS) & STATUS_ERR != 0 {
            return Err("ATA write failed");
        }
    }
    Ok(())
}

#[inline]
unsafe fn outw(port: u16, value: u16) {
    core::arch::asm!("out dx, ax", in("dx") port, in("ax") value, options(nostack, preserves_flags));
}

#[inline]
unsafe fn inw(port: u16) -> u16 {
    let value: u16;
    core::arch::asm!("in ax, dx", in("dx") port, out("ax") value, options(nostack, preserves_flags));
    value
}

/// Probe the primary master and, if present, dump the first sector's signature.
pub fn probe() {
    unsafe {
        // A soft reset brings the controller to a known state after firmware
        // handoff (especially under UEFI).
        outb(0x3F6, 4);
        for _ in 0..1000 {
            core::hint::spin_loop();
        }
        outb(0x3F6, 0);
        for _ in 0..1000 {
            core::hint::spin_loop();
        }

        if !wait_not_busy() {
            diagnostics::debug("ATA: no respond; skipping probe");
            return;
        }
        outb(DRIVE_HEAD, 0xE0);

        let mut buffer = [0u8; 512];
        match read_sector(0, &mut buffer) {
            Ok(()) => {
                PRESENT.store(true, Ordering::Relaxed);
                diagnostics::puts("[ata] primary master LBA0: ");
                for &byte in &buffer[..8] {
                    diagnostics::put_hex(byte as u64);
                    diagnostics::putc(b' ');
                }
                diagnostics::puts("\n");
                if buffer[510] == 0x55 && buffer[511] == 0xAA {
                    diagnostics::debug("ATA: valid MBR signature");
                }
            }
            Err(error) => {
                diagnostics::puts("[ata] probe failed: ");
                diagnostics::puts(error);
                diagnostics::puts("\n");
            }
        }
    }
}
