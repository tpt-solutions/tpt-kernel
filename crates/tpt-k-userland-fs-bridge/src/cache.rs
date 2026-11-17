// SPDX-License-Identifier: MIT OR Apache-2.0
//! A fixed-capacity, write-through LRU cache of sectors in front of a block device.

use crate::range::SECTOR_SIZE;
use tpt_k_ipc::block::{check_range, BlockDevice, BlockError};

/// Number of sectors the cache holds. The storage is inline: 16 x 512 bytes.
pub const CACHE_SECTORS: usize = 16;

/// Hit, miss and write counts, for tests and for tuning.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct CacheStats {
    /// Reads served from the cache.
    pub hits: u64,
    /// Reads that went to the device.
    pub misses: u64,
    /// Writes passed through to the device.
    pub writes: u64,
}

#[derive(Clone, Copy)]
struct Slot {
    lba: u64,
    valid: bool,
    last_use: u64,
    data: [u8; SECTOR_SIZE],
}

const EMPTY: Slot = Slot {
    lba: 0,
    valid: false,
    last_use: 0,
    data: [0; SECTOR_SIZE],
};

/// A write-through LRU cache of 512-byte sectors.
///
/// Reads are served from the cache when they can be. A miss reads the sector
/// from the device and evicts the least recently used slot if the cache is full.
/// A write always goes to the device first; the cached copy is then updated, so
/// a cache never holds data the device does not. Because nothing is dirty, a
/// flush has nothing to write back and only asks the device to flush.
pub struct SectorCache {
    slots: [Slot; CACHE_SECTORS],
    clock: u64,
    stats: CacheStats,
}

impl Default for SectorCache {
    fn default() -> Self {
        Self::new()
    }
}

