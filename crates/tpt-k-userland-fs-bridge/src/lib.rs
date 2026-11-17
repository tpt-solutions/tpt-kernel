// SPDX-License-Identifier: MIT OR Apache-2.0
//! tpt-k filesystem bridge core: reads file bytes from a block device.
//!
//! A file is a list of 512-byte blocks, each at a device sector. [`ByteFile`]
//! says where each block is, and [`BlockFs::read_bytes`] turns a byte range
//! of that file into sector reads through a write-through LRU cache.
//!
//! This crate does not depend on tpt-filesystem. The mapping from a file
//! system's own layout to [`ByteFile`] is for a later adapter to provide.

#[cfg(test)]
extern crate std;

pub mod cache;
pub mod range;

use cache::{CacheStats, SectorCache};
use range::{span, SECTOR_SIZE};
use tpt_k_ipc::block::{BlockDevice, BlockError};

/// A file as a sequence of sector-sized blocks on a block device.
pub trait ByteFile {
    /// The file's length in bytes.
    fn size(&self) -> u64;

    /// The device sector that holds block `file_block` of the file (block
    /// `n` covers bytes `n * 512 .. (n + 1) * 512`), or `None` if the file has
    /// no such block.
    fn sector_of(&self, file_block: u64) -> Option<u64>;
}

/// A block device with a sector cache in front of it.
pub struct BlockFs<D: BlockDevice> {
    dev: D,
    cache: SectorCache,
}

impl<D: BlockDevice> BlockFs<D> {
    /// Wraps `dev`. The device must use 512-byte sectors, the only size the
    /// bridge supports.
    pub fn new(dev: D) -> Result<Self, BlockError> {
        if dev.info().sector_size as usize != SECTOR_SIZE {
            return Err(BlockError::BadRequest);
        }
        Ok(Self {
            dev,
            cache: SectorCache::new(),
        })
    }

    /// The wrapped device.
    pub fn device(&self) -> &D {
        &self.dev
    }

    /// The cache counters.
    #[must_use]
    pub fn stats(&self) -> CacheStats {
        self.cache.stats()
    }

    /// Reads sector `lba` through the cache.
    pub fn read_sector(&mut self, lba: u64, out: &mut [u8; SECTOR_SIZE]) -> Result<(), BlockError> {
        self.cache.read_sector(&mut self.dev, lba, out)
    }

    /// Writes sector `lba` through the cache to the device.
    pub fn write_sector(&mut self, lba: u64, data: &[u8; SECTOR_SIZE]) -> Result<(), BlockError> {
        self.cache.write_sector(&mut self.dev, lba, data)
    }

    /// Asks the device to make its writes durable.
    pub fn flush(&mut self) -> Result<(), BlockError> {
        self.dev.flush()
    }

    /// Reads up to `buf.len()` bytes of `file` starting at byte `offset`.
    ///
    /// The read stops at the end of the file, so the count may be short, and
    /// it is zero at or past the end. Returns an error if the file maps a block
    /// that the file does not have, or if the device fails.
    pub fn read_bytes<F: ByteFile>(
        &mut self,
        file: &F,
        offset: u64,
        buf: &mut [u8],
    ) -> Result<usize, BlockError> {
        let size = file.size();
        if offset >= size || buf.is_empty() {
            return Ok(0);
        }
        let want = (size - offset).min(buf.len() as u64) as usize;
        let span = span(offset, want as u64).ok_or(BlockError::OutOfRange)?;

        let mut done = 0usize;
        let mut sector = [0u8; SECTOR_SIZE];
        for i in 0..span.sectors {
            let file_block = span.first_sector + i;
            let lba = file.sector_of(file_block).ok_or(BlockError::OutOfRange)?;
            self.cache.read_sector(&mut self.dev, lba, &mut sector)?;
            let start = if i == 0 { span.start } else { 0 };
            let take = (SECTOR_SIZE - start).min(want - done);
            buf[done..done + take].copy_from_slice(&sector[start..start + take]);
            done += take;
        }
        Ok(done)
    }
}

#[cfg(test)]
mod tests {
    use super::cache::tests::MemDevice;
    use super::*;
    use std::vec::Vec;

    /// A file whose blocks are listed explicitly, so tests can make them
    /// contiguous or scattered.
    struct TestFile {
        blocks: Vec<u64>,
        size: u64,
    }

    impl ByteFile for TestFile {
        fn size(&self) -> u64 {
            self.size
        }

