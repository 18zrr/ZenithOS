//! FAT32 read-only filesystem driver backed by a block device.

use alloc::string::String;
use alloc::vec::Vec;

use crate::disk::BlockDevice;
use crate::FsError;

const END_OF_CHAIN: u32 = 0x0FFF_FFF8;
const ATTR_DIRECTORY: u8 = 0x10;
const ATTR_LONG_NAME: u8 = 0x0F;

fn trim_spaces(bytes: &[u8]) -> &[u8] {
    let end = bytes.iter().rposition(|&b| b != b' ').map(|i| i + 1).unwrap_or(0);
    &bytes[..end]
}

/// Parsed FAT32 boot sector (BPB).
#[derive(Clone, Copy)]
pub struct Fat32BootSector {
    pub bytes_per_sector: u16,
    pub sectors_per_cluster: u8,
    pub reserved_sectors: u16,
    pub number_of_fats: u8,
    pub total_sectors_32: u32,
    pub fat_size_32: u32,
    pub root_cluster: u32,
}

/// A file extracted from a FAT32 directory.
pub struct FatFile {
    pub name: String,
    pub size: u32,
    pub first_cluster: u32,
    pub is_directory: bool,
}

/// A mounted FAT32 filesystem.
pub struct Fat32<B: BlockDevice> {
    device: B,
    boot: Fat32BootSector,
    fat_start: usize,
    data_start: usize,
}

impl<B: BlockDevice> Fat32<B> {
    /// Mount a FAT32 device by parsing and validating its BPB.
    pub fn mount(mut device: B) -> Result<Self, FsError> {
        let mut sector = [0u8; 512];
        device.read_block(0, &mut sector)?;

        let boot = Fat32BootSector {
            bytes_per_sector: u16::from_le_bytes([sector[11], sector[12]]),
            sectors_per_cluster: sector[13],
            reserved_sectors: u16::from_le_bytes([sector[14], sector[15]]),
            number_of_fats: sector[16],
            total_sectors_32: u32::from_le_bytes([sector[32], sector[33], sector[34], sector[35]]),
            fat_size_32: u32::from_le_bytes([sector[36], sector[37], sector[38], sector[39]]),
            root_cluster: u32::from_le_bytes([sector[44], sector[45], sector[46], sector[47]]),
        };

        if boot.bytes_per_sector != 512 || boot.sectors_per_cluster == 0
            || boot.number_of_fats == 0 || boot.fat_size_32 == 0 || boot.root_cluster < 2
        {
            return Err(FsError::InvalidMagicNumber);
        }

        let fat_start = boot.reserved_sectors as usize;
        let data_start = fat_start + boot.number_of_fats as usize * boot.fat_size_32 as usize;
        if data_start >= boot.total_sectors_32 as usize {
            return Err(FsError::InvalidMagicNumber);
        }

        Ok(Fat32 { device, boot, fat_start, data_start })
    }

    /// Return the boot sector information.
    pub fn boot_sector(&self) -> &Fat32BootSector {
        &self.boot
    }

    fn cluster_sector(&self, cluster: u32) -> usize {
        self.data_start + (cluster as usize - 2) * self.boot.sectors_per_cluster as usize
    }

    fn next_cluster(&mut self, cluster: u32) -> Result<u32, FsError> {
        let offset = self.fat_start * 512 + cluster as usize * 4;
        let fat_sector = offset / 512;
        let within = offset % 512;
        let mut sector = [0u8; 512];
        self.device.read_block(fat_sector, &mut sector)?;
        let value = u32::from_le_bytes([
            sector[within],
            sector[within + 1],
            sector[within + 2],
            sector[within + 3],
        ]) & 0x0FFF_FFFF;
        Ok(value)
    }

