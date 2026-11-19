//! Runtime display-mode control.
//!
//! QEMU's standard VGA and VirtualBox's VBE adapter expose the Bochs VBE
//! "dispi" register interface at I/O ports 0x1CE/0x1CF. That interface can
//! switch the linear framebuffer resolution while the guest is running.
//!
//! Physical GPUs need a native display driver (modesetting, EDID validation
//! and display-link programming). None of those drivers are implemented, so
//! mode switching reports an honest error there instead of pretending.
//!
//! Refresh rate is not a Bochs VBE register. For a VM the host owns the
//! virtual monitor timing, and a physical panel is programmed by its GPU
//! driver. The Settings UI therefore reports refresh as host-controlled.

use core::sync::atomic::{AtomicBool, Ordering};

use crate::io::{inw, outw};
use crate::pci;
use crate::{diagnostics, framebuffer};

const DISPI_INDEX: u16 = 0x1CE;
const DISPI_DATA: u16 = 0x1CF;

const VBE_DISPI_INDEX_ID: u16 = 0;
const VBE_DISPI_INDEX_XRES: u16 = 1;
const VBE_DISPI_INDEX_YRES: u16 = 2;
const VBE_DISPI_INDEX_BPP: u16 = 3;
const VBE_DISPI_INDEX_ENABLE: u16 = 4;
const VBE_DISPI_INDEX_VIRT_WIDTH: u16 = 6;
const VBE_DISPI_INDEX_VIRT_HEIGHT: u16 = 7;
const VBE_DISPI_INDEX_X_OFFSET: u16 = 8;
const VBE_DISPI_INDEX_Y_OFFSET: u16 = 9;
const VBE_DISPI_INDEX_VIDEO_MEMORY_64K: u16 = 10;

const VBE_DISPI_ENABLED: u16 = 0x01;
const VBE_DISPI_LFB_ENABLED: u16 = 0x40;

/// Maximum software surface, matching the compositor's preallocated buffers.
pub const MAX_WIDTH: usize = 3840;
pub const MAX_HEIGHT: usize = 2160;

static AVAILABLE: AtomicBool = AtomicBool::new(false);

fn dispi_write(index: u16, value: u16) {
    unsafe {
        outw(DISPI_INDEX, index);
        outw(DISPI_DATA, value);
    }
}

fn dispi_read(index: u16) -> u16 {
    unsafe {
        outw(DISPI_INDEX, index);
        inw(DISPI_DATA)
    }
}

/// Detect a Bochs-compatible VBE adapter and remember whether modes can be set.
pub fn init() {
    let mut found = false;
    'search: for bus in 0..8u8 {
        for slot in 0..32u8 {
            for function in 0..8u8 {
                let id = pci::read(bus, slot, function, 0);
                let vendor = (id & 0xFFFF) as u16;
                let device = (id >> 16) as u16;
                let bochs = vendor == 0x1234 && device == 0x1111;
                let virtualbox = vendor == 0x80EE && device == 0xBEEF;
                if !bochs && !virtualbox {
                    continue;
                }
                // The register file is only safe to touch if it answers with a
                // Bochs VBE ID (0xB0C0..0xB0C5). A bare VGA adapter may map the
                // PCI IDs without exposing the dispi registers.
                let dispi_id = dispi_read(VBE_DISPI_INDEX_ID);
                if (0xB0C0..=0xB0C5).contains(&dispi_id) {
                    found = true;
                    diagnostics::puts("[display] runtime mode setting: ");
                    diagnostics::puts(if bochs {
                        "QEMU std VGA"
                    } else {
                        "VirtualBox VBE"
                    });
                    diagnostics::puts(", dispi id=");
                    diagnostics::put_hex(dispi_id as u64);
                    diagnostics::puts("\n");
                    break 'search;
                }
            }
        }
    }

    AVAILABLE.store(found, Ordering::Release);
    if !found {
        diagnostics::debug("[display] no Bochs VBE runtime mode interface");
    }
}

/// Whether the running adapter supports guest-side mode changes.
pub fn available() -> bool {
    AVAILABLE.load(Ordering::Acquire)
}

