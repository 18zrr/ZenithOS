//! Minimal VFS layer giving all four filesystems a common interface.

use alloc::string::String;
use alloc::vec::Vec;

use crate::disk::{BlockDevice, ZenithFs};
use crate::ext4::Ext4;
use crate::fat32::Fat32;
use crate::ntfs::Ntfs;
use crate::FsError;

/// Filesystem-independent file description.
pub struct VfsFile {
    pub name: String,
    pub size: u64,
    pub is_directory: bool,
    pub handle: u32,
}

/// Common read-only filesystem operations.
pub trait FileSystem {
    fn name(&self) -> &'static str;
    fn open(&mut self, path: &str) -> Result<VfsFile, FsError>;
    fn read(&mut self, file: &VfsFile) -> Result<Vec<u8>, FsError>;
}

impl<B: BlockDevice> FileSystem for ZenithFs<B> {
    fn name(&self) -> &'static str { "zenith-fs" }

    fn open(&mut self, path: &str) -> Result<VfsFile, FsError> {
        let handle = self.resolve(path)?;
        let (size, is_directory) = self.stat(handle)?;
        Ok(VfsFile {
            name: String::from(path.trim_matches('/').rsplit('/').next().unwrap_or("")),
            size,
            is_directory,
            handle,
        })
    }

    fn read(&mut self, file: &VfsFile) -> Result<Vec<u8>, FsError> {
        self.read(file.handle)
    }
}

impl<B: BlockDevice> FileSystem for Fat32<B> {
    fn name(&self) -> &'static str { "fat32" }

    fn open(&mut self, path: &str) -> Result<VfsFile, FsError> {
        let file = Fat32::open(self, path)?;
        Ok(VfsFile {
            name: file.name,
            size: file.size as u64,
            is_directory: file.is_directory,
            handle: file.first_cluster,
        })
    }

    fn read(&mut self, file: &VfsFile) -> Result<Vec<u8>, FsError> {
        let entry = crate::fat32::FatFile {
            name: file.name.clone(),
            size: file.size as u32,
            first_cluster: file.handle,
            is_directory: file.is_directory,
        };
        Fat32::read_file(self, &entry)
    }
}

impl<B: BlockDevice> FileSystem for Ntfs<B> {
    fn name(&self) -> &'static str { "ntfs" }

    fn open(&mut self, path: &str) -> Result<VfsFile, FsError> {
        let file = Ntfs::open(self, path)?;
        Ok(VfsFile {
            name: file.name,
            size: file.size,
            is_directory: file.is_directory,
            handle: file.reference as u32,
        })
    }

    fn read(&mut self, file: &VfsFile) -> Result<Vec<u8>, FsError> {
        let entry = crate::ntfs::NtfsFile {
            reference: file.handle as u64,
            name: file.name.clone(),
            size: file.size,
            is_directory: file.is_directory,
        };
        Ntfs::read_file(self, &entry)
    }
}

impl<B: BlockDevice> FileSystem for Ext4<B> {
    fn name(&self) -> &'static str { "ext4" }

    fn open(&mut self, path: &str) -> Result<VfsFile, FsError> {
        let file = Ext4::open(self, path)?;
        Ok(VfsFile {
            name: file.name,
            size: file.size,
            is_directory: file.is_directory,
            handle: file.inode,
        })
    }

    fn read(&mut self, file: &VfsFile) -> Result<Vec<u8>, FsError> {
        let entry = crate::ext4::Ext4File {
            inode: file.handle,
            name: file.name.clone(),
            is_directory: file.is_directory,
            size: file.size,
        };
        Ext4::read_file(self, &entry)
    }
}
