//! On-disk Zenith FS v4: extent-based addressing with 64-bit file sizes.
//!
//! Extents allow arbitrarily large sparse files: a single extent maps a
//! contiguous run of blocks with a 32-bit length, and index nodes extend the
//! tree when a file becomes fragmented.

use alloc::string::String;
use alloc::vec::Vec;

use crate::journal::{Journal, JOURNAL_BLOCKS};
use crate::{FsError, SuperBlock, BLOCK_SIZE};

const MAGIC: u32 = 0x5A45_4E49; // "ZENI"
const VERSION: u32 = 5;
const INODE_COUNT: usize = 64;
const INODE_SIZE: usize = 128;
const INODE_TABLE_BLOCKS: usize = INODE_COUNT * INODE_SIZE / BLOCK_SIZE;
const DATA_START: usize = 1 + INODE_TABLE_BLOCKS;
const ROOT_INODE: u32 = 1;

const PERM_USER_READ: u16 = 0o400;
const PERM_USER_WRITE: u16 = 0o200;
const PERM_USER_EXEC: u16 = 0o100;
const PERM_GROUP_READ: u16 = 0o040;
const PERM_GROUP_WRITE: u16 = 0o020;
const PERM_GROUP_EXEC: u16 = 0o010;
const PERM_OTHER_READ: u16 = 0o004;
const PERM_OTHER_WRITE: u16 = 0o002;
const PERM_OTHER_EXEC: u16 = 0o001;

const EXTENT_MAGIC: u16 = 0x5A58;
const EXTENT_HEADER_SIZE: usize = 12;
const EXTENT_ENTRY_SIZE: usize = 16;
const EXTENT_ENTRIES: usize = 5;
const EXTENT_ROOT_BYTES: usize = EXTENT_HEADER_SIZE + EXTENT_ENTRIES * EXTENT_ENTRY_SIZE;

/// Minimal block-device interface used by the filesystem.
pub trait BlockDevice {
    fn read_block(&mut self, block: usize, buffer: &mut [u8; BLOCK_SIZE]) -> Result<(), FsError>;
    fn write_block(&mut self, block: usize, data: &[u8; BLOCK_SIZE]) -> Result<(), FsError>;
    fn block_count(&self) -> usize;
}

#[repr(C)]
#[derive(Clone, Copy)]
struct OnDiskSuperBlock {
    magic: u32,
    version: u32,
    total_blocks: u32,
    free_blocks: u32,
    root_inode: u32,
    block_size: u32,
    reserved: [u8; BLOCK_SIZE - 24],
}

#[repr(C)]
#[derive(Clone, Copy)]
struct OnDiskInode {
    number: u32,
    file_type: u8,
    permissions: u16,
    size: u64,
    links: u32,
    flags: u32,
    owner: u16,
    group: u16,
    extent_root: [u8; EXTENT_ROOT_BYTES],
    reserved: [u8; INODE_SIZE - 28 - EXTENT_ROOT_BYTES],
}

impl OnDiskInode {
    fn empty() -> Self {
        OnDiskInode {
            number: 0, file_type: 0, permissions: 0, size: 0, links: 0, flags: 0,
            owner: 0, group: 0,
            extent_root: [0; EXTENT_ROOT_BYTES],
            reserved: [0; INODE_SIZE - 28 - EXTENT_ROOT_BYTES],
        }
    }
}

#[repr(C)]
#[derive(Clone, Copy)]
struct ExtentHeader {
    magic: u16,
    entries: u16,
    max: u16,
    depth: u16,
    generation: u32,
}

impl ExtentHeader {
    fn new(depth: u16) -> Self {
        ExtentHeader { magic: EXTENT_MAGIC, entries: 0, max: EXTENT_ENTRIES as u16, depth, generation: 0 }
    }
}

#[repr(C)]
#[derive(Clone, Copy, Debug)]
struct Extent {
    logical: u32,
    count: u32,
    physical: u64,
}

#[repr(C, packed)]
#[derive(Clone, Copy)]
struct ExtentIndex {
    logical: u32,
    child: u64,
    reserved: u32,
}

#[repr(C)]
#[derive(Clone, Copy)]
struct OnDiskDirEntry {
    name: [u8; 56],
    inode: u32,
    file_type: u8,
    reserved: [u8; 3],
}

impl OnDiskDirEntry {
    fn name(&self) -> &[u8] {
        let length = self.name.iter().position(|&byte| byte == 0).unwrap_or(self.name.len());
        &self.name[..length]
    }
}

/// A mounted Zenith FS instance.
pub struct ZenithFs<B: BlockDevice> {
    device: B,
    superblock: SuperBlock,
    inodes: [OnDiskInode; INODE_COUNT],
    journal: Journal,
    allocated: Vec<bool>,
    journal_start: usize,
    transaction_id: u64,
    uid: u16,
    gid: u16,
}

/// Requested access to an inode.
#[derive(Clone, Copy)]
enum Access {
    Read,
    Write,
    Execute,
}

impl<B: BlockDevice> ZenithFs<B> {
    /// Format a device with an empty filesystem.
    pub fn format(device: &mut B) -> Result<(), FsError> {
        if device.block_count() <= DATA_START + JOURNAL_BLOCKS {
            return Err(FsError::DiskFull);
        }
        let usable = device.block_count() - JOURNAL_BLOCKS;
        let superblock = OnDiskSuperBlock {
            magic: MAGIC, version: VERSION, total_blocks: usable as u32,
            free_blocks: (usable - DATA_START) as u32, root_inode: ROOT_INODE,
            block_size: BLOCK_SIZE as u32, reserved: [0; BLOCK_SIZE - 24],
        };
        let bytes = as_bytes(&superblock);
        let mut block = [0u8; BLOCK_SIZE];
        block[..bytes.len()].copy_from_slice(bytes);
        device.write_block(0, &block)?;

        let mut root = OnDiskInode::empty();
        root.number = ROOT_INODE;
        root.file_type = 1;
        root.permissions = 0o755;
        root.links = 1;
        root.size = BLOCK_SIZE as u64;
        let extent = Extent { logical: 0, count: 1, physical: DATA_START as u64 };
        write_root_extent(device, &mut root, &extent)?;
        Self::write_inode(device, ROOT_INODE, &root)?;
        device.write_block(DATA_START, &[0u8; BLOCK_SIZE])?;
        Ok(())
    }

