//! Zenith OS Driver Framework
//!
//! This module provides the core infrastructure for managing hardware drivers
//! in the Zenith operating system.

#![no_std]
#![deny(warnings)]

#[macro_use]
extern crate alloc;

use alloc::boxed::Box;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

/// Driver result type
pub type Result<T> = core::result::Result<T, DriverError>;

/// Error types for driver operations
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DriverError {
    InitializationFailed,
    DeviceNotFound,
    UnsupportedDevice,
    InvalidOperation,
    ResourceExhausted,
    UnknownError,
}

impl core::fmt::Display for DriverError {
    fn fmt(&self, f: &mut core::fmt::Formatter) -> core::fmt::Result {
        match self {
            DriverError::InitializationFailed => write!(f, "Driver initialization failed"),
            DriverError::DeviceNotFound => write!(f, "Hardware device not found"),
            DriverError::UnsupportedDevice => write!(f, "Unsupported hardware device"),
            DriverError::InvalidOperation => write!(f, "Invalid driver operation"),
            DriverError::ResourceExhausted => write!(f, "Driver resources exhausted"),
            DriverError::UnknownError => write!(f, "Unknown driver error"),
        }
    }
}

impl core::convert::From<DriverError> for String {
    fn from(error: DriverError) -> Self {
        format!("{}", error)
    }
}

/// Hardware device identifier
#[derive(Debug)]
pub struct DeviceId {
    pub vendor_id: u16,
    pub device_id: u16,
    pub class_code: u8,
    pub subclass: u8,
    pub prog_if: u8,
}

impl DeviceId {
    /// Create a new device ID
    pub fn new(vendor_id: u16, device_id: u16, class_code: u8, subclass: u8, prog_if: u8) -> Self {
        DeviceId {
            vendor_id,
            device_id,
            class_code,
            subclass,
            prog_if,
        }
    }
}

/// Driver trait that all drivers must implement
pub trait Driver {
    /// Get the driver name
    fn name(&self) -> &str;
    
    /// Get supported device IDs
    fn device_ids(&self) -> &[DeviceId];
    
    /// Initialize the driver
    fn initialize(&mut self) -> Result<()> {
        Ok(())
    }
    
    /// Handle hardware interrupts
    fn handle_interrupt(&self, _irq_number: u32) -> bool {
        false // Default implementation - no interrupt handling
    }
    
    /// Get driver statistics
    fn stats(&self) -> DriverStats;
}

/// Driver statistics structure
#[derive(Debug, Clone, Copy)]
pub struct DriverStats {
    pub devices_detected: usize,
    pub operations_performed: usize,
    pub errors_encountered: usize,
}

impl Default for DriverStats {
    fn default() -> Self {
        DriverStats {
            devices_detected: 0,
            operations_performed: 0,
            errors_encountered: 0,
        }
    }
}

/// Driver manager to handle driver lifecycle
pub struct DriverManager {
    drivers: Vec<Box<dyn Driver>>,
    loaded_drivers: Vec<String>,
}

impl DriverManager {
    /// Create a new driver manager
    pub fn new() -> Self {
        DriverManager {
            drivers: Vec::new(),
            loaded_drivers: Vec::new(),
        }
    }

    /// Register a driver with the system
    pub fn register_driver(&mut self, driver: Box<dyn Driver>) {
        let name = driver.name().to_string();
        self.drivers.push(driver);
        self.loaded_drivers.push(name);
    }

    /// Initialize all registered drivers
    pub fn initialize_all(&mut self) -> Result<()> {
        for driver in &mut self.drivers {
            driver.initialize()?;
        }
        Ok(())
    }

    /// Get a list of loaded driver names
    pub fn get_loaded_drivers(&self) -> &[String] {
        &self.loaded_drivers
    }
}