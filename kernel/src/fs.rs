//! Kernel filesystem integration over the ATA block device.

use zenith_fs::disk::{BlockDevice, ZenithFs};
use zenith_fs::ext4::Ext4;
use zenith_fs::fat32::Fat32;
use zenith_fs::ntfs::Ntfs;
use zenith_fs::{FsError, BLOCK_SIZE};

use crate::ata;
use crate::diagnostics;

const FS_BASE_LBA: usize = 4096; // 2 MiB into the disk image
const FS_BLOCKS: usize = 32768;  // 16 MiB

const FAT32_BASE_LBA: usize = 40960;
const FAT32_BLOCKS: usize = 1024;
const NTFS_BASE_LBA: usize = 49152;
const NTFS_BLOCKS: usize = 32;
const EXT4_BASE_LBA: usize = 57344;
const EXT4_BLOCKS: usize = 65536;

struct FatAtaBlockDevice;

impl BlockDevice for FatAtaBlockDevice {
    fn read_block(&mut self, block: usize, buffer: &mut [u8; BLOCK_SIZE]) -> Result<(), FsError> {
        if block >= FAT32_BLOCKS {
            return Err(FsError::InvalidBlockNumber);
        }
        ata::read_sector((FAT32_BASE_LBA + block) as u32, buffer)
            .map_err(|_| FsError::IoError)
    }

    fn write_block(&mut self, _block: usize, _data: &[u8; BLOCK_SIZE]) -> Result<(), FsError> {
        Err(FsError::PermissionDenied) // read-only mount
    }

    fn block_count(&self) -> usize {
        FAT32_BLOCKS
    }
}

struct Ext4AtaBlockDevice;

impl BlockDevice for Ext4AtaBlockDevice {
    fn read_block(&mut self, block: usize, buffer: &mut [u8; BLOCK_SIZE]) -> Result<(), FsError> {
        if block >= EXT4_BLOCKS {
            return Err(FsError::InvalidBlockNumber);
        }
        ata::read_sector((EXT4_BASE_LBA + block) as u32, buffer).map_err(|_| FsError::IoError)
    }

    fn write_block(&mut self, _block: usize, _data: &[u8; BLOCK_SIZE]) -> Result<(), FsError> {
        Err(FsError::PermissionDenied)
    }

    fn block_count(&self) -> usize {
        EXT4_BLOCKS
    }
}

struct NtfsAtaBlockDevice;

impl BlockDevice for NtfsAtaBlockDevice {
    fn read_block(&mut self, block: usize, buffer: &mut [u8; BLOCK_SIZE]) -> Result<(), FsError> {
        if block >= NTFS_BLOCKS {
            return Err(FsError::InvalidBlockNumber);
        }
        ata::read_sector((NTFS_BASE_LBA + block) as u32, buffer).map_err(|_| FsError::IoError)
    }

    fn write_block(&mut self, _block: usize, _data: &[u8; BLOCK_SIZE]) -> Result<(), FsError> {
        Err(FsError::PermissionDenied)
    }

    fn block_count(&self) -> usize {
        NTFS_BLOCKS
    }
}

pub(crate) struct AtaBlockDevice;

// Keep one authoritative inode/extent cache for installer and desktop writes.
static VOLUME: crate::sync::SpinLock<Option<ZenithFs<AtaBlockDevice>>> =
    crate::sync::SpinLock::new(None);

pub(crate) fn volume<T>(f: impl FnOnce(&mut ZenithFs<AtaBlockDevice>) -> Result<T, FsError>)
    -> Result<T, FsError>
{
    let mut mounted = VOLUME.lock();
    f(mounted.as_mut().ok_or(FsError::InvalidMagicNumber)?)
}

impl BlockDevice for AtaBlockDevice {
    fn read_block(&mut self, block: usize, buffer: &mut [u8; BLOCK_SIZE]) -> Result<(), FsError> {
        if block >= FS_BLOCKS {
            return Err(FsError::InvalidBlockNumber);
        }
        ata::read_sector((FS_BASE_LBA + block) as u32, buffer)
            .map_err(|_| FsError::IoError)
    }

    fn write_block(&mut self, block: usize, data: &[u8; BLOCK_SIZE]) -> Result<(), FsError> {
        if block >= FS_BLOCKS {
            return Err(FsError::InvalidBlockNumber);
        }
        ata::write_sector((FS_BASE_LBA + block) as u32, data)
            .map_err(|_| FsError::IoError)
    }

    fn block_count(&self) -> usize {
        FS_BLOCKS
    }

    fn sync(&mut self) -> Result<(), FsError> {
        ata::flush_cache().map_err(|_| FsError::IoError)
    }
}