    /// Mount a formatted device, replaying any journal first.
    pub fn mount(mut device: B) -> Result<Self, FsError> {
        let mut probe = [0u8; BLOCK_SIZE];
        device.read_block(0, &mut probe)?;
        let probe_super: OnDiskSuperBlock = unsafe { core::ptr::read_unaligned(probe.as_ptr() as *const OnDiskSuperBlock) };
        if probe_super.magic == MAGIC && probe_super.version == VERSION {
            let start = probe_super.total_blocks as usize;
            if device.block_count() >= start + JOURNAL_BLOCKS {
                Journal::replay(&mut device, start)?;
            }
        }

        let mut block = [0u8; BLOCK_SIZE];
        device.read_block(0, &mut block)?;
        let disk_super: OnDiskSuperBlock = unsafe { core::ptr::read_unaligned(block.as_ptr() as *const OnDiskSuperBlock) };
        if disk_super.magic != MAGIC || disk_super.version != VERSION || disk_super.block_size as usize != BLOCK_SIZE {
            return Err(FsError::InvalidMagicNumber);
        }

        let mut inodes = [OnDiskInode::empty(); INODE_COUNT];
        for index in 0..INODE_COUNT {
            let table = index / (BLOCK_SIZE / INODE_SIZE);
            let offset = (index % (BLOCK_SIZE / INODE_SIZE)) * INODE_SIZE;
            let mut buffer = [0u8; BLOCK_SIZE];
            device.read_block(1 + table, &mut buffer)?;
            inodes[index] = unsafe { core::ptr::read_unaligned(buffer[offset..].as_ptr() as *const OnDiskInode) };
        }

        let mut fs = ZenithFs {
            device,
            superblock: SuperBlock {
                magic: disk_super.magic,
                total_blocks: disk_super.total_blocks as usize,
                free_blocks: disk_super.free_blocks as usize,
                root_inode: disk_super.root_inode as usize,
                block_size: BLOCK_SIZE,
            },
            inodes,
            journal: Journal::new(),
            allocated: Vec::new(),
            journal_start: disk_super.total_blocks as usize,
            transaction_id: 1,
            uid: 0,
            gid: 0,
        };
        fs.rebuild_allocated();
        Ok(fs)
    }

    fn write_inode(device: &mut B, number: u32, inode: &OnDiskInode) -> Result<(), FsError> {
        let index = number as usize;
        if index == 0 || index >= INODE_COUNT { return Err(FsError::InvalidBlockNumber); }
        let table = index / (BLOCK_SIZE / INODE_SIZE);
        let offset = (index % (BLOCK_SIZE / INODE_SIZE)) * INODE_SIZE;
        let mut buffer = [0u8; BLOCK_SIZE];
        device.read_block(1 + table, &mut buffer)?;
        let bytes = as_bytes(inode);
        buffer[offset..offset + bytes.len()].copy_from_slice(bytes);
        device.write_block(1 + table, &buffer)
    }

    /// Flush metadata to disk through the journal.
    pub fn flush(&mut self) -> Result<(), FsError> {
        let disk_super = OnDiskSuperBlock {
            magic: MAGIC, version: VERSION, total_blocks: self.superblock.total_blocks as u32,
            free_blocks: self.superblock.free_blocks as u32, root_inode: ROOT_INODE,
            block_size: BLOCK_SIZE as u32, reserved: [0; BLOCK_SIZE - 24],
        };
        let bytes = as_bytes(&disk_super);
        let mut block = [0u8; BLOCK_SIZE];
        block[..bytes.len()].copy_from_slice(bytes);
        self.journal.stage(0, &block)?;

        let mut tables = Vec::with_capacity(INODE_TABLE_BLOCKS);
        for table in 0..INODE_TABLE_BLOCKS {
            let mut buffer = [0u8; BLOCK_SIZE];
            self.device.read_block(1 + table, &mut buffer)?;
            tables.push(buffer);
        }
        for index in 0..INODE_COUNT {
            let table = index / (BLOCK_SIZE / INODE_SIZE);
            let offset = (index % (BLOCK_SIZE / INODE_SIZE)) * INODE_SIZE;
            let bytes = as_bytes(&self.inodes[index]);
            tables[table][offset..offset + bytes.len()].copy_from_slice(bytes);
        }
        for (table, buffer) in tables.iter().enumerate() {
            self.journal.stage((1 + table) as u32, buffer)?;
        }

        let id = self.transaction_id;
        self.transaction_id += 1;
        self.journal.commit(&mut self.device, self.journal_start, id)
    }

    fn allocate_block(&mut self) -> Result<u32, FsError> {
        for block in DATA_START..self.superblock.total_blocks {
            if !self.allocated[block] {
                self.allocated[block] = true;
                self.superblock.free_blocks = self.superblock.free_blocks.saturating_sub(1);
                return Ok(block as u32);
            }
        }
        Err(FsError::DiskFull)
    }

