//! Read-only NTFS driver: MFT parsing, attributes, run lists and file reads.
//!
//! Supports resident and non-resident $DATA attributes and resolves paths by
//! walking $FILE_NAME parent references (no $INDEX tree parsing needed).

use alloc::string::String;
use alloc::vec::Vec;

use crate::disk::BlockDevice;
use crate::FsError;

const ATTR_FILE_NAME: u32 = 0x30;
const ATTR_DATA: u32 = 0x80;
const NTFS_ROOT_REFERENCE: u64 = 5;
const MFT_SCAN_LIMIT: usize = 128;

struct BootSector {
    bytes_per_sector: usize,
    sectors_per_cluster: usize,
    mft_cluster: u64,
    record_size: usize,
}

/// A file discovered in the MFT.
pub struct NtfsFile {
    pub reference: u64,
    pub name: String,
    pub size: u64,
    pub is_directory: bool,
}

/// A mounted NTFS volume.
pub struct Ntfs<B: BlockDevice> {
    device: B,
    boot: BootSector,
}

impl<B: BlockDevice> Ntfs<B> {
    /// Parse the boot sector and mount the volume.
    pub fn mount(mut device: B) -> Result<Self, FsError> {
        let mut sector = [0u8; 512];
        device.read_block(0, &mut sector)?;
        if &sector[3..11] != b"NTFS    " {
            return Err(FsError::InvalidMagicNumber);
        }

        let bytes_per_sector = u16::from_le_bytes([sector[11], sector[12]]) as usize;
        let sectors_per_cluster = sector[13] as usize;
        let mft_cluster = u64::from_le_bytes([
            sector[48], sector[49], sector[50], sector[51], sector[52], sector[53], sector[54], sector[55],
        ]);
        let clusters_per_record = i8::from_le_bytes([sector[64]]) as i32;
        let record_size = if clusters_per_record > 0 {
            clusters_per_record as usize * sectors_per_cluster * bytes_per_sector
        } else {
            1usize << (-clusters_per_record) as u32
        };

        if bytes_per_sector != 512 || record_size == 0 {
            return Err(FsError::InvalidMagicNumber);
        }

        Ok(Ntfs {
            device,
            boot: BootSector {
                bytes_per_sector,
                sectors_per_cluster,
                mft_cluster,
                record_size,
            },
        })
    }

    /// Read one MFT record by index.
    fn read_record(&mut self, index: usize) -> Result<Vec<u8>, FsError> {
        let sectors_per_record = self.boot.record_size / self.boot.bytes_per_sector;
        let start = self.boot.mft_cluster as usize * self.boot.sectors_per_cluster
            + index * sectors_per_record;

        let mut record = Vec::with_capacity(self.boot.record_size);
        for offset in 0..sectors_per_record {
            let mut sector = [0u8; 512];
            self.device.read_block(start + offset, &mut sector)?;
            record.extend_from_slice(&sector);
        }

        if record.len() < 48 || &record[0..4] != b"FILE" {
            return Err(FsError::FileNotFound);
        }

        // Apply the update sequence array (fixups).
        let usa_offset = u16::from_le_bytes([record[4], record[5]]) as usize;
        let usa_count = u16::from_le_bytes([record[6], record[7]]) as usize;
        if usa_offset != 0 && usa_count > 1 {
            let usa = Vec::from(&record[usa_offset..usa_offset + usa_count * 2]);
            for entry in 0..usa_count - 1 {
                let position = (entry + 1) * 512 - 2;
                if position + 2 <= record.len() {
                    let value = [usa[2 + entry * 2], usa[3 + entry * 2]];
                    record[position..position + 2].copy_from_slice(&value);
                }
            }
        }

        Ok(record)
    }

