//! Read-only ext4 driver: superblock, block groups, extent trees and directories.

use alloc::string::String;
use alloc::vec::Vec;

use crate::disk::BlockDevice;
use crate::{FsError, BLOCK_SIZE};

const EXT4_MAGIC: u16 = 0xEF53;
const ROOT_INODE: u32 = 2;
const EXTENT_HEADER_MAGIC: u16 = 0xF30A;
const EXTENTS_FL: u32 = 0x0008_0000;
const INLINE_DATA_FL: u32 = 0x1000_0000;
const S_IFDIR: u16 = 0x4000;
const S_IFREG: u16 = 0x8000;

#[derive(Clone, Copy)]
struct SuperBlock {
    inodes_count: u32,
    blocks_count: u32,
    first_data_block: u32,
    block_size: usize,
    blocks_per_group: u32,
    inodes_per_group: u32,
    inode_size: usize,
    desc_size: usize,
}

#[derive(Clone, Copy)]
struct GroupDescriptor {
    inode_table: u32,
}

struct Inode {
    number: u32,
    raw: Vec<u8>,
}

impl Inode {
    fn mode(&self) -> u16 {
        u16::from_le_bytes([self.raw[0], self.raw[1]])
    }

    fn flags(&self) -> u32 {
        u32::from_le_bytes([self.raw[32], self.raw[33], self.raw[34], self.raw[35]])
    }

    fn size(&self) -> u64 {
        let low = u32::from_le_bytes([self.raw[4], self.raw[5], self.raw[6], self.raw[7]]);
        let high = if self.raw.len() >= 112 {
            u32::from_le_bytes([self.raw[108], self.raw[109], self.raw[110], self.raw[111]])
        } else {
            0
        };
        ((high as u64) << 32) | low as u64
    }

    fn block(&self, index: usize) -> u32 {
        u32::from_le_bytes([
            self.raw[40 + index * 4],
            self.raw[41 + index * 4],
            self.raw[42 + index * 4],
            self.raw[43 + index * 4],
        ])
    }
}

/// A file discovered in an ext4 directory.
pub struct Ext4File {
    pub inode: u32,
    pub name: String,
    pub is_directory: bool,
    pub size: u64,
}

/// A mounted ext4 filesystem.
pub struct Ext4<B: BlockDevice> {
    device: B,
    superblock: SuperBlock,
    groups: Vec<GroupDescriptor>,
}

impl<B: BlockDevice> Ext4<B> {
    /// Parse the superblock and group descriptor table.
    pub fn mount(mut device: B) -> Result<Self, FsError> {
        let mut sector = [0u8; BLOCK_SIZE];
        // The ext4 superblock always begins at byte 1024, i.e. sector 2 of
        // our 512-byte block device.
        device.read_block(2, &mut sector)?;
        if u16::from_le_bytes([sector[56], sector[57]]) != EXT4_MAGIC {
            return Err(FsError::InvalidMagicNumber);
        }

        let log_block_size = u32::from_le_bytes([sector[24], sector[25], sector[26], sector[27]]);
        if log_block_size > 2 {
            return Err(FsError::InvalidMagicNumber);
        }
        let block_size = 1024usize << log_block_size;

        let superblock = SuperBlock {
            inodes_count: u32::from_le_bytes([sector[0], sector[1], sector[2], sector[3]]),
            blocks_count: u32::from_le_bytes([sector[4], sector[5], sector[6], sector[7]]),
            first_data_block: u32::from_le_bytes([sector[20], sector[21], sector[22], sector[23]]),
            block_size,
            blocks_per_group: u32::from_le_bytes([sector[32], sector[33], sector[34], sector[35]]),
            inodes_per_group: u32::from_le_bytes([sector[40], sector[41], sector[42], sector[43]]),
            inode_size: u16::from_le_bytes([sector[88], sector[89]]) as usize,
            desc_size: u16::from_le_bytes([sector[254], sector[255]]) as usize,
        };

        if superblock.inode_size < 128 || superblock.inodes_per_group == 0 {
            return Err(FsError::InvalidMagicNumber);
        }
        let desc_size = if superblock.desc_size >= 32 { superblock.desc_size } else { 32 };

        let group_count = (superblock.blocks_count - superblock.first_data_block
            + superblock.blocks_per_group - 1) / superblock.blocks_per_group;
        let descriptor_start = if block_size == 1024 {
            superblock.first_data_block + 1
        } else {
            1
        };

        let mut groups = Vec::with_capacity(group_count as usize);
        let descriptor_block = Self::read_block(&mut device, block_size, descriptor_start as usize)?;
        for index in 0..group_count as usize {
            let offset = index * desc_size;
            if offset + 8 > descriptor_block.len() {
                return Err(FsError::IoError);
            }
            groups.push(GroupDescriptor {
                inode_table: u32::from_le_bytes([
                    descriptor_block[offset + 8],
                    descriptor_block[offset + 9],
                    descriptor_block[offset + 10],
                    descriptor_block[offset + 11],
                ]),
            });
        }

        Ok(Ext4 { device, superblock, groups })
    }