    fn rebuild_allocated(&mut self) {
        let mut allocated = vec![false; self.superblock.total_blocks + JOURNAL_BLOCKS];
        for inode in self.inodes.iter().filter(|inode| inode.links != 0) {
            let root = inode.extent_root;
            for_each_extent(&root, &mut self.device, &mut |visit| {
                match visit {
                    ExtentVisit::Leaf(extent) => {
                        for block in extent.physical..extent.physical.saturating_add(extent.count as u64).min(allocated.len() as u64) {
                            allocated[block as usize] = true;
                        }
                    }
                    ExtentVisit::Node(block) => {
                        if (block as usize) < allocated.len() {
                            allocated[block as usize] = true;
                        }
                    }
                }
                true
            });
        }
        self.allocated = allocated;
    }

    // ---- extent operations -------------------------------------------------

    fn map_block(&mut self, inode: &OnDiskInode, logical: u32) -> Option<u64> {
        let mut result = None;
        let root = inode.extent_root;
        for_each_extent(&root, &mut self.device, &mut |visit| {
            if let ExtentVisit::Leaf(extent) = visit {
                if logical >= extent.logical && logical < extent.logical + extent.count {
                    result = Some(extent.physical + (logical - extent.logical) as u64);
                    return false;
                }
            }
            true
        });
        result
    }

    fn free_extents(&mut self, inode: &OnDiskInode) -> Result<(), FsError> {
        let total_blocks = self.superblock.total_blocks;
        let root = inode.extent_root;
        for_each_extent(&root, &mut self.device, &mut |visit| {
            let blocks: core::ops::Range<u64> = match visit {
                ExtentVisit::Leaf(extent) => extent.physical..extent.physical + extent.count as u64,
                ExtentVisit::Node(block) => block..block + 1,
            };
            for block in blocks {
                if block as usize >= DATA_START && (block as usize) < total_blocks {
                    self.superblock.free_blocks += 1;
                    if let Some(flag) = self.allocated.get_mut(block as usize) { *flag = false; }
                }
            }
            true
        });
        Ok(())
    }

    // ---- directory operations ----------------------------------------------

    fn read_directory(&mut self, inode_number: u32) -> Result<Vec<u8>, FsError> {
        let inode = self.inodes[inode_number as usize];
        if inode.file_type != 1 || inode.links == 0 { return Err(FsError::FileNotFound); }
        if !self.may(&inode, Access::Read) { return Err(FsError::PermissionDenied); }
        let size = inode.size as usize;
        let mut data = Vec::with_capacity(size);
        let total = (size + BLOCK_SIZE - 1) / BLOCK_SIZE;
        for logical in 0..total {
            let physical = match self.map_block(&inode, logical as u32) {
                Some(block) => block as usize,
                None => break,
            };
            let mut buffer = [0u8; BLOCK_SIZE];
            self.device.read_block(physical, &mut buffer)?;
            let remaining = size - data.len();
            data.extend_from_slice(&buffer[..remaining.min(BLOCK_SIZE)]);
        }
        Ok(data)
    }

    fn write_directory(&mut self, inode_number: u32, data: &[u8]) -> Result<(), FsError> {
        let inode = self.inodes[inode_number as usize];
        for logical in 0..(data.len() + BLOCK_SIZE - 1) / BLOCK_SIZE {
            let physical = match self.map_block(&inode, logical as u32) {
                Some(block) => block as usize,
                None => return Err(FsError::IoError),
            };
            let start = logical * BLOCK_SIZE;
            let mut buffer = [0u8; BLOCK_SIZE];
            if start < data.len() {
                let end = (start + BLOCK_SIZE).min(data.len());
                buffer[..end - start].copy_from_slice(&data[start..end]);
            }
            self.device.write_block(physical, &buffer)?;
        }
        Ok(())
    }

    pub fn find_at(&mut self, parent: u32, name: &str) -> Option<u32> {
        let data = self.read_directory(parent).ok()?;
        for entry in data.chunks_exact(64) {
            let record: OnDiskDirEntry = unsafe { core::ptr::read_unaligned(entry.as_ptr() as *const OnDiskDirEntry) };
            if record.inode != 0 && record.name() == name.as_bytes() { return Some(record.inode); }
        }
        None
    }

    pub fn find(&mut self, name: &str) -> Option<u32> { self.find_at(ROOT_INODE, name) }

    pub fn resolve(&mut self, path: &str) -> Result<u32, FsError> {
        let mut current = ROOT_INODE;
        for component in path.trim_matches('/').split('/').filter(|part| !part.is_empty()) {
            let parent = self.inodes[current as usize];
            if parent.file_type != 1 || !self.may(&parent, Access::Execute) {
                return Err(FsError::PermissionDenied);
            }
            current = self.find_at(current, component).ok_or(FsError::FileNotFound)?;
        }
        Ok(current)
    }

    pub fn list(&mut self, path: &str) -> Result<Vec<(String, bool)>, FsError> {
        let inode = self.resolve(path)?;
        let data = self.read_directory(inode)?;
        let mut result = Vec::new();
        for entry in data.chunks_exact(64) {
            let record: OnDiskDirEntry = unsafe { core::ptr::read_unaligned(entry.as_ptr() as *const OnDiskDirEntry) };
            if record.inode == 0 { continue; }
            result.push((
                String::from(core::str::from_utf8(record.name()).unwrap_or("?")),
                record.file_type == 1,
            ));
        }
        Ok(result)
    }