    /// Iterate attributes of a record, calling `callback(type, bytes)`.
    fn for_each_attribute<F>(record: &[u8], mut callback: F)
    where
        F: FnMut(u32, &[u8]),
    {
        let mut offset = u16::from_le_bytes([record[20], record[21]]) as usize;
        while offset + 16 <= record.len() {
            let kind = u32::from_le_bytes([
                record[offset], record[offset + 1], record[offset + 2], record[offset + 3],
            ]);
            if kind == 0xFFFF_FFFF {
                break;
            }
            let length = u32::from_le_bytes([
                record[offset + 4], record[offset + 5], record[offset + 6], record[offset + 7],
            ]) as usize;
            if length < 16 || offset + length > record.len() {
                break;
            }

            let attribute = &record[offset..offset + length];
            if attribute[8] == 0 {
                // Resident: value follows the 24-byte resident header.
                let value_length = u32::from_le_bytes([
                    attribute[16], attribute[17], attribute[18], attribute[19],
                ]) as usize;
                let value_offset = u16::from_le_bytes([attribute[20], attribute[21]]) as usize;
                if value_offset + value_length <= attribute.len() {
                    callback(kind, &attribute[value_offset..value_offset + value_length]);
                }
            } else {
                // Non-resident: expose the whole header so run lists can be read.
                callback(kind, attribute);
            }

            offset += length;
        }
    }

    /// Extract the file name from a $FILE_NAME attribute value.
    fn parse_file_name(value: &[u8]) -> Option<(u64, String, bool)> {
        if value.len() < 66 {
            return None;
        }
        let parent = u64::from_le_bytes([
            value[0], value[1], value[2], value[3], value[4], value[5], value[6], value[7],
        ]);
        let flags = u32::from_le_bytes([value[56], value[57], value[58], value[59]]);
        let name_length = value[64] as usize;
        if 66 + name_length * 2 > value.len() {
            return None;
        }

        let mut name = String::new();
        for pair in value[66..66 + name_length * 2].chunks_exact(2) {
            let code = u16::from_le_bytes([pair[0], pair[1]]);
            name.push(char::from_u32(code as u32).unwrap_or('?'));
        }
        Some((parent, name, flags & 0x2 != 0)) // 0x2 = FILE_ATTRIBUTE_DIRECTORY
    }

    /// Resolve a record's full path by walking parent references.
    fn full_path(&mut self, index: usize) -> Option<String> {
        let record = self.read_record(index).ok()?;
        let (mut parent, name, _) = {
            let mut result = None;
            Self::for_each_attribute(&record, |kind, value| {
                if kind == ATTR_FILE_NAME && result.is_none() {
                    result = Self::parse_file_name(value);
                }
            });
            result?
        };

        let mut components = Vec::new();
        components.push(name);
        let mut guard = 0usize;
        while parent & 0xFFFF_FFFF_FFFF != NTFS_ROOT_REFERENCE && guard < 64 {
            guard += 1;
            let parent_index = (parent & 0xFFFF_FFFF_FFFF) as usize;
            if parent_index >= MFT_SCAN_LIMIT {
                return None;
            }
            let parent_record = self.read_record(parent_index).ok()?;
            let (next, parent_name, _) = {
                let mut result = None;
                Self::for_each_attribute(&parent_record, |kind, value| {
                    if kind == ATTR_FILE_NAME && result.is_none() {
                        result = Self::parse_file_name(value);
                    }
                });
                result?
            };
            components.push(parent_name);
            parent = next;
        }

        components.reverse();
        let mut path = String::from("/");
        for (index, component) in components.iter().enumerate() {
            if index != 0 {
                path.push('/');
            }
            path.push_str(component);
        }
        Some(path)
    }

