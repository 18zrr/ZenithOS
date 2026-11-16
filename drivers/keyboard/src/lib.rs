//! Keyboard Driver for Zenith OS
//!
//! Implements support for keyboard input devices.

#![no_std]
#![deny(warnings)]

extern crate alloc;

use alloc::string::{String, ToString};
use alloc::vec::Vec;
use drivers::{DeviceId, Driver, DriverError, DriverStats, Result};

/// Keyboard scan code representation
#[derive(Debug, Clone, Copy)]
pub enum ScanCode {
    /// Standard ASCII characters
    Ascii(u8),
    /// Special keys (control, shift, etc.)
    Control(u8),
    /// Function keys
    Function(u8),
    /// Unknown/invalid scan code
    Unknown,
}

/// Keyboard device state
#[derive(Debug)]
pub struct KeyboardDevice {
    pub vendor_id: u16,
    pub product_id: u16,
    pub layout: String, // e.g., "US", "UK"
    pub is_connected: bool,
}

/// Keyboard driver implementation
pub struct KeyboardDriver {
    /// List of supported device IDs
    device_ids: Vec<DeviceId>,
    /// Statistics for this driver
    stats: DriverStats,
    /// Connected keyboard devices
    keyboards: Vec<KeyboardDevice>,
    /// Input buffer for key events
    input_buffer: Vec<ScanCode>,
}

impl KeyboardDriver {
    /// Create a new keyboard driver instance
    pub fn new() -> Self {
        let mut keyboard_driver = KeyboardDriver {
            device_ids: Vec::new(),
            stats: DriverStats::default(),
            keyboards: Vec::new(),
            input_buffer: Vec::new(),
        };
        
        // Add common keyboard device IDs that this driver might support
        keyboard_driver.device_ids.push(DeviceId::new(0x046D, 0xC52B, 0x03, 0x01, 0x01)); // Logitech keyboard
        keyboard_driver.device_ids.push(DeviceId::new(0x046D, 0xC52C, 0x03, 0x01, 0x01)); // Logitech keyboard (different PID)
        
        keyboard_driver
    }
    
    /// Simulate detecting a keyboard device
    pub fn detect_keyboard(&mut self) -> Result<()> {
        let kb = KeyboardDevice {
            vendor_id: 0x046D, // Logitech
            product_id: 0xC52B,
            layout: "US".to_string(),
            is_connected: true,
        };
        
        self.keyboards.push(kb);
        self.stats.devices_detected += 1;
        
        Ok(())
    }
    
    /// Process a scan code from keyboard input
    pub fn process_scan_code(&mut self, scancode: ScanCode) -> Result<()> {
        // Add to input buffer (simplified implementation)
        if self.input_buffer.len() < 256 { // Limit buffer size
            self.input_buffer.push(scancode);
            self.stats.operations_performed += 1;
        } else {
            return Err(DriverError::ResourceExhausted);
        }
        
        Ok(())
    }
    
    /// Get the next available key press from input buffer
    pub fn get_next_key(&mut self) -> Option<ScanCode> {
        if !self.input_buffer.is_empty() {
            Some(self.input_buffer.remove(0))
        } else {
            None
        }
    }
}

impl Driver for KeyboardDriver {
    fn name(&self) -> &str {
        "Keyboard Driver"
    }

    fn device_ids(&self) -> &[DeviceId] {
        &self.device_ids
    }

    fn initialize(&mut self) -> Result<()> {
        // Initialize the keyboard subsystem during driver initialization
        self.detect_keyboard()?;
        
        Ok(())
    }

    fn handle_interrupt(&self, irq_number: u32) -> bool {
        // Handle keyboard interrupt (IRQ 1 for PS/2 keyboards)
        if irq_number == 1 {
            true // We handled this interrupt
        } else {
            false // Not our interrupt
        }
    }

    fn stats(&self) -> DriverStats {
        self.stats.clone()
    }
}