/// Human-readable backend description for the Settings card.
pub fn backend_name() -> &'static str {
    if available() {
        "Bochs VBE"
    } else {
        "Firmware only"
    }
}

/// Parse `WIDTHxHEIGHT` (either case for the separator).
pub fn parse(text: &str) -> Option<(usize, usize)> {
    let (width, height) = text.split_once('x').or_else(|| text.split_once('X'))?;
    let width = width.trim().parse::<usize>().ok()?;
    let height = height.trim().parse::<usize>().ok()?;
    if width == 0 || height == 0 {
        return None;
    }
    Some((width, height))
}

/// Validate a requested mode against the compositor's surface limit.
pub fn validate(width: usize, height: usize) -> Result<(), &'static str> {
    if width < 640 || height < 480 {
        return Err("Minimum resolution is 640x480");
    }
    if width > MAX_WIDTH || height > MAX_HEIGHT {
        return Err("Maximum resolution is 3840x2160");
    }
    if width.saturating_mul(height) > MAX_WIDTH * MAX_HEIGHT {
        return Err("Resolution uses more than 3840x2160 pixels");
    }
    // The Bochs VBE interface used by QEMU and VirtualBox rejects widths that
    // are not a multiple of eight (common modes such as 1366x768 are rounded
    // by native GPU drivers instead, which this kernel does not implement).
    if width % 8 != 0 {
        return Err("Width must be a multiple of 8 on this adapter");
    }
    if height % 2 != 0 {
        return Err("Height must be even on this adapter");
    }
    Ok(())
}

fn program_mode(width: usize, height: usize) -> bool {
    let width = width as u16;
    let height = height as u16;
    dispi_write(VBE_DISPI_INDEX_ENABLE, 0);
    dispi_write(VBE_DISPI_INDEX_XRES, width);
    dispi_write(VBE_DISPI_INDEX_YRES, height);
    dispi_write(VBE_DISPI_INDEX_BPP, 32);
    dispi_write(VBE_DISPI_INDEX_VIRT_WIDTH, width);
    dispi_write(VBE_DISPI_INDEX_VIRT_HEIGHT, height);
    dispi_write(VBE_DISPI_INDEX_X_OFFSET, 0);
    dispi_write(VBE_DISPI_INDEX_Y_OFFSET, 0);
    dispi_write(
        VBE_DISPI_INDEX_ENABLE,
        VBE_DISPI_ENABLED | VBE_DISPI_LFB_ENABLED,
    );

    dispi_read(VBE_DISPI_INDEX_XRES) == width
        && dispi_read(VBE_DISPI_INDEX_YRES) == height
        && dispi_read(VBE_DISPI_INDEX_BPP) == 32
        && dispi_read(VBE_DISPI_INDEX_ENABLE) & VBE_DISPI_ENABLED != 0
}

/// Switch the live framebuffer resolution.
///
/// The linear framebuffer address does not move for these adapters, so only
/// the dimensions and pitch reported to the compositor change. On failure the
/// previous mode is restored and a description is returned for the UI.
pub fn set_mode(width: usize, height: usize) -> Result<(), &'static str> {
    if !available() {
        return Err("This display has no guest mode-setting driver");
    }
    if !framebuffer::available() {
        return Err("No linear framebuffer is available");
    }
    validate(width, height)?;

    let (current_width, current_height) = framebuffer::dimensions();
    if current_width == width && current_height == height {
        return Ok(());
    }

    let required = width.saturating_mul(height).saturating_mul(4);
    let video_memory = dispi_read(VBE_DISPI_INDEX_VIDEO_MEMORY_64K) as usize * 64 * 1024;
    if video_memory != 0 && required > video_memory {
        return Err("Not enough video memory for that resolution");
    }

    if !program_mode(width, height) {
        // The adapter may clamp or reject unsupported modes; leave the guest
        // at its previous, working size.
        if current_width != 0 && current_height != 0 {
            program_mode(current_width, current_height);
        }
        return Err("The display rejected that resolution");
    }

    framebuffer::set_mode(width, height, width * 4);
    diagnostics::puts("[display] mode ");
    diagnostics::put_dec(width as u64);
    diagnostics::puts("x");
    diagnostics::put_dec(height as u64);
    diagnostics::puts("\n");
    Ok(())
}