    /// Read a whole directory (root or subdirectory) into a list of files.
    pub fn read_directory(&mut self, cluster: u32) -> Result<Vec<FatFile>, FsError> {
        let mut result = Vec::new();
        let mut current = cluster;
        let mut guard = 0usize;

        while current >= 2 && current < END_OF_CHAIN && guard < 4096 {
            guard += 1;
            let cluster_sector = self.cluster_sector(current);
            for sector_offset in 0..self.boot.sectors_per_cluster as usize {
                let mut sector = [0u8; 512];
                self.device.read_block(cluster_sector + sector_offset, &mut sector)?;

                for index in 0..16 {
                    let entry = &sector[index * 32..(index + 1) * 32];
                    if entry[0] == 0x00 {
                        return Ok(result); // end of directory
                    }
                    if entry[0] == 0xE5 || entry[11] == ATTR_LONG_NAME {
                        continue; // deleted or long-name fragment
                    }

                    let is_directory = entry[11] & ATTR_DIRECTORY != 0;
                    let cluster_low = u16::from_le_bytes([entry[20], entry[21]]) as u32;
                    let cluster_high = u16::from_le_bytes([entry[26], entry[27]]) as u32;
                    let first_cluster = (cluster_high << 16) | cluster_low;
                    let size = u32::from_le_bytes([entry[28], entry[29], entry[30], entry[31]]);

                    // Convert the 8.3 short name into a normal "base.ext" form.
                    let base = trim_spaces(&entry[0..8]);
                    let extension = trim_spaces(&entry[8..11]);
                    let mut name = String::from(core::str::from_utf8(base).unwrap_or(""));
                    if !extension.is_empty() {
                        name.push('.');
                        name.push_str(core::str::from_utf8(extension).unwrap_or(""));
                    }

                    result.push(FatFile {
                        name,
                        size,
                        first_cluster,
                        is_directory,
                    });
                }
            }

            current = self.next_cluster(current)?;
        }

        Ok(result)
    }

    /// Look up a file in the root directory.
    pub fn open(&mut self, name: &str) -> Result<FatFile, FsError> {
        let entries = self.read_directory(self.boot.root_cluster)?;
        entries
            .into_iter()
            .find(|entry| entry.name.eq_ignore_ascii_case(name))
            .ok_or(FsError::FileNotFound)
    }

    /// Read a file's entire contents.
    pub fn read_file(&mut self, file: &FatFile) -> Result<Vec<u8>, FsError> {
        let mut result = Vec::with_capacity(file.size as usize);
        let mut current = file.first_cluster;
        let mut guard = 0usize;

        while current >= 2 && current < END_OF_CHAIN && result.len() < file.size as usize
            && guard < 4096
        {
            guard += 1;
            let cluster_sector = self.cluster_sector(current);
            for sector_offset in 0..self.boot.sectors_per_cluster as usize {
                let mut sector = [0u8; 512];
                self.device.read_block(cluster_sector + sector_offset, &mut sector)?;
                let remaining = file.size as usize - result.len();
                result.extend_from_slice(&sector[..remaining.min(512)]);
                if result.len() >= file.size as usize {
                    break;
                }
            }
            current = self.next_cluster(current)?;
        }

        Ok(result)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::disk::BlockDevice;
    use crate::BLOCK_SIZE;

    struct MemoryDevice {
        blocks: Vec<[u8; BLOCK_SIZE]>,
    }

    impl BlockDevice for MemoryDevice {
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

    fn build_image() -> MemoryDevice {
        // 1 reserved sector, FAT at sector 1, data starts at sector 3.
        let mut device = MemoryDevice { blocks: vec![[0; BLOCK_SIZE]; 16] };
        let boot = &mut device.blocks[0];
        boot[11..13].copy_from_slice(&512u16.to_le_bytes());
        boot[13] = 1;
        boot[14..16].copy_from_slice(&1u16.to_le_bytes());
        boot[16] = 1;
        boot[32..36].copy_from_slice(&16u32.to_le_bytes());
        boot[36..40].copy_from_slice(&2u32.to_le_bytes());
        boot[44..48].copy_from_slice(&2u32.to_le_bytes()); // root cluster

        // FAT: cluster 2 -> EOC, cluster 3 -> EOC.
        let fat = &mut device.blocks[1];
        fat[8..12].copy_from_slice(&0x0FFF_FFFFu32.to_le_bytes());
        fat[12..16].copy_from_slice(&0x0FFF_FFFFu32.to_le_bytes());

        // Root directory at cluster 2 (sector 3).
        let root = &mut device.blocks[3];
        root[0..11].copy_from_slice(b"README  TXT");
        root[11] = 0x20;
        root[20..22].copy_from_slice(&3u16.to_le_bytes()); // data cluster
        root[28..32].copy_from_slice(&11u32.to_le_bytes()); // size

        // File data at cluster 3 (sector 4).
        device.blocks[4][..11].copy_from_slice(b"hello fat32");
        device
    }

    #[test]
    fn mount_open_and_read() {
        let mut fs = Fat32::mount(build_image()).unwrap();
        assert_eq!(fs.boot_sector().root_cluster, 2);
        let file = fs.open("README.TXT").unwrap();
        assert_eq!(file.size, 11);
        assert_eq!(fs.read_file(&file).unwrap(), b"hello fat32");
    }
}