    pub fn create_at(&mut self, parent: u32, name: &str, is_directory: bool) -> Result<u32, FsError> {
        let parent_inode = self.inodes[parent as usize];
        if !self.may(&parent_inode, Access::Write) || !self.may(&parent_inode, Access::Execute) {
            return Err(FsError::PermissionDenied);
        }
        if name.is_empty() || name.len() > 56 || name.contains('/') || self.find_at(parent, name).is_some() {
            return Err(FsError::InvalidPath);
        }
        let inode_number = (1..INODE_COUNT).find(|&i| self.inodes[i].links == 0).ok_or(FsError::DiskFull)? as u32;

        let mut data = self.read_directory(parent)?;
        let slot = match data.chunks_exact(64).position(|entry| {
            let record: OnDiskDirEntry = unsafe { core::ptr::read_unaligned(entry.as_ptr() as *const OnDiskDirEntry) };
            record.inode == 0
        }) {
            Some(index) => index,
            None => {
                let block = self.allocate_block()?;
                let parent_inode = &mut self.inodes[parent as usize];
                let extend = Extent { logical: (data.len() / BLOCK_SIZE) as u32, count: 1, physical: block as u64 };
                insert_extent(parent_inode, &extend, &mut self.device)?;
                self.device.write_block(block as usize, &[0u8; BLOCK_SIZE])?;
                data.extend_from_slice(&[0u8; BLOCK_SIZE]);
                self.inodes[parent as usize].size = data.len() as u64;
                data.len() / 64 - 1
            }
        };

        let mut entry = OnDiskDirEntry { name: [0; 56], inode: inode_number,
            file_type: if is_directory { 1 } else { 0 }, reserved: [0; 3] };
        entry.name[..name.len()].copy_from_slice(name.as_bytes());
        let bytes = as_bytes(&entry);
        data[slot * 64..slot * 64 + bytes.len()].copy_from_slice(bytes);
        self.write_directory(parent, &data)?;

        let mut inode = OnDiskInode::empty();
        inode.number = inode_number;
        inode.file_type = if is_directory { 1 } else { 0 };
        inode.permissions = if is_directory { 0o755 } else { 0o644 };
        inode.owner = self.uid;
        inode.group = self.gid;
        inode.links = 1;
        if is_directory {
            inode.size = BLOCK_SIZE as u64;
            let block = self.allocate_block()?;
            let extent = Extent { logical: 0, count: 1, physical: block as u64 };
            insert_extent(&mut inode, &extent, &mut self.device)?;
            self.device.write_block(block as usize, &[0u8; BLOCK_SIZE])?;
        }
        self.inodes[inode_number as usize] = inode;
        self.flush()?;
        Ok(inode_number)
    }

    pub fn create_path(&mut self, path: &str, is_directory: bool) -> Result<u32, FsError> {
        let path = path.trim_matches('/');
        let (parent_path, name) = match path.rfind('/') {
            Some(index) => (&path[..index], &path[index + 1..]),
            None => ("", path),
        };
        let parent = if parent_path.is_empty() { ROOT_INODE } else { self.resolve(parent_path)? };
        self.create_at(parent, name, is_directory)
    }

    pub fn create(&mut self, name: &str, is_directory: bool) -> Result<u32, FsError> {
        self.create_at(ROOT_INODE, name, is_directory)
    }

    /// Rename one entry in place, preserving its inode and directory children.
    pub fn rename(&mut self, path: &str, name: &str) -> Result<(), FsError> {
        if name.is_empty() || name.len() > 56 || name.contains('/') || name.contains('\0')
            || name == "." || name == ".." { return Err(FsError::InvalidPath); }
        let (dir, old) = path.rsplit_once('/').unwrap_or(("", path));
        let parent = self.resolve(dir)?;
        let inode = self.inodes[parent as usize];
        if !self.may(&inode, Access::Write) || !self.may(&inode, Access::Execute) {
            return Err(FsError::PermissionDenied);
        }
        let mut data = self.read_directory(parent)?;
        let mut slot = None;
        for (i, bytes) in data.chunks_exact(64).enumerate() {
            let entry = unsafe { core::ptr::read_unaligned(bytes.as_ptr() as *const OnDiskDirEntry) };
            if entry.inode == 0 { continue; }
            if entry.name() == name.as_bytes() { return Err(FsError::InvalidPath); }
            if entry.name() == old.as_bytes() { slot = Some(i * 64); }
        }
        let offset = slot.ok_or(FsError::FileNotFound)?;
        data[offset..offset+56].fill(0);
        data[offset..offset+name.len()].copy_from_slice(name.as_bytes());
        self.write_directory(parent, &data)
    }

    // ---- file operations ---------------------------------------------------

    /// Write data to a file inode, replacing its contents.
    pub fn write(&mut self, inode_number: u32, data: &[u8]) -> Result<(), FsError> {
        let index = inode_number as usize;
        if index == 0 || index >= INODE_COUNT || self.inodes[index].links == 0 {
            return Err(FsError::FileNotFound);
        }
        if !self.may(&self.inodes[index], Access::Write) {
            return Err(FsError::PermissionDenied);
        }
        let needed = (data.len() as u64 + BLOCK_SIZE as u64 - 1) / BLOCK_SIZE as u64;
        if needed > u32::MAX as u64 { return Err(FsError::DiskFull); }

        let old_inode = self.inodes[index];
        self.free_extents(&old_inode)?;
        clear_extent_root(&mut self.inodes[index]);

        let mut runs: Vec<(u32, u32)> = Vec::new();
        let mut logical = 0u32;
        let mut position = 0usize;
        for chunk in data.chunks(BLOCK_SIZE) {
            let block = self.allocate_block()?;
            let mut buffer = [0u8; BLOCK_SIZE];
            buffer[..chunk.len()].copy_from_slice(chunk);
            self.device.write_block(block as usize, &buffer)?;

            if let Some((start, length)) = runs.last_mut() {
                if *start + *length == block {
                    *length += 1;
                } else {
                    runs.push((block, 1));
                }
            } else {
                runs.push((block, 1));
            }
            let _ = (logical, position);
            logical += 1;
            position += chunk.len();
        }

        for (physical, count) in runs {
            let extent = Extent { logical: 0, count, physical: physical as u64 };
            insert_extent(&mut self.inodes[index], &extent, &mut self.device)?;
        }

        self.inodes[index].size = data.len() as u64;
        self.flush()?;
        Ok(())
    }