    fn read_block(device: &mut B, block_size: usize, block: usize) -> Result<Vec<u8>, FsError> {
        let mut data = Vec::with_capacity(block_size);
        for offset in 0..block_size / BLOCK_SIZE {
            let mut sector = [0u8; BLOCK_SIZE];
            device.read_block(block * (block_size / BLOCK_SIZE) + offset, &mut sector)?;
            data.extend_from_slice(&sector);
        }
        Ok(data)
    }

    fn read_inode(&mut self, number: u32) -> Result<Inode, FsError> {
        if number == 0 || number > self.superblock.inodes_count {
            return Err(FsError::FileNotFound);
        }
        let index = number - 1;
        let group = (index / self.superblock.inodes_per_group) as usize;
        let offset = (index % self.superblock.inodes_per_group) as usize;
        let group = self.groups.get(group).ok_or(FsError::IoError)?;
        let inode_table = group.inode_table as usize;
        let block_offset = offset * self.superblock.inode_size;
        let block = block_offset / self.superblock.block_size;
        let within = block_offset % self.superblock.block_size;
        let data = Self::read_block(&mut self.device, self.superblock.block_size, inode_table + block)?;
        if within + self.superblock.inode_size > data.len() {
            return Err(FsError::IoError);
        }
        Ok(Inode {
            number,
            raw: data[within..within + self.superblock.inode_size].to_vec(),
        })
    }

    /// Map a logical block of an inode to a physical filesystem block.
    fn map_block(&mut self, inode: &Inode, logical: u32) -> Result<Option<u32>, FsError> {
        if inode.flags() & EXTENTS_FL == 0 {
            if logical < 12 {
                return Ok(Some(inode.block(logical as usize)));
            }
            return Ok(None); // indirect blocks not implemented
        }
        if inode.flags() & INLINE_DATA_FL != 0 {
            return Ok(None);
        }

        let header_offset = 40usize;
        self.map_extent(inode.number, header_offset, logical, 0)
    }

    fn map_extent(
        &mut self,
        inode_number: u32,
        header_offset: usize,
        logical: u32,
        depth: u16,
    ) -> Result<Option<u32>, FsError> {
        let inode = self.read_inode(inode_number)?;
        if header_offset + 12 > inode.raw.len() {
            return Ok(None);
        }
        let header = &inode.raw[header_offset..header_offset + 12];
        if u16::from_le_bytes([header[0], header[1]]) != EXTENT_HEADER_MAGIC {
            return Ok(None);
        }
        let entries = u16::from_le_bytes([header[2], header[3]]) as usize;
        let node_depth = u16::from_le_bytes([header[6], header[7]]);

        for index in 0..entries {
            let offset = header_offset + 12 + index * 12;
            let entry = self.extent_entry(inode_number, offset)?;
            if depth == 0 {
                if logical >= entry.0 && logical < entry.0 + entry.1 as u32 {
                    let physical = entry.2 + (logical - entry.0) as u64;
                    return Ok(Some(physical as u32));
                }
            } else {
                if logical >= entry.0 {
                    let child_block = entry.2 as usize;
                    let child = Self::read_block(&mut self.device, self.superblock.block_size, child_block)?;
                    // Recurse using a synthetic inode whose raw bytes are the
                    // child extent node, so the same parser can be reused.
                    let synthetic = Inode { number: inode_number, raw: child };
                    if let Some(result) = self.map_extent_node(&synthetic, 0, logical, depth - 1)? {
                        return Ok(Some(result));
                    }
                }
            }
        }
        let _ = node_depth;
        Ok(None)
    }