    /// Look up a file by its full path (case-insensitive).
    pub fn open(&mut self, path: &str) -> Result<NtfsFile, FsError> {
        let wanted = if path.starts_with('/') { &path[1..] } else { path };
        for index in 0..MFT_SCAN_LIMIT {
            let full = match self.full_path(index) {
                Some(full) => full,
                None => continue,
            };
            let candidate = full.strip_prefix('/').unwrap_or(&full);
            if !candidate.eq_ignore_ascii_case(wanted) {
                continue;
            }
            let record = self.read_record(index)?;
            let mut size = 0u64;
            Self::for_each_attribute(&record, |kind, value| {
                if kind == ATTR_DATA && value.len() >= 64 && value[8] != 0 {
                    size = u64::from_le_bytes([
                        value[48], value[49], value[50], value[51], value[52], value[53], value[54], value[55],
                    ]);
                }
            });
            if size == 0 {
                Self::for_each_attribute(&record, |kind, value| {
                    if kind == ATTR_DATA && value.len() >= 20 && value[8] == 0 {
                        size = u32::from_le_bytes([
                            value[16], value[17], value[18], value[19],
                        ]) as u64;
                    }
                });
            }
            let name = candidate.rsplit('/').next().unwrap_or(candidate);
            return Ok(NtfsFile {
                reference: index as u64,
                name: String::from(name),
                size,
                is_directory: false,
            });
        }
        Err(FsError::FileNotFound)
    }

    /// Read a file's data, handling resident and non-resident attributes.
    pub fn read_file(&mut self, file: &NtfsFile) -> Result<Vec<u8>, FsError> {
        let record = self.read_record(file.reference as usize)?;
        let mut result = None;
        Self::for_each_attribute(&record, |kind, value| {
            if kind != ATTR_DATA || result.is_some() {
                return;
            }
            if value[8] == 0 && value.len() >= 22 {
                let length = u32::from_le_bytes([value[16], value[17], value[18], value[19]]) as usize;
                let offset = u16::from_le_bytes([value[20], value[21]]) as usize;
                if offset + length <= value.len() {
                    result = Some(Vec::from(&value[offset..offset + length]));
                }
            } else if value.len() >= 64 {
                let runlist_offset = u16::from_le_bytes([value[32], value[33]]) as usize;
                let real_size = u64::from_le_bytes([
                    value[48], value[49], value[50], value[51], value[52], value[53], value[54], value[55],
                ]) as usize;
                if runlist_offset < value.len() {
                    result = Some(self.read_runs(&value[runlist_offset..], real_size));
                }
            }
        });
        result.ok_or(FsError::FileNotFound)
    }

