//! Zenith FS - Custom filesystem implementation

#![no_std]

#[cfg(test)]
extern crate std;

#[macro_use]
extern crate alloc;

pub mod directory;
pub mod disk;
pub mod fat32;
pub mod ext4;
pub mod file;
pub mod journal;
pub mod ntfs;
pub mod storage;
pub mod vfs;

use alloc::vec::Vec;

/// Filesystem block size in bytes
pub const BLOCK_SIZE: usize = 512;

/// Maximum filename length
pub const MAX_FILENAME_LEN: usize = 255;

/// File type enumeration
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum FileType {
    RegularFile,
    Directory,
}

/// File metadata structure
#[derive(Debug, Clone)]
pub struct Metadata {
    pub file_type: FileType,
    pub size: usize,
    pub permissions: u16,
    pub created_time: u64,
    pub modified_time: u64,
}

impl Default for Metadata {
    fn default() -> Self {
        Metadata {
            file_type: FileType::RegularFile,
            size: 0,
            permissions: 0o755, // rwxr-xr-x
            created_time: 0,
            modified_time: 0,
        }
    }
}

/// Filesystem error types
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum FsError {
    InvalidPath,
    FileNotFound,
    DirectoryNotEmpty,
    DiskFull,
    PermissionDenied,
    InvalidBlockNumber,
    IoError,
    InvalidMagicNumber,
}

impl core::fmt::Display for FsError {
    fn fmt(&self, f: &mut core::fmt::Formatter) -> core::fmt::Result {
        match self {
            FsError::InvalidPath => write!(f, "Invalid path"),
            FsError::FileNotFound => write!(f, "File not found"),
            FsError::DirectoryNotEmpty => write!(f, "Directory not empty"),
            FsError::DiskFull => write!(f, "Disk full"),
            FsError::PermissionDenied => write!(f, "Permission denied"),
            FsError::InvalidBlockNumber => write!(f, "Invalid block number"),
            FsError::IoError => write!(f, "I/O error"),
            FsError::InvalidMagicNumber => write!(f, "Invalid filesystem magic number"),
        }
    }
}

/// Block allocation information
#[derive(Debug, Clone)]
pub struct BlockInfo {
    pub block_number: usize,
    pub is_free: bool,
}

/// Filesystem superblock structure
#[derive(Debug, Clone)]
pub struct SuperBlock {
    pub magic: u32,
    pub total_blocks: usize,
    pub free_blocks: usize,
    pub root_inode: usize,
    pub block_size: usize,
}

impl Default for SuperBlock {
    fn default() -> Self {
        SuperBlock {
            magic: 0x5A454E49, // "ZENI" in hex
            total_blocks: 0,
            free_blocks: 0,
            root_inode: 0,
            block_size: BLOCK_SIZE,
        }
    }
}

/// Inode structure for filesystem entries
#[derive(Debug, Clone)]
pub struct Inode {
    pub inode_number: usize,
    pub metadata: Metadata,
    pub blocks: Vec<usize>, // Block numbers this file uses
    pub links_count: u32,
}

impl Default for Inode {
    fn default() -> Self {
        Inode {
            inode_number: 0,
            metadata: Metadata::default(),
            blocks: Vec::new(),
            links_count: 1,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::directory::{DirEntry, Directory};
    use super::file::File;
    use super::storage::StorageDevice;
    use super::{FileType, Metadata};

    #[test]
    fn file_write_and_read() {
        let mut file = File::new(Metadata::default());
        assert_eq!(file.write(b"hello").unwrap(), 5);
        let mut buffer = [0u8; 5];
        assert_eq!(file.read(&mut buffer, 0).unwrap(), 5);
        assert_eq!(&buffer, b"hello");
        assert_eq!(file.read(&mut buffer, 5).unwrap(), 0);
    }

    #[test]
    fn directory_rejects_duplicates() {
        let mut dir = Directory::new(Metadata::default());
        dir.add_entry(DirEntry::new("a".into(), 1, FileType::RegularFile)).unwrap();
        assert!(dir.add_entry(DirEntry::new("a".into(), 2, FileType::RegularFile)).is_err());
        assert_eq!(dir.list_entries().len(), 1);
        assert!(dir.remove_entry("a").is_ok());
    }

    #[test]
    fn blocks_are_allocated_once_and_can_be_freed() {
        let mut storage = StorageDevice::new(super::BLOCK_SIZE * 4);
        let first = storage.allocate_block().unwrap();
        let second = storage.allocate_block().unwrap();
        assert_ne!(first, second);
        assert!(storage.is_block_used(first));
        assert_eq!(storage.deallocate_block(first), Ok(()));
        assert!(!storage.is_block_used(first));
        assert!(storage.deallocate_block(first).is_err());
    }
}