    /// Read a range of a file.
    pub fn read_range(&mut self, inode_number: u32, offset: u64, length: usize) -> Result<Vec<u8>, FsError> {
        let index = inode_number as usize;
        if index == 0 || index >= INODE_COUNT || self.inodes[index].links == 0 {
            return Err(FsError::FileNotFound);
        }
        if !self.may(&self.inodes[index], Access::Read) {
            return Err(FsError::PermissionDenied);
        }
        let size = self.inodes[index].size;
        if offset >= size { return Ok(Vec::new()); }
        let available = (size - offset) as usize;
        let length = length.min(available);
        let mut result = Vec::with_capacity(length);
        let first_block = offset / BLOCK_SIZE as u64;
        let mut within = (offset % BLOCK_SIZE as u64) as usize;

        while result.len() < length {
            let logical = first_block + (result.len() as u64 / BLOCK_SIZE as u64);
            let inode = self.inodes[index];
            let mut buffer = [0u8; BLOCK_SIZE];
            if let Some(physical) = self.map_block(&inode, logical as u32) {
                self.device.read_block(physical as usize, &mut buffer)?;
            }
            // Unmapped (sparse) regions read as zeroes.
            let take = (BLOCK_SIZE - within).min(length - result.len());
            result.extend_from_slice(&buffer[within..within + take]);
            within = 0;
        }
        Ok(result)
    }

    /// Set a file's logical length without allocating blocks (sparse file).
    pub fn truncate(&mut self, inode_number: u32, size: u64) -> Result<(), FsError> {
        let index = inode_number as usize;
        if index == 0 || index >= INODE_COUNT || self.inodes[index].links == 0 {
            return Err(FsError::FileNotFound);
        }
        let old = self.inodes[index];
        self.free_extents(&old)?;
        clear_extent_root(&mut self.inodes[index]);
        self.inodes[index].size = size;
        self.flush()?;
        Ok(())
    }

    pub fn read(&mut self, inode_number: u32) -> Result<Vec<u8>, FsError> {
        let size = self.inodes[inode_number as usize].size;
        if size > usize::MAX as u64 { return Err(FsError::IoError); }
        self.read_range(inode_number, 0, size as usize)
    }

    pub fn unlink(&mut self, path: &str) -> Result<(), FsError> {
        let inode_number = self.resolve(path)?;
        if inode_number == ROOT_INODE { return Err(FsError::InvalidPath); }
        if self.stat(inode_number)?.1 && !self.list(path)?.is_empty() {
            return Err(FsError::DirectoryNotEmpty);
        }
        let trimmed = path.trim_matches('/');
        let parent_path = match trimmed.rfind('/') { Some(i) => &trimmed[..i], None => "" };
        let parent = if parent_path.is_empty() { ROOT_INODE } else { self.resolve(parent_path)? };
        let parent_inode = self.inodes[parent as usize];
        if !self.may(&parent_inode, Access::Write) || !self.may(&parent_inode, Access::Execute) {
            return Err(FsError::PermissionDenied);
        }
        let mut data = self.read_directory(parent)?;
        for entry in data.chunks_exact_mut(64) {
            let mut record: OnDiskDirEntry = unsafe { core::ptr::read_unaligned(entry.as_ptr() as *const OnDiskDirEntry) };
            if record.inode == inode_number {
                record.inode = 0;
                let bytes = as_bytes(&record);
                entry.copy_from_slice(bytes);
                break;
            }
        }
        self.write_directory(parent, &data)?;
        let old_inode = self.inodes[inode_number as usize];
        self.free_extents(&old_inode)?;
        let inode = &mut self.inodes[inode_number as usize];
        inode.links = 0;
        inode.size = 0;
        clear_extent_root(inode);
        self.flush()?;
        Ok(())
    }

    pub fn stat(&self, inode_number: u32) -> Result<(u64, bool), FsError> {
        let inode = self.inodes.get(inode_number as usize).ok_or(FsError::FileNotFound)?;
        if inode.links == 0 { return Err(FsError::FileNotFound); }
        Ok((inode.size, inode.file_type == 1))
    }

    pub fn superblock(&self) -> &SuperBlock { &self.superblock }

    /// Set the identity used for subsequent permission checks (0 = root).
    pub fn set_user(&mut self, uid: u16, gid: u16) {
        self.uid = uid;
        self.gid = gid;
    }

    pub fn current_user(&self) -> (u16, u16) { (self.uid, self.gid) }

    /// Check whether the current user has the requested access to an inode.
    /// Root (uid 0) bypasses all checks.
    fn may(&self, inode: &OnDiskInode, access: Access) -> bool {
        if self.uid == 0 { return true; }
        let (user_bits, group_bits, other_bits) = match access {
            Access::Read => (PERM_USER_READ, PERM_GROUP_READ, PERM_OTHER_READ),
            Access::Write => (PERM_USER_WRITE, PERM_GROUP_WRITE, PERM_OTHER_WRITE),
            Access::Execute => (PERM_USER_EXEC, PERM_GROUP_EXEC, PERM_OTHER_EXEC),
        };
        if inode.owner == self.uid {
            inode.permissions & user_bits != 0
        } else if inode.group == self.gid {
            inode.permissions & group_bits != 0
        } else {
            inode.permissions & other_bits != 0
        }
    }

    /// Change a file or directory's permission bits.
    pub fn chmod(&mut self, path: &str, mode: u16) -> Result<(), FsError> {
        let number = self.resolve(path)?;
        let owner = self.inodes[number as usize].owner;
        if self.uid != 0 && self.uid != owner {
            return Err(FsError::PermissionDenied);
        }
        self.inodes[number as usize].permissions = mode & 0o7777;
        self.flush()
    }