        fn sector_of(&self, file_block: u64) -> Option<u64> {
            self.blocks.get(file_block as usize).copied()
        }
    }

    /// A file that occupies sectors `first ..` in order.
    fn contiguous(first: u64, size: u64) -> TestFile {
        let blocks = (0..size.div_ceil(512)).map(|i| first + i).collect();
        TestFile { blocks, size }
    }

    fn fs(sectors: u64) -> BlockFs<MemDevice> {
        BlockFs::new(MemDevice::new(sectors)).unwrap()
    }

    #[test]
    fn read_crossing_a_sector_boundary() {
        let mut fs = fs(16);
        let file = contiguous(2, 2048);
        let mut buf = [0u8; 30];
        let n = fs.read_bytes(&file, 500, &mut buf).unwrap();
        assert_eq!(n, 30);
        let at = 2 * 512 + 500;
        assert_eq!(buf[..], fs.device().data[at..at + 30]);
    }

    #[test]
    fn unaligned_read_inside_one_sector() {
        let mut fs = fs(16);
        let file = contiguous(0, 4096);
        let mut buf = [0u8; 7];
        fs.read_bytes(&file, 515, &mut buf).unwrap();
        assert_eq!(buf[..], fs.device().data[515..522]);
    }

    #[test]
    fn large_unaligned_read_spanning_many_sectors() {
        let mut fs = fs(16);
        let file = contiguous(1, 4000);
        let mut buf = vec![0u8; 3000];
        let n = fs.read_bytes(&file, 333, &mut buf).unwrap();
        assert_eq!(n, 3000);
        let at = 512 + 333;
        assert_eq!(buf[..], fs.device().data[at..at + 3000]);
    }

    #[test]
    fn read_past_end_of_file_is_short_and_then_zero() {
        let mut fs = fs(16);
        let file = contiguous(0, 1000);
        let mut buf = [0u8; 300];
        assert_eq!(fs.read_bytes(&file, 900, &mut buf).unwrap(), 100);
        assert_eq!(fs.read_bytes(&file, 1000, &mut buf).unwrap(), 0);
        assert_eq!(fs.read_bytes(&file, 5000, &mut buf).unwrap(), 0);
    }

    #[test]
    fn scattered_blocks_map_to_their_own_sectors() {
        let mut fs = fs(16);
        let file = TestFile {
            blocks: std::vec![5, 2],
            size: 1024,
        };
        let mut first = [0u8; 512];
        fs.read_bytes(&file, 0, &mut first).unwrap();
        assert_eq!(first[..], fs.device().data[5 * 512..6 * 512]);
        let mut second = [0u8; 512];
        fs.read_bytes(&file, 512, &mut second).unwrap();
        assert_eq!(second[..], fs.device().data[2 * 512..3 * 512]);
    }

    #[test]
    fn missing_block_is_an_error() {
        let mut fs = fs(16);
        // The file claims 1024 bytes but maps only one block.
        let file = TestFile {
            blocks: std::vec![3],
            size: 1024,
        };
        let mut buf = [0u8; 16];
        assert_eq!(
            fs.read_bytes(&file, 600, &mut buf),
            Err(BlockError::OutOfRange)
        );
    }

    #[test]
    fn repeated_reads_come_from_the_cache() {
        let mut fs = fs(16);
        let file = contiguous(0, 1024);
        let mut buf = [0u8; 1024];
        fs.read_bytes(&file, 0, &mut buf).unwrap();
        fs.read_bytes(&file, 0, &mut buf).unwrap();
        assert_eq!(fs.stats().misses, 2);
        assert_eq!(fs.stats().hits, 2);
        assert_eq!(fs.device().reads, 2);
    }

    #[test]
    fn write_through_is_visible_to_later_reads() {
        let mut fs = fs(16);
        let data = [0x5Au8; 512];
        fs.write_sector(4, &data).unwrap();
        let file = contiguous(4, 512);
        let mut buf = [0u8; 512];
        fs.read_bytes(&file, 0, &mut buf).unwrap();
        assert_eq!(buf, data);
        assert_eq!(fs.device().writes, 1);
        assert_eq!(fs.device().reads, 0);
    }

    #[test]
    fn flush_reaches_the_device() {
        let mut fs = fs(4);
        fs.flush().unwrap();
        assert_eq!(fs.device().flushes, 1);
    }

    #[test]
    fn device_with_other_sector_size_is_refused() {
        let mut dev = MemDevice::new(4);
        dev.info.sector_size = 4096;
        assert!(BlockFs::new(dev).is_err());
    }
}
