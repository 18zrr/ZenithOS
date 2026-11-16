//! Storage management for Zenith FS

use crate::{FsError, SuperBlock, BLOCK_SIZE};
use alloc::vec::Vec;

/// Storage device abstraction
pub struct StorageDevice {
    pub data: Vec<u8>,
    pub superblock: SuperBlock,
    /// Tracks which blocks have been handed out by `allocate_block`.
    allocated: Vec<bool>,
}

impl StorageDevice {
    /// Create a new storage device with given size
    pub fn new(size_in_bytes: usize) -> Self {
        let mut data = vec![0u8; size_in_bytes];
        let total_blocks = size_in_bytes / BLOCK_SIZE;

        // Initialize superblock at beginning of storage. Block 0 is reserved
        // for the superblock itself and is therefore always marked used.
        let superblock = SuperBlock {
            total_blocks,
            free_blocks: total_blocks.saturating_sub(1),
            ..Default::default()
        };

        if data.len() >= 4 {
            data[..4].copy_from_slice(&superblock.magic.to_le_bytes());
        }

        let mut allocated = vec![false; total_blocks];
        if total_blocks > 0 {
            allocated[0] = true;
        }

        StorageDevice {
            data,
            superblock,
            allocated,
        }
    }

    /// Read a block from storage
    pub fn read_block(&self, block_number: usize) -> Result<&[u8], FsError> {
        let start = block_number.checked_mul(BLOCK_SIZE).ok_or(FsError::InvalidBlockNumber)?;
        let end = start.checked_add(BLOCK_SIZE).ok_or(FsError::InvalidBlockNumber)?;

        if end > self.data.len() {
            return Err(FsError::InvalidBlockNumber);
        }

        Ok(&self.data[start..end])
    }

    /// Write a block to storage
    pub fn write_block(&mut self, block_number: usize, data: &[u8]) -> Result<(), FsError> {
        let start = block_number.checked_mul(BLOCK_SIZE).ok_or(FsError::InvalidBlockNumber)?;
        let end = start.checked_add(BLOCK_SIZE).ok_or(FsError::InvalidBlockNumber)?;

        if end > self.data.len() {
            return Err(FsError::InvalidBlockNumber);
        }

        if data.len() != BLOCK_SIZE {
            return Err(FsError::IoError);
        }

        self.data[start..end].copy_from_slice(data);
        Ok(())
    }

    /// Allocate a block, marking it used so it will not be handed out twice.
    pub fn allocate_block(&mut self) -> Result<usize, FsError> {
        for i in 0..self.superblock.total_blocks {
            if !self.allocated[i] {
                self.allocated[i] = true;
                self.superblock.free_blocks = self.superblock.free_blocks.saturating_sub(1);
                return Ok(i);
            }
        }

        Err(FsError::DiskFull)
    }

    /// Deallocate a previously allocated block.
    pub fn deallocate_block(&mut self, block_number: usize) -> Result<(), FsError> {
        if block_number >= self.superblock.total_blocks || !self.allocated[block_number] {
            return Err(FsError::InvalidBlockNumber);
        }

        self.allocated[block_number] = false;
        self.superblock.free_blocks += 1;
        Ok(())
    }

    /// Check whether a block is currently in use.
    pub fn is_block_used(&self, block_number: usize) -> bool {
        self.allocated.get(block_number).copied().unwrap_or(true)
    }
}