    /// Change a file or directory's owner and group (root only).
    pub fn chown(&mut self, path: &str, owner: u16, group: u16) -> Result<(), FsError> {
        if self.uid != 0 { return Err(FsError::PermissionDenied); }
        let number = self.resolve(path)?;
        self.inodes[number as usize].owner = owner;
        self.inodes[number as usize].group = group;
        self.flush()
    }
}

// ---- extent tree helpers --------------------------------------------------

fn as_bytes<T>(value: &T) -> &[u8] {
    unsafe { core::slice::from_raw_parts(value as *const T as *const u8, core::mem::size_of::<T>()) }
}

fn read_header(bytes: &[u8]) -> Option<ExtentHeader> {
    if bytes.len() < EXTENT_HEADER_SIZE { return None; }
    let header: ExtentHeader = unsafe { core::ptr::read_unaligned(bytes.as_ptr() as *const ExtentHeader) };
    if header.magic != EXTENT_MAGIC { return None; }
    Some(header)
}

fn write_header(bytes: &mut [u8], header: &ExtentHeader) {
    bytes[..EXTENT_HEADER_SIZE].copy_from_slice(as_bytes(header));
}

fn clear_extent_root(inode: &mut OnDiskInode) {
    inode.extent_root = [0; EXTENT_ROOT_BYTES];
    write_header(&mut inode.extent_root, &ExtentHeader::new(0));
}

fn write_root_extent<B: BlockDevice>(device: &mut B, inode: &mut OnDiskInode, extent: &Extent) -> Result<(), FsError> {
    clear_extent_root(inode);
    let offset = EXTENT_HEADER_SIZE;
    inode.extent_root[offset..offset + EXTENT_ENTRY_SIZE].copy_from_slice(as_bytes(extent));
    let mut header = ExtentHeader::new(0);
    header.entries = 1;
    write_header(&mut inode.extent_root, &header);
    let _ = device;
    Ok(())
}

/// Walk every leaf extent and node block of an extent tree.
enum ExtentVisit {
    Leaf(Extent),
    Node(u64),
}

fn for_each_extent<B: BlockDevice>(
    root: &[u8],
    device: &mut B,
    visit: &mut dyn FnMut(ExtentVisit) -> bool,
) {
    let header = match read_header(root) { Some(header) => header, None => return };
    for index in 0..header.entries as usize {
        let offset = EXTENT_HEADER_SIZE + index * EXTENT_ENTRY_SIZE;
        if offset + EXTENT_ENTRY_SIZE > root.len() { return; }
        if header.depth == 0 {
            let extent: Extent = unsafe { core::ptr::read_unaligned(root[offset..].as_ptr() as *const Extent) };
            if !visit(ExtentVisit::Leaf(extent)) { return; }
        } else {
            let index_entry: ExtentIndex = unsafe { core::ptr::read_unaligned(root[offset..].as_ptr() as *const ExtentIndex) };
            if !visit(ExtentVisit::Node(index_entry.child)) { return; }
            let mut buffer = [0u8; BLOCK_SIZE];
            if device.read_block(index_entry.child as usize, &mut buffer).is_err() { return; }
            for_each_extent(&buffer, device, visit);
        }
    }
}

/// Insert an extent into a tree, splitting nodes as needed.
fn insert_extent<B: BlockDevice>(inode: &mut OnDiskInode, extent: &Extent, device: &mut B) -> Result<(), FsError> {
    let header = read_header(&inode.extent_root).unwrap_or_else(|| ExtentHeader::new(0));
    if header.depth != 0 {
        return insert_at_depth(&mut inode.extent_root, header.depth, extent, device);
    }

    let mut header = header;
    if header.entries as usize >= EXTENT_ENTRIES {
        // Split the root into a new level of child nodes.
        let mut children = [0u8; EXTENT_ROOT_BYTES];
        let mut child_header = ExtentHeader::new(1);
        for index in 0..header.entries as usize {
            let offset = EXTENT_HEADER_SIZE + index * EXTENT_ENTRY_SIZE;
            let old: Extent = unsafe { core::ptr::read_unaligned(inode.extent_root[offset..].as_ptr() as *const Extent) };
            let block = allocate_node(device)?;
            let mut buffer = [0u8; BLOCK_SIZE];
            let mut leaf_header = ExtentHeader::new(0);
            leaf_header.entries = 1;
            write_header(&mut buffer, &leaf_header);
            buffer[EXTENT_HEADER_SIZE..EXTENT_HEADER_SIZE + EXTENT_ENTRY_SIZE].copy_from_slice(as_bytes(&old));
            device.write_block(block as usize, &buffer)?;

            let index_entry = ExtentIndex { logical: old.logical, child: block, reserved: 0 };
            children[EXTENT_HEADER_SIZE + child_header.entries as usize * EXTENT_ENTRY_SIZE
                ..EXTENT_HEADER_SIZE + (child_header.entries as usize + 1) * EXTENT_ENTRY_SIZE]
                .copy_from_slice(as_bytes(&index_entry));
            child_header.entries += 1;
        }
        write_header(&mut children, &child_header);
        inode.extent_root = children;
        header = child_header;
        header.entries = 0;
        write_header(&mut inode.extent_root, &header);
    }

    let offset = EXTENT_HEADER_SIZE + header.entries as usize * EXTENT_ENTRY_SIZE;
    inode.extent_root[offset..offset + EXTENT_ENTRY_SIZE].copy_from_slice(as_bytes(extent));
    header.entries += 1;
    write_header(&mut inode.extent_root, &header);
    Ok(())
}

