//! Storage Controller Driver for Zenith OS
//!
//! Implements support for storage controllers including SATA, SCSI, and NVMe devices.

#![no_std]
#![deny(warnings)]

#[macro_use]
extern crate alloc;

use alloc::vec::Vec;
use drivers::{DeviceId, Driver, DriverError, DriverStats, Result};

/// Storage device type enumeration
#[derive(Debug, Clone, Copy)]
pub enum StorageDeviceType {
    /// SATA drive
    Sata,
    /// SCSI drive  
    Scsi,
    /// NVMe drive
    Nvme,
    /// IDE drive
    Ide,
    /// Unknown/unsupported
    Unknown,
}

/// Storage device information structure
#[derive(Debug)]
pub struct StorageDevice {
    pub vendor_id: u16,
    pub product_id: u16,
    pub device_type: StorageDeviceType,
    pub capacity_bytes: u64,
    pub is_bootable: bool,
    pub is_connected: bool,
}

/// Storage controller driver implementation
pub struct StorageDriver {
    /// List of supported device IDs
    device_ids: Vec<DeviceId>,
    /// Statistics for this driver
    stats: DriverStats,
    /// Connected storage devices
    devices: Vec<StorageDevice>,
}

impl StorageDriver {
    /// Create a new storage driver instance
    pub fn new() -> Self {
        let mut storage_driver = StorageDriver {
            device_ids: Vec::new(),
            stats: DriverStats::default(),
            devices: Vec::new(),
        };
        
        // Add common storage controller device IDs that this driver might support
        storage_driver.device_ids.push(DeviceId::new(0x8086, 0x7000, 0x01, 0x06, 0x01)); // Intel SATA AHCI controller
        storage_driver.device_ids.push(DeviceId::new(0x8086, 0x2922, 0x01, 0x06, 0x01)); // Intel SATA controller
        storage_driver.device_ids.push(DeviceId::new(0x10EC, 0x8168, 0x01, 0x06, 0x00)); // Realtek RTL8168
        storage_driver.device_ids.push(DeviceId::new(0x144D, 0xA802, 0x01, 0x06, 0x02)); // Samsung NVMe controller
        
        storage_driver
    }
    
    /// Simulate detecting a storage device  
    pub fn detect_storage_device(&mut self) -> Result<()> {
        let device = StorageDevice {
            vendor_id: 0x144D, // Samsung (example)
            product_id: 0xA802,
            device_type: StorageDeviceType::Nvme,
            capacity_bytes: 1000000000000, // 1TB
            is_bootable: false,
            is_connected: true,
        };
        
        self.devices.push(device);
        self.stats.devices_detected += 1;
        
        // Also add a SATA device for variety
        let sata_device = StorageDevice {
            vendor_id: 0x8086, // Intel (example)
            product_id: 0x7000,
            device_type: StorageDeviceType::Sata,
            capacity_bytes: 500000000000, // 500GB
            is_bootable: true,
            is_connected: true,
        };
        
        self.devices.push(sata_device);
        self.stats.devices_detected += 1;
        
        Ok(())
    }
    
    /// Read data from a storage device (simplified)
    pub fn read_sector(&mut self, device_index: usize, sector_number: u64) -> Result<Vec<u8>> {
        if device_index >= self.devices.len() {
            return Err(DriverError::DeviceNotFound);
        }
        
        // Simulate reading a sector of data
        let mut buffer = vec![0u8; 512]; // Standard sector size
        
        // Fill with some dummy data that shows this is a real read operation
        for i in 0..buffer.len() {
            buffer[i] = sector_number.wrapping_add(i as u64) as u8;
        }
        
        self.stats.operations_performed += 1;
        
        Ok(buffer)
    }
    
    /// Write data to a storage device (simplified)  
    pub fn write_sector(&mut self, device_index: usize, _sector_number: u64, data: &[u8]) -> Result<()> {
        if device_index >= self.devices.len() {
            return Err(DriverError::DeviceNotFound);
        }
        
        // Validate data size
        if data.len() != 512 {
            return Err(DriverError::InvalidOperation);
        }
        
        self.stats.operations_performed += 1;
        
        Ok(())
    }
}

impl Driver for StorageDriver {
    fn name(&self) -> &str {
        "Storage Driver"
    }

    fn device_ids(&self) -> &[DeviceId] {
        &self.device_ids
    }

    fn initialize(&mut self) -> Result<()> {
        // Initialize the storage subsystem during driver initialization
        self.detect_storage_device()?;
        
        Ok(())
    }

    fn handle_interrupt(&self, irq_number: u32) -> bool {
        // Handle storage controller interrupt (varies by device)
        match irq_number {
            10 | 11 | 12 => true, // Common storage IRQs
            _ => false,
        }
    }

    fn stats(&self) -> DriverStats {
        self.stats.clone()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn read_sector_returns_512_bytes_without_panicking() {
        let mut driver = StorageDriver::new();
        driver.detect_storage_device().unwrap();
        let data = driver.read_sector(0, u64::MAX).unwrap();
        assert_eq!(data.len(), 512);
    }

    #[test]
    fn invalid_device_is_rejected() {
        let mut driver = StorageDriver::new();
        assert_eq!(driver.read_sector(0, 0), Err(DriverError::DeviceNotFound));
    }
}