impl SectorCache {
    /// An empty cache.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            slots: [EMPTY; CACHE_SECTORS],
            clock: 0,
            stats: CacheStats {
                hits: 0,
                misses: 0,
                writes: 0,
            },
        }
    }

    /// The counters so far.
    #[must_use]
    pub const fn stats(&self) -> CacheStats {
        self.stats
    }

    fn find(&self, lba: u64) -> Option<usize> {
        self.slots.iter().position(|s| s.valid && s.lba == lba)
    }

    /// The slot to reuse: an empty one if there is any, else the least recently used.
    fn victim(&self) -> usize {
        if let Some(i) = self.slots.iter().position(|s| !s.valid) {
            return i;
        }
        self.slots
            .iter()
            .enumerate()
            .min_by_key(|(_, s)| s.last_use)
            .map_or(0, |(i, _)| i)
    }

    /// Reads sector `lba` into `out`, from the cache if it is there.
    pub fn read_sector<D: BlockDevice>(
        &mut self,
        dev: &mut D,
        lba: u64,
        out: &mut [u8; SECTOR_SIZE],
    ) -> Result<(), BlockError> {
        self.clock += 1;
        if let Some(i) = self.find(lba) {
            self.stats.hits += 1;
            self.slots[i].last_use = self.clock;
            out.copy_from_slice(&self.slots[i].data);
            return Ok(());
        }

        self.stats.misses += 1;
        check_range(&dev.info(), lba, 1)?;
        let i = self.victim();
        // Mark the slot empty first, so a failed read cannot leave it looking valid.
        self.slots[i].valid = false;
        dev.read(lba, &mut self.slots[i].data)?;
        self.slots[i].lba = lba;
        self.slots[i].valid = true;
        self.slots[i].last_use = self.clock;
        out.copy_from_slice(&self.slots[i].data);
        Ok(())
    }

    /// Writes `data` to sector `lba` on the device, then updates the cache.
    ///
    /// If the device refuses the write, any cached copy of the sector is
    /// dropped, so the cache cannot disagree with the device.
    pub fn write_sector<D: BlockDevice>(
        &mut self,
        dev: &mut D,
        lba: u64,
        data: &[u8; SECTOR_SIZE],
    ) -> Result<(), BlockError> {
        let info = dev.info();
        if info.read_only {
            return Err(BlockError::ReadOnly);
        }
        check_range(&info, lba, 1)?;
        if let Err(e) = dev.write(lba, data) {
            if let Some(i) = self.find(lba) {
                self.slots[i].valid = false;
            }
            return Err(e);
        }
        self.clock += 1;
        self.stats.writes += 1;
        let i = self.find(lba).unwrap_or_else(|| self.victim());
        self.slots[i].lba = lba;
        self.slots[i].valid = true;
        self.slots[i].last_use = self.clock;
        self.slots[i].data.copy_from_slice(data);
        Ok(())
    }

    /// Drops every cached sector. The next read of each goes to the device.
    pub fn invalidate(&mut self) {
        for s in &mut self.slots {
            s.valid = false;
        }
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use std::vec::Vec;
    use tpt_k_ipc::block::BlockInfo;

    /// A device backed by a byte vector, counting its own calls.
    pub(crate) struct MemDevice {
        pub data: Vec<u8>,
        pub info: BlockInfo,
        pub reads: u64,
        pub writes: u64,
        pub flushes: u64,
    }

    impl MemDevice {
        pub(crate) fn new(sectors: u64) -> Self {
            Self {
                data: (0..sectors * 512).map(|i| (i % 251) as u8).collect(),
                info: BlockInfo {
                    sector_size: 512,
                    sector_count: sectors,
                    read_only: false,
                },
                reads: 0,
                writes: 0,
                flushes: 0,
            }
        }
    }

    impl BlockDevice for MemDevice {
        fn info(&self) -> BlockInfo {
            self.info
        }

        fn read(&mut self, lba: u64, out: &mut [u8]) -> Result<(), BlockError> {
            check_range(&self.info, lba, (out.len() / 512) as u32)?;
            self.reads += 1;
            let at = lba as usize * 512;
            out.copy_from_slice(&self.data[at..at + out.len()]);
            Ok(())
        }

        fn write(&mut self, lba: u64, data: &[u8]) -> Result<(), BlockError> {
            check_range(&self.info, lba, (data.len() / 512) as u32)?;
            self.writes += 1;
            let at = lba as usize * 512;
            self.data[at..at + data.len()].copy_from_slice(data);
            Ok(())
        }

        fn flush(&mut self) -> Result<(), BlockError> {
            self.flushes += 1;
            Ok(())
        }
    }

    fn sector(fill: u8) -> [u8; 512] {
        [fill; 512]
    }

    #[test]
    fn repeat_read_hits_the_cache() {
        let mut dev = MemDevice::new(64);
        let mut cache = SectorCache::new();
        let mut out = [0u8; 512];
        cache.read_sector(&mut dev, 3, &mut out).unwrap();
        cache.read_sector(&mut dev, 3, &mut out).unwrap();
        assert_eq!(
            cache.stats(),
            CacheStats {
                hits: 1,
                misses: 1,
                writes: 0
            }
        );
        assert_eq!(dev.reads, 1);
        assert_eq!(out[..], dev.data[3 * 512..4 * 512]);
    }

    #[test]
    fn least_recently_used_slot_is_evicted() {
        let mut dev = MemDevice::new(64);
        let mut cache = SectorCache::new();
        let mut out = [0u8; 512];
        for lba in 0..CACHE_SECTORS as u64 {
            cache.read_sector(&mut dev, lba, &mut out).unwrap();
        }
        // Touch sector 0 so that sector 1 is now the oldest.
        cache.read_sector(&mut dev, 0, &mut out).unwrap();
        // A new sector evicts sector 1.
        cache.read_sector(&mut dev, 16, &mut out).unwrap();
        let before = cache.stats().misses;
        cache.read_sector(&mut dev, 0, &mut out).unwrap();
        assert_eq!(
            cache.stats().misses,
            before,
            "sector 0 should still be cached"
        );
        cache.read_sector(&mut dev, 1, &mut out).unwrap();
        assert_eq!(
            cache.stats().misses,
            before + 1,
            "sector 1 should have been evicted"
        );
    }

    #[test]
    fn write_goes_through_and_updates_the_cache() {
        let mut dev = MemDevice::new(8);
        let mut cache = SectorCache::new();
        let new = sector(0xAB);
        cache.write_sector(&mut dev, 5, &new).unwrap();
        assert_eq!(dev.writes, 1);
        assert_eq!(dev.data[5 * 512..6 * 512], new[..]);

        let mut out = [0u8; 512];
        cache.read_sector(&mut dev, 5, &mut out).unwrap();
        assert_eq!(out, new);
        assert_eq!(dev.reads, 0, "the write should have filled the cache");
        assert_eq!(cache.stats().hits, 1);
    }

    #[test]
    fn refused_write_drops_the_cached_copy() {
        let mut dev = MemDevice::new(8);
        let mut cache = SectorCache::new();
        let mut out = [0u8; 512];
        cache.read_sector(&mut dev, 2, &mut out).unwrap();
        // Out of range: the device refuses, and the cache must not keep an old copy.
        assert_eq!(
            cache.write_sector(&mut dev, 99, &sector(1)),
            Err(BlockError::OutOfRange)
        );
        dev.info.read_only = true;
        assert_eq!(
            cache.write_sector(&mut dev, 2, &sector(1)),
            Err(BlockError::ReadOnly)
        );
        assert_eq!(dev.writes, 0);
    }

    #[test]
    fn out_of_range_read_is_refused_without_touching_the_cache() {
        let mut dev = MemDevice::new(4);
        let mut cache = SectorCache::new();
        let mut out = [0u8; 512];
        assert_eq!(
            cache.read_sector(&mut dev, 4, &mut out),
            Err(BlockError::OutOfRange)
        );
        assert_eq!(dev.reads, 0);
        assert_eq!(cache.find(4), None);
        // A miss is still counted: the lookup missed before the range check.
        assert_eq!(cache.stats().misses, 1);
    }

    #[test]
    fn invalidate_forces_a_device_read() {
        let mut dev = MemDevice::new(4);
        let mut cache = SectorCache::new();
        let mut out = [0u8; 512];
        cache.read_sector(&mut dev, 1, &mut out).unwrap();
        cache.invalidate();
        cache.read_sector(&mut dev, 1, &mut out).unwrap();
        assert_eq!(dev.reads, 2);
    }
}