    fn map_extent_node(
        &mut self,
        node: &Inode,
        header_offset: usize,
        logical: u32,
        depth: u16,
    ) -> Result<Option<u32>, FsError> {
        let header = &node.raw[header_offset..header_offset + 12];
        if u16::from_le_bytes([header[0], header[1]]) != EXTENT_HEADER_MAGIC {
            return Ok(None);
        }
        let entries = u16::from_le_bytes([header[2], header[3]]) as usize;

        for index in 0..entries {
            let offset = header_offset + 12 + index * 12;
            let entry = &node.raw[offset..offset + 12];
            let first_block = u32::from_le_bytes([entry[0], entry[1], entry[2], entry[3]]);
            if depth == 0 {
                let count = u16::from_le_bytes([entry[4], entry[5]]) as u32;
                if logical >= first_block && logical < first_block + count {
                    let low = u32::from_le_bytes([entry[8], entry[9], entry[10], entry[11]]);
                    let high = u16::from_le_bytes([entry[6], entry[7]]) as u64;
                    return Ok(Some((((high << 32) | low as u64) + (logical - first_block) as u64) as u32));
                }
            } else {
                if logical >= first_block {
                    let low = u32::from_le_bytes([entry[4], entry[5], entry[6], entry[7]]);
                    let high = u16::from_le_bytes([entry[8], entry[9]]) as u64;
                    let child_block = ((high << 32) | low as u64) as usize;
                    let child = Self::read_block(&mut self.device, self.superblock.block_size, child_block)?;
                    let synthetic = Inode { number: node.number, raw: child };
                    if let Some(result) = self.map_extent_node(&synthetic, 0, logical, depth - 1)? {
                        return Ok(Some(result));
                    }
                }
            }
        }
        Ok(None)
    }

    /// Read an extent entry from an inode (returns first block, count, physical).
    fn extent_entry(&mut self, inode_number: u32, offset: usize) -> Result<(u32, u16, u64), FsError> {
        let inode = self.read_inode(inode_number)?;
        let entry = &inode.raw[offset..offset + 12];
        let first = u32::from_le_bytes([entry[0], entry[1], entry[2], entry[3]]);
        let count = u16::from_le_bytes([entry[4], entry[5]]);
        let low = u32::from_le_bytes([entry[8], entry[9], entry[10], entry[11]]);
        let high = u16::from_le_bytes([entry[6], entry[7]]) as u64;
        Ok((first, count, (high << 32) | low as u64))
    }

    /// Read a directory's entries.
    fn read_dir(&mut self, inode_number: u32) -> Result<Vec<Ext4File>, FsError> {
        let inode = self.read_inode(inode_number)?;
        if inode.mode() & S_IFDIR == 0 {
            return Err(FsError::InvalidPath);
        }
        let size = inode.size() as usize;
        let mut entries = Vec::new();
        let mut logical = 0u32;
        let mut remaining = size;

        while remaining > 0 {
            let physical = match self.map_block(&inode, logical)? {
                Some(block) => block as usize,
                None => break,
            };
            let block = Self::read_block(&mut self.device, self.superblock.block_size, physical)?;
            let mut offset = 0usize;
            while offset + 8 <= block.len() {
                let child_inode = u32::from_le_bytes([
                    block[offset], block[offset + 1], block[offset + 2], block[offset + 3],
                ]);
                let record_length = u16::from_le_bytes([block[offset + 4], block[offset + 5]]) as usize;
                if record_length < 8 || offset + record_length > block.len() {
                    break;
                }
                let name_length = block[offset + 6] as usize;
                let file_type = block[offset + 7];
                if child_inode != 0 && name_length > 0 && offset + 8 + name_length <= offset + record_length {
                    let name = String::from(core::str::from_utf8(
                        &block[offset + 8..offset + 8 + name_length],
                    ).unwrap_or("?"));
                    entries.push(Ext4File {
                        inode: child_inode,
                        name,
                        is_directory: file_type == 2,
                        size: self.read_inode(child_inode).map(|i| i.size()).unwrap_or(0),
                    });
                }
                offset += record_length;
            }
            remaining = remaining.saturating_sub(self.superblock.block_size);
            logical += 1;
        }
        Ok(entries)
    }