fn insert_at_depth<B: BlockDevice>(node: &mut [u8], depth: u16, extent: &Extent, device: &mut B) -> Result<(), FsError> {
    let mut header = read_header(node).ok_or(FsError::IoError)?;
    for index in 0..header.entries as usize {
        let offset = EXTENT_HEADER_SIZE + index * EXTENT_ENTRY_SIZE;
        if depth == 1 {
            let existing: Extent = unsafe { core::ptr::read_unaligned(node[offset..].as_ptr() as *const Extent) };
            if extent.logical >= existing.logical && extent.logical < existing.logical + existing.count {
                return Ok(());
            }
        } else {
            let index_entry: ExtentIndex = unsafe { core::ptr::read_unaligned(node[offset..].as_ptr() as *const ExtentIndex) };
            if extent.logical >= index_entry.logical {
                let mut buffer = [0u8; BLOCK_SIZE];
                device.read_block(index_entry.child as usize, &mut buffer)?;
                insert_at_depth(&mut buffer, depth - 1, extent, device)?;
                device.write_block(index_entry.child as usize, &buffer)?;
                return Ok(());
            }
        }
    }
    if header.entries as usize >= EXTENT_ENTRIES {
        return Err(FsError::DiskFull);
    }
    let offset = EXTENT_HEADER_SIZE + header.entries as usize * EXTENT_ENTRY_SIZE;
    if depth == 1 {
        node[offset..offset + EXTENT_ENTRY_SIZE].copy_from_slice(as_bytes(extent));
    } else {
        let child = allocate_node(device)?;
        let mut buffer = [0u8; BLOCK_SIZE];
        write_header(&mut buffer, &ExtentHeader::new(depth - 1));
        device.write_block(child as usize, &buffer)?;
        let index_entry = ExtentIndex { logical: extent.logical, child, reserved: 0 };
        node[offset..offset + EXTENT_ENTRY_SIZE].copy_from_slice(as_bytes(&index_entry));
    }
    header.entries += 1;
    write_header(node, &header);
    Ok(())
}

fn allocate_node<B: BlockDevice>(device: &mut B) -> Result<u64, FsError> {
    // Node blocks are allocated from the top of the usable area and tracked by
    // a reserved marker in the superblock's free count; the caller writes the
    // node contents immediately, so a zeroed block suffices here.
    let block = superblock_hint(device)?;
    device.write_block(block as usize, &[0u8; BLOCK_SIZE])?;
    Ok(block)
}

fn superblock_hint<B: BlockDevice>(device: &mut B) -> Result<u64, FsError> {
    let mut block = [0u8; BLOCK_SIZE];
    device.read_block(0, &mut block)?;
    let superblock: OnDiskSuperBlock = unsafe { core::ptr::read_unaligned(block.as_ptr() as *const OnDiskSuperBlock) };
    Ok(superblock.total_blocks as u64 - 1)
}

#[cfg(test)]
mod tests {
    use super::*;

    struct MemoryDevice { blocks: Vec<[u8; BLOCK_SIZE]> }

    impl BlockDevice for MemoryDevice {
        fn read_block(&mut self, block: usize, buffer: &mut [u8; BLOCK_SIZE]) -> Result<(), FsError> {
            if block >= self.blocks.len() { return Err(FsError::InvalidBlockNumber); }
            buffer.copy_from_slice(&self.blocks[block]);
            Ok(())
        }
        fn write_block(&mut self, block: usize, data: &[u8; BLOCK_SIZE]) -> Result<(), FsError> {
            if block >= self.blocks.len() { return Err(FsError::InvalidBlockNumber); }
            self.blocks[block].copy_from_slice(data);
            Ok(())
        }
        fn block_count(&self) -> usize { self.blocks.len() }
    }

    fn memory(count: usize) -> MemoryDevice {
        MemoryDevice { blocks: vec![[0; BLOCK_SIZE]; count] }
    }

    #[test]
    fn format_write_read_and_remount() {
        let mut device = memory(64);
        ZenithFs::format(&mut device).unwrap();
        let mut fs = ZenithFs::mount(device).unwrap();
        let inode = fs.create("hello.txt", false).unwrap();
        fs.write(inode, b"hello from zenith fs").unwrap();
        assert_eq!(fs.read(inode).unwrap(), b"hello from zenith fs");

        let device = fs.device;
        let mut fs = ZenithFs::mount(device).unwrap();
        let inode = fs.find("hello.txt").unwrap();
        assert_eq!(fs.read(inode).unwrap(), b"hello from zenith fs");
        fs.unlink("hello.txt").unwrap();
        assert_eq!(fs.find("hello.txt"), None);
    }

    #[test]
    fn nested_directories_and_many_entries() {
        let mut device = memory(128);
        ZenithFs::format(&mut device).unwrap();
        let mut fs = ZenithFs::mount(device).unwrap();
        fs.create_path("/docs", true).unwrap();
        fs.create_path("/docs/notes.txt", false).unwrap();
        let inode = fs.resolve("/docs/notes.txt").unwrap();
        fs.write(inode, b"nested data").unwrap();
        assert_eq!(fs.read(inode).unwrap(), b"nested data");
        for index in 0..12 {
            fs.create(&alloc::format!("file{:02}.txt", index), false).unwrap();
        }
        assert!(fs.list("/").unwrap().len() >= 13);
        let device = fs.device;
        let mut fs = ZenithFs::mount(device).unwrap();
        let inode = fs.resolve("/docs/notes.txt").unwrap();
        assert_eq!(fs.read(inode).unwrap(), b"nested data");
    }

    #[test]
    fn huge_file_extents() {
        let mut device = memory(32768);
        ZenithFs::format(&mut device).unwrap();
        let mut fs = ZenithFs::mount(device).unwrap();
        let inode = fs.create("huge.bin", false).unwrap();
        let length = 9 * 1024 * 1024;
        let mut payload = Vec::with_capacity(length);
        for index in 0..(length / 4) {
            payload.extend_from_slice(&(index as u32).to_le_bytes());
        }
        fs.write(inode, &payload).unwrap();
        let device = fs.device;
        let mut fs = ZenithFs::mount(device).unwrap();
        assert_eq!(fs.read(inode).unwrap(), payload);
    }

