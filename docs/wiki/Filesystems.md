# Filesystems

All filesystem code lives in `filesystem/zenith-fs/`.

## Zenith FS v5

The primary filesystem is fully disk-backed over ATA.

- **On-disk format**: superblock, 128-byte inodes with extent roots, directory
  entries, data blocks, and a reserved journal area.
- **Operations**: `format`, `mount`, `create`, `read`, `write`, `unlink`,
  `resolve`, `list`.
- **Journaling**: write-ahead journal with commit marker and replay on mount.
- **Extents**: 64-bit file sizes, sparse files, large files. Verified with a
  200 GiB sparse file and a 9 MiB contiguous file.
- **Permissions**: Unix-style `rwx` with owner/group, `chmod`, `chown`, and a
  current-user context; root bypasses checks.
- **Directories**: nested paths and directories that grow across multiple
  blocks.
- **VFS**: a `FileSystem` trait shared with the other drivers.
- **Persistence**: verified across real reboots via an incrementing boot counter.

Key source: `disk.rs`, `journal.rs`, `vfs.rs`, `lib.rs`.

## FAT32 (read-only)

`fat32.rs` parses the BPB, FAT chains, and directory entries. It reads files
from a test partition embedded in the disk images and is unit-tested.

## NTFS (read-only)

`ntfs.rs` parses the boot sector, MFT records, fixups, attributes, and data
runs. Paths are resolved by scanning `$FILE_NAME` attributes rather than
parsing `$INDEX` trees. Unit-tested with a synthetic image.

## ext4 (read-only)

`ext4.rs` parses the superblock, group descriptors, extents, and directories.
It is validated against an image produced by real `mkfs.ext4`, not just a
synthetic fixture.

## Block device interface

```rust
pub trait BlockDevice {
    fn read_block(&mut self, block: usize, buffer: &mut [u8; 512]) -> Result<(), FsError>;
    fn write_block(&mut self, block: usize, data: &[u8; 512]) -> Result<(), FsError>;
    fn block_count(&self) -> usize;
}
```

The kernel implements this over ATA with different LBA offsets for each test
partition.

## Limits

- FAT32/NTFS/ext4 are read-only.
- Zenith FS has one journal mode and no directory ACLs beyond `rwx`.
- No unified VFS mount table; the trait exists but mounting is manual.