    /// Resolve a path to an inode number.
    pub fn resolve(&mut self, path: &str) -> Result<u32, FsError> {
        let mut current = ROOT_INODE;
        for component in path.trim_matches('/').split('/').filter(|part| !part.is_empty()) {
            let entries = self.read_dir(current)?;
            current = entries.into_iter()
                .find(|entry| entry.name == component)
                .ok_or(FsError::FileNotFound)?
                .inode;
        }
        Ok(current)
    }

    /// Look up a file by full path.
    pub fn open(&mut self, path: &str) -> Result<Ext4File, FsError> {
        let number = self.resolve(path)?;
        let inode = self.read_inode(number)?;
        let name = path.trim_matches('/').rsplit('/').next().unwrap_or("");
        Ok(Ext4File {
            inode: number,
            name: String::from(name),
            is_directory: inode.mode() & S_IFDIR != 0,
            size: inode.size(),
        })
    }

    /// Read a file's contents.
    pub fn read_file(&mut self, file: &Ext4File) -> Result<Vec<u8>, FsError> {
        let inode = self.read_inode(file.inode)?;
        if inode.mode() & S_IFREG == 0 {
            return Err(FsError::InvalidPath);
        }
        let size = inode.size() as usize;
        let mut result = Vec::with_capacity(size);
        let mut logical = 0u32;

        while result.len() < size {
            let physical = match self.map_block(&inode, logical)? {
                Some(block) => block as usize,
                None => break,
            };
            let block = Self::read_block(&mut self.device, self.superblock.block_size, physical)?;
            let remaining = size - result.len();
            result.extend_from_slice(&block[..remaining.min(block.len())]);
            logical += 1;
        }
        Ok(result)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::disk::BlockDevice;

    struct FileDevice {
        blocks: Vec<[u8; BLOCK_SIZE]>,
    }

    impl BlockDevice for FileDevice {
        fn read_block(&mut self, block: usize, buffer: &mut [u8; BLOCK_SIZE]) -> Result<(), FsError> {
            if block >= self.blocks.len() {
                return Err(FsError::InvalidBlockNumber);
            }
            buffer.copy_from_slice(&self.blocks[block]);
            Ok(())
        }

        fn write_block(&mut self, block: usize, data: &[u8; BLOCK_SIZE]) -> Result<(), FsError> {
            if block >= self.blocks.len() {
                return Err(FsError::InvalidBlockNumber);
            }
            self.blocks[block].copy_from_slice(data);
            Ok(())
        }

        fn block_count(&self) -> usize {
            self.blocks.len()
        }
    }

    /// Validate against a real mkfs.ext4 image when the tool is available.
    #[test]
    fn reads_real_ext4_image() {
        use std::eprintln;
        use std::process::Command;

        if Command::new("mkfs.ext4").arg("-V").output().is_err() {
            eprintln!("mkfs.ext4 unavailable; skipping ext4 integration test");
            return;
        }

        let image_path = "/tmp/zenith-ext4-test.img";
        let source_path = "/tmp/zenith-ext4-source.txt";
        std::fs::write(source_path, b"hello from ext4").unwrap();

        let create = Command::new("truncate")
            .args(["-s", "32M", image_path])
            .status().unwrap();
        assert!(create.success());

        let format = Command::new("mkfs.ext4")
            .args(["-q", "-F", "-b", "1024", "-O", "^64bit,^metadata_csum", image_path])
            .status().unwrap();
        assert!(format.success());

        let write = Command::new("debugfs")
            .args(["-w", "-R", &format!("write {} HELLO.TXT", source_path), image_path])
            .output().unwrap();
        assert!(write.status.success(), "debugfs failed: {:?}", write);

        let bytes = std::fs::read(image_path).unwrap();
        let blocks = bytes.chunks(BLOCK_SIZE).map(|chunk| {
            let mut block = [0u8; BLOCK_SIZE];
            block.copy_from_slice(chunk);
            block
        }).collect();

        let mut fs = Ext4::mount(FileDevice { blocks }).unwrap();
        let file = fs.open("/HELLO.TXT").unwrap();
        assert_eq!(fs.read_file(&file).unwrap(), b"hello from ext4");
    }
}