    /// Decode NTFS data runs into file contents.
    fn read_runs(&mut self, runs: &[u8], max_length: usize) -> Vec<u8> {
        let mut data = Vec::with_capacity(max_length);
        let mut offset = 0usize;
        let mut lcn: i64 = 0;

        while offset < runs.len() {
            let header = runs[offset];
            offset += 1;
            if header == 0 {
                break;
            }
            let length_size = (header & 0x0F) as usize;
            let offset_size = (header >> 4) as usize;
            if length_size == 0 || offset + length_size + offset_size > runs.len() {
                break;
            }

            let mut run_length = 0u64;
            for byte in &runs[offset..offset + length_size] {
                run_length = (run_length << 8) | *byte as u64;
            }
            offset += length_size;

            let mut run_offset = 0u64;
            for byte in &runs[offset..offset + offset_size] {
                run_offset = (run_offset << 8) | *byte as u64;
            }
            offset += offset_size;

            // Sign-extend the delta when the high bit is set.
            if offset_size > 0 && runs[offset - 1] & 0x80 != 0 {
                run_offset |= !0u64 << (offset_size * 8);
            }
            lcn = lcn.wrapping_add(run_offset as i64);

            for cluster in 0..run_length as usize {
                if data.len() >= max_length {
                    break;
                }
                let sector = lcn as usize * self.boot.sectors_per_cluster + cluster * self.boot.sectors_per_cluster;
                for sector_offset in 0..self.boot.sectors_per_cluster {
                    let mut buffer = [0u8; 512];
                    if self.device.read_block(sector + sector_offset, &mut buffer).is_err() {
                        return data;
                    }
                    let remaining = max_length - data.len();
                    data.extend_from_slice(&buffer[..remaining.min(512)]);
                }
            }
        }

        data
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

    fn set_u16(record: &mut [u8], offset: usize, value: u16) {
        record[offset..offset + 2].copy_from_slice(&value.to_le_bytes());
    }

    fn set_u32(record: &mut [u8], offset: usize, value: u32) {
        record[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
    }

    fn write_record(image: &mut [u8], index: usize, attributes: &[u8], used: usize) {
        let start = (1 + index * 2) * BLOCK_SIZE;
        let record = &mut image[start..start + 1024];
        record[0..4].copy_from_slice(b"FILE");
        set_u16(record, 20, 0x38); // first attribute offset
        set_u16(record, 22, 1);    // in use
        set_u32(record, 24, used as u32);
        set_u32(record, 28, 1024);
        record[0x38..0x38 + attributes.len()].copy_from_slice(attributes);
    }

    fn file_name_attribute(parent: u64, name: &str, is_directory: bool) -> Vec<u8> {
        let mut value = vec![0u8; 66 + name.len() * 2];
        value[0..8].copy_from_slice(&parent.to_le_bytes());
        if is_directory {
            value[56] = 2;
        }
        value[64] = name.len() as u8;
        value[65] = 1;
        for (index, character) in name.chars().enumerate() {
            value[66 + index * 2..68 + index * 2].copy_from_slice(&(character as u16).to_le_bytes());
        }

        let length = (0x40 + value.len() + 7) & !7;
        let mut attribute = vec![0u8; length];
        attribute[0..4].copy_from_slice(&ATTR_FILE_NAME.to_le_bytes());
        attribute[4..8].copy_from_slice(&(length as u32).to_le_bytes());
        attribute[16..20].copy_from_slice(&(value.len() as u32).to_le_bytes());
        attribute[20..22].copy_from_slice(&0x40u16.to_le_bytes());
        attribute[0x40..0x40 + value.len()].copy_from_slice(&value);
        attribute
    }

    fn data_attribute(size: usize, lcn: u8) -> Vec<u8> {
        let mut attribute = vec![0u8; 72];
        attribute[0..4].copy_from_slice(&ATTR_DATA.to_le_bytes());
        attribute[4..8].copy_from_slice(&72u32.to_le_bytes());
        attribute[8] = 1; // non-resident
        attribute[32..34].copy_from_slice(&0x40u16.to_le_bytes());
        attribute[48..56].copy_from_slice(&(size as u64).to_le_bytes());
        // One run of one cluster at LCN 20, then end.
        attribute[0x40..0x43].copy_from_slice(&[0x11, 0x01, lcn]);
        attribute[0x43] = 0;
        attribute
    }

    fn payload_len() -> usize {
        b"hello ntfs".len()
    }

    fn build_image() -> MemoryDevice {
        let mut image = vec![0u8; 32 * BLOCK_SIZE];
        let boot = &mut image[..BLOCK_SIZE];
        boot[3..11].copy_from_slice(b"NTFS    ");
        boot[11..13].copy_from_slice(&512u16.to_le_bytes());
        boot[13] = 1;
        boot[48..56].copy_from_slice(&1u64.to_le_bytes()); // MFT at cluster 1
        boot[64] = 2; // 2 sectors per MFT record

        // Record 5: root directory.
        let root = file_name_attribute(NTFS_ROOT_REFERENCE, ".", true);
        write_record(&mut image, 5, &root, 0x38 + root.len());

        // Record 6: HELLO.TXT with non-resident data at LCN 20.
        let mut attributes = file_name_attribute(NTFS_ROOT_REFERENCE, "HELLO.TXT", false);
        attributes.extend_from_slice(&data_attribute(payload_len(), 20));
        write_record(&mut image, 6, &attributes, 0x38 + attributes.len());

        let payload = b"hello ntfs";
        image[20 * BLOCK_SIZE..20 * BLOCK_SIZE + payload.len()].copy_from_slice(payload);

        MemoryDevice { blocks: image.chunks(BLOCK_SIZE).map(|chunk| {
            let mut block = [0u8; BLOCK_SIZE];
            block.copy_from_slice(chunk);
            block
        }).collect() }
    }

    #[test]
    fn mount_open_and_read() {
        let mut fs = Ntfs::mount(build_image()).unwrap();
        let file = fs.open("/HELLO.TXT").unwrap();
        assert_eq!(file.size, 10);
        assert_eq!(fs.read_file(&file).unwrap(), b"hello ntfs");
    }
}
