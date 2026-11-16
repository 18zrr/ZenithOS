//! Simple write-ahead journal for Zenith FS.
//!
//! A transaction records the full contents of every block it will modify,
//! writes them to a reserved journal area, marks the transaction committed,
//! then applies them. If a crash occurs before commit the transaction is
//! ignored; if it occurs after commit, `replay()` finishes it on mount.

use alloc::vec::Vec;

use crate::disk::BlockDevice;
use crate::{FsError, BLOCK_SIZE};

pub const JOURNAL_BLOCKS: usize = 32;

const JOURNAL_MAGIC: u32 = 0x5A4A_524E; // "ZJRN"
const HEADER_BLOCK: usize = 0;

#[repr(C, packed)]
#[derive(Clone, Copy)]
struct JournalHeader {
    magic: u32,
    transaction_id: u64,
    committed: u64,
    record_count: u32,
    checksum: u32,
}

/// Pending block writes for one transaction.
pub struct Journal {
    records: Vec<(u32, [u8; BLOCK_SIZE])>,
}

impl Journal {
    pub fn new() -> Self {
        Journal { records: Vec::new() }
    }

    /// Stage a block's new contents.
    pub fn stage(&mut self, block: u32, data: &[u8; BLOCK_SIZE]) -> Result<(), FsError> {
        if self.records.len() >= JOURNAL_BLOCKS - 1 {
            return Err(FsError::DiskFull);
        }
        if let Some(record) = self.records.iter_mut().find(|(number, _)| *number == block) {
            record.1.copy_from_slice(data);
        } else {
            self.records.push((block, *data));
        }
        Ok(())
    }

    /// Write the journal, mark it committed, apply the changes, then clear it.
    pub fn commit<B: BlockDevice>(
        &mut self,
        device: &mut B,
        start: usize,
        transaction_id: u64,
    ) -> Result<(), FsError> {
        if self.records.is_empty() {
            return Ok(());
        }

        let checksum = self.records.iter().fold(0u32, |sum, (number, data)| {
            sum.wrapping_add(*number).wrapping_add(data[0] as u32)
        });

        // Phase 1: write record data and the block-number table in the header.
        let mut header_block = [0u8; BLOCK_SIZE];
        for (index, (number, data)) in self.records.iter().enumerate() {
            device.write_block(start + 1 + index, data)?;
            let offset = 64 + index * 4;
            header_block[offset..offset + 4].copy_from_slice(&number.to_le_bytes());
        }

        // Phase 2: commit marker.
        let header = JournalHeader {
            magic: JOURNAL_MAGIC,
            transaction_id,
            committed: 1,
            record_count: self.records.len() as u32,
            checksum,
        };
        let bytes = unsafe {
            core::slice::from_raw_parts(
                &header as *const _ as *const u8,
                core::mem::size_of::<JournalHeader>(),
            )
        };
        header_block[..bytes.len()].copy_from_slice(bytes);
        device.write_block(start + HEADER_BLOCK, &header_block)?;

        // Phase 3: apply to real blocks.
        for (number, data) in &self.records {
            device.write_block(*number as usize, data)?;
        }

        // Phase 4: clear the journal header.
        device.write_block(start + HEADER_BLOCK, &[0u8; BLOCK_SIZE])?;
        self.records.clear();
        Ok(())
    }

    /// Replay a committed transaction left behind by a crash.
    pub fn replay<B: BlockDevice>(device: &mut B, start: usize) -> Result<bool, FsError> {
        let mut header_block = [0u8; BLOCK_SIZE];
        device.read_block(start + HEADER_BLOCK, &mut header_block)?;
        let header: JournalHeader = unsafe {
            core::ptr::read_unaligned(header_block.as_ptr() as *const JournalHeader)
        };
        if header.magic != JOURNAL_MAGIC || header.committed == 0 {
            return Ok(false);
        }
        let _ = header.checksum;

        let numbers_offset = 64;
        for index in 0..header.record_count as usize {
            let offset = numbers_offset + index * 4;
            let number = u32::from_le_bytes([
                header_block[offset], header_block[offset + 1],
                header_block[offset + 2], header_block[offset + 3],
            ]);
            let mut data = [0u8; BLOCK_SIZE];
            device.read_block(start + 1 + index, &mut data)?;
            device.write_block(number as usize, &data)?;
        }
        device.write_block(start + HEADER_BLOCK, &[0u8; BLOCK_SIZE])?;
        Ok(true)
    }
}