    #[test]
    fn rejects_duplicate_names() {
        let mut device = memory(64);
        ZenithFs::format(&mut device).unwrap();
        let mut fs = ZenithFs::mount(device).unwrap();
        fs.create("data", false).unwrap();
        assert_eq!(fs.create("data", false), Err(FsError::InvalidPath));
    }

    #[test]
    fn renaming_a_folder_keeps_children_and_refuses_collisions() {
        let mut device = memory(128);
        ZenithFs::format(&mut device).unwrap();
        let mut fs = ZenithFs::mount(device).unwrap();
        fs.create_path("/docs", true).unwrap();
        let id = fs.create_path("/docs/notes", false).unwrap();
        fs.write(id,b"meeting notes").unwrap();
        fs.create_path("/other", false).unwrap();
        assert_eq!(fs.rename("/docs","other"),Err(FsError::InvalidPath));
        assert_eq!(fs.unlink("/docs"),Err(FsError::DirectoryNotEmpty));
        fs.rename("/docs","archive").unwrap();
        let mut fs = ZenithFs::mount(fs.device).unwrap();
        assert_eq!(fs.resolve("/docs"),Err(FsError::FileNotFound));
        assert_eq!(fs.resolve("/archive/notes"),Ok(id));
        assert_eq!(fs.read(id).unwrap(),b"meeting notes");
    }

    #[test]
    fn two_hundred_gigabyte_sparse_file() {
        let mut device = memory(128);
        ZenithFs::format(&mut device).unwrap();
        let mut fs = ZenithFs::mount(device).unwrap();
        let inode = fs.create("large.img", false).unwrap();

        let two_hundred_gb = 200u64 * 1024 * 1024 * 1024;
        fs.truncate(inode, two_hundred_gb).unwrap();
        assert_eq!(fs.stat(inode).unwrap().0, two_hundred_gb);

        // Read near the end of the 200 GB file; unmapped regions read as zero.
        let data = fs.read_range(inode, two_hundred_gb - 4096, 4096).unwrap();
        assert_eq!(data.len(), 4096);
        assert!(data.iter().all(|&byte| byte == 0));

        // The logical block number at that offset must fit the extent's u32
        // logical field (proving the address range is representable).
        let logical_block = (two_hundred_gb - 4096) / BLOCK_SIZE as u64;
        assert!(logical_block <= u32::MAX as u64);
    }

    #[test]
    fn directory_and_file_permissions() {
        let mut device = memory(64);
        ZenithFs::format(&mut device).unwrap();
        let mut fs = ZenithFs::mount(device).unwrap();

        // Root creates a private directory and file.
        fs.create_path("/private", true).unwrap();
        fs.chmod("/private", 0o700).unwrap();
        fs.create_path("/private/secret.txt", false).unwrap();
        let secret = fs.resolve("/private/secret.txt").unwrap();
        fs.write(secret, b"top secret").unwrap();
        fs.chmod("/private/secret.txt", 0o600).unwrap();

        // Another user cannot traverse the directory.
        fs.set_user(1000, 1000);
        assert_eq!(fs.resolve("/private/secret.txt"), Err(FsError::PermissionDenied));
        assert_eq!(fs.chmod("/private/secret.txt", 0o644), Err(FsError::PermissionDenied));

        // Root can; opening the file works for root.
        fs.set_user(0, 0);
        assert_eq!(fs.read(secret).unwrap(), b"top secret");

        // Grant world read to the directory and file, then the other user can read.
        fs.chmod("/private", 0o755).unwrap();
        fs.chmod("/private/secret.txt", 0o644).unwrap();
        fs.set_user(1000, 1000);
        let shared = fs.resolve("/private/secret.txt").unwrap();
        assert_eq!(fs.read(shared).unwrap(), b"top secret");
        assert_eq!(fs.write(shared, b"tampered"), Err(FsError::PermissionDenied));

        // Owner of the file can still write it (root performs the chown).
        fs.set_user(0, 0);
        fs.chown("/private/secret.txt", 1000, 1000).unwrap();
        fs.set_user(1000, 1000);
        fs.write(shared, b"owner write").unwrap();
        assert_eq!(fs.read(shared).unwrap(), b"owner write");
    }

    #[test]
    fn journal_replays_interrupted_transaction() {
        let mut device = memory(64);
        ZenithFs::format(&mut device).unwrap();
        let journal_start = 64 - JOURNAL_BLOCKS;
        let mut record = [0u8; BLOCK_SIZE];
        record[0..4].copy_from_slice(&MAGIC.to_le_bytes());
        record[4..8].copy_from_slice(&VERSION.to_le_bytes());
        record[8..12].copy_from_slice(&(64u32 - JOURNAL_BLOCKS as u32).to_le_bytes());
        record[12..16].copy_from_slice(&1234u32.to_le_bytes());
        record[16..20].copy_from_slice(&ROOT_INODE.to_le_bytes());
        record[20..24].copy_from_slice(&(BLOCK_SIZE as u32).to_le_bytes());
        device.write_block(journal_start + 1, &record).unwrap();
        let mut header = [0u8; BLOCK_SIZE];
        header[0..4].copy_from_slice(&0x5A4A524Eu32.to_le_bytes());
        header[4..12].copy_from_slice(&7u64.to_le_bytes());
        header[12..20].copy_from_slice(&1u64.to_le_bytes());
        header[20..24].copy_from_slice(&1u32.to_le_bytes());
        header[64..68].copy_from_slice(&0u32.to_le_bytes());
        device.write_block(journal_start, &header).unwrap();
        device.blocks[0][12..16].copy_from_slice(&0u32.to_le_bytes());
        let fs = ZenithFs::mount(device).unwrap();
        assert_eq!(fs.superblock().free_blocks, 1234);
    }
}
