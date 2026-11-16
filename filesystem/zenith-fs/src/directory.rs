//! Directory operations for Zenith FS

use crate::{FileType, FsError, Inode, Metadata};
use alloc::string::String;
use alloc::vec::Vec;

/// Directory entry structure
#[derive(Debug, Clone)]
pub struct DirEntry {
    pub name: String,
    pub inode_number: usize,
    pub file_type: FileType,
}

impl DirEntry {
    pub fn new(name: String, inode_number: usize, file_type: FileType) -> Self {
        DirEntry {
            name,
            inode_number,
            file_type,
        }
    }
}

/// Directory structure for filesystem operations
pub struct Directory {
    pub inode: Inode,
    pub entries: Vec<DirEntry>,
}

impl Directory {
    /// Create a new directory with given metadata
    pub fn new(metadata: Metadata) -> Self {
        Directory {
            inode: Inode {
                metadata,
                ..Default::default()
            },
            entries: Vec::new(),
        }
    }

    /// Add an entry to the directory
    pub fn add_entry(&mut self, entry: DirEntry) -> Result<(), FsError> {
        // Check if entry already exists
        for existing in &self.entries {
            if existing.name == entry.name {
                return Err(FsError::IoError); // This is a simplified error handling approach
            }
        }
        
        self.entries.push(entry);
        Ok(())
    }

    /// Remove an entry from the directory by name
    pub fn remove_entry(&mut self, name: &str) -> Result<DirEntry, FsError> {
        let index = self.entries.iter().position(|entry| entry.name == *name)
            .ok_or(FsError::FileNotFound)?;
        
        Ok(self.entries.remove(index))
    }

    /// Find an entry by name
    pub fn find_entry(&self, name: &str) -> Option<&DirEntry> {
        self.entries.iter().find(|entry| entry.name == *name)
    }

    /// List directory contents
    pub fn list_entries(&self) -> &[DirEntry] {
        &self.entries
    }
}