//! File operations for Zenith FS

use crate::{FsError, Inode, Metadata};
use alloc::vec::Vec;
use core::cmp;

/// File structure for filesystem operations
pub struct File {
    pub inode: Inode,
    pub data: Vec<u8>,
}

impl File {
    /// Create a new file with given metadata
    pub fn new(metadata: Metadata) -> Self {
        File {
            inode: Inode {
                metadata,
                ..Default::default()
            },
            data: Vec::new(),
        }
    }

    /// Write data to the file
    pub fn write(&mut self, data: &[u8]) -> Result<usize, FsError> {
        self.data.extend_from_slice(data);
        self.inode.metadata.size = self.data.len();
        Ok(data.len())
    }

    /// Read data from the file
    pub fn read(&self, buffer: &mut [u8], offset: usize) -> Result<usize, FsError> {
        let end = cmp::min(offset + buffer.len(), self.data.len());
        if offset >= self.data.len() {
            return Ok(0);
        }
        
        let read_len = end - offset;
        buffer[..read_len].copy_from_slice(&self.data[offset..end]);
        Ok(read_len)
    }

    /// Get file size
    pub fn size(&self) -> usize {
        self.inode.metadata.size
    }

    /// Truncate the file to specified size
    pub fn truncate(&mut self, size: usize) -> Result<(), FsError> {
        if size <= self.data.len() {
            self.data.truncate(size);
        } else {
            // Extend with zeros
            self.data.resize(size, 0);
        }
        self.inode.metadata.size = size;
        Ok(())
    }

    /// Delete the file (clear data)
    pub fn delete(&mut self) -> Result<(), FsError> {
        self.data.clear();
        self.inode.metadata.size = 0;
        Ok(())
    }
}