/// Mount Zenith FS, creating it on first boot. A counter file proves that data
/// survives a reboot by incrementing on every boot.
pub fn init() {
    let mut device = AtaBlockDevice;

    let mut probe = [0u8; BLOCK_SIZE];
    let needs_format = match ata::read_sector(FS_BASE_LBA as u32, &mut probe) {
        Ok(()) => u32::from_le_bytes([probe[0], probe[1], probe[2], probe[3]]) != 0x5A45_4E49,
        Err(_) => { diagnostics::debug("[fs] no readable data disk"); return; }
    };

    if needs_format {
        // A missing signature is not consent to overwrite another volume.
        // Only initialize a blank region on an unpartitioned data disk.
        if ata::read_sector(0, &mut probe).is_err() || probe != [0; BLOCK_SIZE]
            || !(0..FS_BLOCKS).all(|i| device.read_block(i, &mut probe).is_ok() && probe == [0; BLOCK_SIZE]) {
            diagnostics::debug("[fs] unrecognized data disk; leaving it untouched");
            return;
        }
        if let Err(error) = ZenithFs::format(&mut device) {
            diagnostics::puts("[fs] Zenith FS format failed: ");
            diagnostics::puts(match error {
                FsError::IoError => "I/O error",
                _ => "error",
            });
            diagnostics::puts("\n");
            return;
        }
        diagnostics::debug("[fs] Zenith FS formatted");
    }

    let mut fs = match ZenithFs::mount(device) {
        Ok(fs) => fs,
        Err(_) => {
            diagnostics::debug("[fs] Zenith FS mount failed");
            return;
        }
    };

    let inode = match fs.find("counter.txt") {
        Some(inode) => inode,
        None => match fs.create("counter.txt", false) {
            Ok(inode) => inode,
            Err(_) => {
                diagnostics::debug("[fs] could not create counter.txt");
                return;
            }
        },
    };

    let mut count = 0u32;
    if let Ok(data) = fs.read(inode) {
        if data.len() >= 4 {
            count = u32::from_le_bytes([data[0], data[1], data[2], data[3]]);
        }
    }
    count += 1;

    if fs.write(inode, &count.to_le_bytes()).is_err() {
        diagnostics::debug("[fs] could not update counter.txt");
        return;
    }

    diagnostics::puts("[fs] Zenith FS mounted; boot count = ");
    diagnostics::put_hex(count as u64);
    diagnostics::puts("\n");
    *VOLUME.lock() = Some(fs);

    init_fat32();
    init_ntfs();
    init_ext4();
}

/// Write installer manifest files onto the mounted Zenith FS partition.
/// Returns a short human-readable error description on failure.
pub fn installer_write() -> Result<(), &'static str> {
    let map_error = |error: FsError| -> &'static str {
        match error {
            FsError::IoError => "disk I/O error",
            FsError::DiskFull => "disk full",
            FsError::PermissionDenied => "permission denied",
            FsError::InvalidPath => "invalid path",
            FsError::InvalidMagicNumber => "filesystem not present",
            _ => "filesystem error",
        }
    };

    let mut mounted = VOLUME.lock();
    let fs = mounted.as_mut().ok_or("filesystem not present")?;
    fs.create_path("/boot", true).map_err(map_error)?;
    let inode = fs
        .create_path("/boot/ZENITH.CONF", false)
        .map_err(map_error)?;
    let manifest = concat!(env!("ZENITH_RELEASE"), "\ninstalled-by=setup\nroot=zenith-fs\n").as_bytes();
    fs.write(inode, manifest).map_err(map_error)?;
    fs.flush().map_err(map_error)?;
    Ok(())
}

/// Mount the ext4 test partition and read a known file.
fn init_ext4() {
    let mut fs = match Ext4::mount(Ext4AtaBlockDevice) {
        Ok(fs) => fs,
        Err(_) => {
            diagnostics::debug("[fs] no ext4 partition detected");
            return;
        }
    };

    match fs.open("/HELLO.TXT") {
        Ok(file) => match fs.read_file(&file) {
            Ok(data) => {
                diagnostics::puts("[fs] ext4 HELLO.TXT: ");
                diagnostics::puts(core::str::from_utf8(&data).unwrap_or("<invalid utf8>"));
                diagnostics::puts("\n");
            }
            Err(_) => diagnostics::debug("[fs] ext4 read failed"),
        },
        Err(_) => diagnostics::debug("[fs] ext4 HELLO.TXT not found"),
    }
}

/// Mount the NTFS test partition and read a known file.
fn init_ntfs() {
    let mut fs = match Ntfs::mount(NtfsAtaBlockDevice) {
        Ok(fs) => fs,
        Err(_) => {
            diagnostics::debug("[fs] no NTFS partition detected");
            return;
        }
    };

    match fs.open("/HELLO.TXT") {
        Ok(file) => match fs.read_file(&file) {
            Ok(data) => {
                diagnostics::puts("[fs] NTFS HELLO.TXT: ");
                diagnostics::puts(core::str::from_utf8(&data).unwrap_or("<invalid utf8>"));
                diagnostics::puts("\n");
            }
            Err(_) => diagnostics::debug("[fs] NTFS read failed"),
        },
        Err(_) => diagnostics::debug("[fs] NTFS HELLO.TXT not found"),
    }
}

/// Mount the FAT32 partition and read a known file from it.
fn init_fat32() {
    let mut fs = match Fat32::mount(FatAtaBlockDevice) {
        Ok(fs) => fs,
        Err(_) => {
            diagnostics::debug("[fs] no FAT32 partition detected");
            return;
        }
    };

    match fs.open("HELLO.TXT") {
        Ok(file) => match fs.read_file(&file) {
            Ok(data) => {
                diagnostics::puts("[fs] FAT32 HELLO.TXT: ");
                diagnostics::puts(core::str::from_utf8(&data).unwrap_or("<invalid utf8>"));
                diagnostics::puts("\n");
            }
            Err(_) => diagnostics::debug("[fs] FAT32 read failed"),
        },
        Err(_) => diagnostics::debug("[fs] FAT32 HELLO.TXT not found"),
    }
}
