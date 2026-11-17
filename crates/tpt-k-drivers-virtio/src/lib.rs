// SPDX-License-Identifier: MIT OR Apache-2.0
//! tpt-k VirtIO drivers: the split virtqueue (VirtIO 1.x) and the virtio-blk
//! request layer.
//!
//! This crate works on byte slices. It lays out the queue areas, encodes and
//! decodes descriptors and ring entries, allocates descriptor chains, builds
//! virtio-blk request chains, and checks the device status sequence. It does
//! no device I/O: the driver maps the memory and writes the MMIO registers.
//! All fields are little-endian, as VirtIO 1.x requires on this target.

#![no_std]

#[cfg(test)]
extern crate std;

/// Largest queue size the split-ring layout allows.
pub const MAX_QUEUE_SIZE: u16 = 32768;

/// Alignment of the descriptor table.
pub const DESC_ALIGN: usize = 16;
/// Alignment of the available ring.
pub const AVAIL_ALIGN: usize = 2;
/// Alignment of the used ring.
pub const USED_ALIGN: usize = 4;

/// Descriptor flag: the chain continues at `next`.
pub const DESC_F_NEXT: u16 = 1;
/// Descriptor flag: the device writes this buffer (otherwise it reads it).
pub const DESC_F_WRITE: u16 = 2;
/// Descriptor flag: the buffer holds an indirect descriptor table.
pub const DESC_F_INDIRECT: u16 = 4;

/// Device status bit: the guest has noticed the device.
pub const STATUS_ACKNOWLEDGE: u8 = 1;
/// Device status bit: the guest knows how to drive the device.
pub const STATUS_DRIVER: u8 = 2;
/// Device status bit: the driver is set up and the device is live.
pub const STATUS_DRIVER_OK: u8 = 4;
/// Device status bit: the feature selection was accepted.
pub const STATUS_FEATURES_OK: u8 = 8;
/// Device status bit: the guest gave up on the device.
pub const STATUS_FAILED: u8 = 0x80;

/// Feature bit: VirtIO 1.x compliant device (feature bit 32).
pub const F_VERSION_1: u64 = 1 << 32;
/// Feature bit: indirect descriptors are supported (feature bit 28).
pub const F_RING_INDIRECT_DESC: u64 = 1 << 28;
/// Feature bit: used/avail event indices are supported (feature bit 29).
pub const F_RING_EVENT_IDX: u64 = 1 << 29;
/// virtio-blk feature bit: the device reports a block size (feature bit 6).
pub const BLK_F_BLK_SIZE: u64 = 1 << 6;

/// Bytes in one sector, the unit of virtio-blk capacity and addressing.
pub const SECTOR_SIZE: u32 = 512;

fn align_up(value: usize, align: usize) -> usize {
    (value + align - 1) & !(align - 1)
}

/// Why a queue size was refused.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LayoutError {
    /// The size is zero, not a power of two, or above `MAX_QUEUE_SIZE`.
    BadQueueSize,
}

/// Where each area of a split virtqueue sits in one allocation.
///
/// Each area is aligned on its own, as in VirtIO 1.x: the descriptor table on
/// 16 bytes, the available ring on 2 and the used ring on 4. The areas follow
/// one another in that order.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct QueueLayout {
    /// Number of descriptors (and ring entries).
    pub queue_size: u16,
    /// Offset of the descriptor table.
    pub desc_offset: usize,
    /// Offset of the available ring.
    pub avail_offset: usize,
    /// Offset of the used ring.
    pub used_offset: usize,
    /// Bytes needed for the whole queue.
    pub total_size: usize,
}

impl QueueLayout {
    /// Lays out a queue of `queue_size` entries.
    pub fn new(queue_size: u16) -> Result<Self, LayoutError> {
        if queue_size == 0 || !queue_size.is_power_of_two() || queue_size > MAX_QUEUE_SIZE {
            return Err(LayoutError::BadQueueSize);
        }
        let n = queue_size as usize;
        let desc_offset = 0;
        let desc_end = desc_offset + 16 * n;
        let avail_offset = align_up(desc_end, AVAIL_ALIGN);
        // flags, idx, ring[n], used_event
        let avail_end = avail_offset + 6 + 2 * n;
        let used_offset = align_up(avail_end, USED_ALIGN);
        // flags, idx, ring[n] of {id, len}, avail_event
        let used_end = used_offset + 6 + 8 * n;
        Ok(Self {
            queue_size,
            desc_offset,
            avail_offset,
            used_offset,
            total_size: used_end,
        })
    }
}

/// One descriptor: a buffer address and length, flags, and the next index.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Descriptor {
    /// Guest-physical address of the buffer.
    pub addr: u64,
    /// Length of the buffer in bytes.
    pub len: u32,
    /// `DESC_F_*` flags.
    pub flags: u16,
    /// Index of the next descriptor when `DESC_F_NEXT` is set.
    pub next: u16,
}

impl Descriptor {
    /// The 16-byte wire form.
    #[must_use]
    pub fn to_bytes(&self) -> [u8; 16] {
        let mut b = [0u8; 16];
        b[0..8].copy_from_slice(&self.addr.to_le_bytes());
        b[8..12].copy_from_slice(&self.len.to_le_bytes());
        b[12..14].copy_from_slice(&self.flags.to_le_bytes());
        b[14..16].copy_from_slice(&self.next.to_le_bytes());
        b
    }

    /// Parses the 16-byte wire form.
    #[must_use]
    pub fn from_bytes(b: &[u8; 16]) -> Self {
        Self {
            addr: u64::from_le_bytes([b[0], b[1], b[2], b[3], b[4], b[5], b[6], b[7]]),
            len: u32::from_le_bytes([b[8], b[9], b[10], b[11]]),
            flags: u16::from_le_bytes([b[12], b[13]]),
            next: u16::from_le_bytes([b[14], b[15]]),
        }
    }
}

/// Reads descriptor `index` from the descriptor table bytes.
#[must_use]
pub fn read_descriptor(table: &[u8], index: u16) -> Option<Descriptor> {
    let at = 16 * index as usize;
    let raw: [u8; 16] = table.get(at..at + 16)?.try_into().ok()?;
    Some(Descriptor::from_bytes(&raw))
}

/// Writes descriptor `index` into the descriptor table bytes.
#[must_use]
pub fn write_descriptor(table: &mut [u8], index: u16, desc: &Descriptor) -> Option<()> {
    let at = 16 * index as usize;
    table
        .get_mut(at..at + 16)?
        .copy_from_slice(&desc.to_bytes());
    Some(())
}

fn read_u16(area: &[u8], at: usize) -> Option<u16> {
    Some(u16::from_le_bytes(area.get(at..at + 2)?.try_into().ok()?))
}

fn read_u32(area: &[u8], at: usize) -> Option<u32> {
    Some(u32::from_le_bytes(area.get(at..at + 4)?.try_into().ok()?))
}

fn write_u16(area: &mut [u8], at: usize, value: u16) -> Option<()> {
    area.get_mut(at..at + 2)?
        .copy_from_slice(&value.to_le_bytes());
    Some(())
}

fn write_u32(area: &mut [u8], at: usize, value: u32) -> Option<()> {
    area.get_mut(at..at + 4)?
        .copy_from_slice(&value.to_le_bytes());
    Some(())
}

/// View of the available ring area: `flags`, `idx`, `ring[n]`, `used_event`.
///
/// The queue size is taken from the area length, which must be `6 + 2n`
/// bytes for some `n`, as produced by [`QueueLayout`].
#[derive(Debug)]
pub struct AvailRing<'a> {
    area: &'a mut [u8],
}

impl<'a> AvailRing<'a> {
    /// Wraps the area, or `None` if its length does not fit a queue size.
    pub fn new(area: &'a mut [u8]) -> Option<Self> {
        if area.len() < 6 || !(area.len() - 6).is_multiple_of(2) {
            return None;
        }
        Some(Self { area })
    }

    /// Number of ring entries.
    #[must_use]
    pub fn entries(&self) -> usize {
        (self.area.len() - 6) / 2
    }

    /// The `flags` field.
    #[must_use]
    pub fn flags(&self) -> u16 {
        read_u16(self.area, 0).unwrap_or(0)
    }

    /// Sets the `flags` field.
    pub fn set_flags(&mut self, flags: u16) {
        let _ = write_u16(self.area, 0, flags);
    }

    /// The free-running index: how many entries the driver has published.
    #[must_use]
    pub fn idx(&self) -> u16 {
        read_u16(self.area, 2).unwrap_or(0)
    }

    /// Sets the index.
    pub fn set_idx(&mut self, idx: u16) {
        let _ = write_u16(self.area, 2, idx);
    }

    /// The descriptor head published at ring slot `slot`.
    #[must_use]
    pub fn ring(&self, slot: usize) -> Option<u16> {
        if slot >= self.entries() {
            return None;
        }
        read_u16(self.area, 4 + 2 * slot)
    }

    /// Publishes descriptor head `head` at ring slot `slot`.
    pub fn set_ring(&mut self, slot: usize, head: u16) -> Option<()> {
        if slot >= self.entries() {
            return None;
        }
        write_u16(self.area, 4 + 2 * slot, head)
    }

    /// The `used_event` field, used only with `F_RING_EVENT_IDX`.
    #[must_use]
    pub fn used_event(&self) -> u16 {
        read_u16(self.area, 4 + 2 * self.entries()).unwrap_or(0)
    }

    /// Sets the `used_event` field.
    pub fn set_used_event(&mut self, value: u16) {
        let at = 4 + 2 * self.entries();
        let _ = write_u16(self.area, at, value);
    }
}

/// One completed request in the used ring.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct UsedElem {
    /// Head index of the completed descriptor chain.
    pub id: u32,
    /// Bytes the device wrote into the chain's writable buffers.
    pub len: u32,
}

/// View of the used ring area: `flags`, `idx`, `ring[n]` of `{id, len}`,
/// `avail_event`.
///
/// The area length must be `6 + 8n` bytes for some `n`.
#[derive(Debug)]
pub struct UsedRing<'a> {
    area: &'a mut [u8],
}

impl<'a> UsedRing<'a> {
    /// Wraps the area, or `None` if its length does not fit a queue size.
    pub fn new(area: &'a mut [u8]) -> Option<Self> {
        if area.len() < 6 || !(area.len() - 6).is_multiple_of(8) {
            return None;
        }
        Some(Self { area })
    }

    /// Number of ring entries.
    #[must_use]
    pub fn entries(&self) -> usize {
        (self.area.len() - 6) / 8
    }

    /// The `flags` field, written by the device.
    #[must_use]
    pub fn flags(&self) -> u16 {
        read_u16(self.area, 0).unwrap_or(0)
    }

    /// The device's free-running index: how many chains it has completed.
    #[must_use]
    pub fn idx(&self) -> u16 {
        read_u16(self.area, 2).unwrap_or(0)
    }

    /// Sets the index. The device does this; this is for tests and fakes.
    pub fn set_idx(&mut self, idx: u16) {
        let _ = write_u16(self.area, 2, idx);
    }

    /// The completion at ring slot `slot`.
    #[must_use]
    pub fn elem(&self, slot: usize) -> Option<UsedElem> {
        if slot >= self.entries() {
            return None;
        }
        let at = 4 + 8 * slot;
        Some(UsedElem {
            id: read_u32(self.area, at)?,
            len: read_u32(self.area, at + 4)?,
        })
    }

    /// Writes a completion at ring slot `slot`. The device does this; this is
    /// for tests and fakes.
    pub fn set_elem(&mut self, slot: usize, elem: UsedElem) -> Option<()> {
        if slot >= self.entries() {
            return None;
        }
        let at = 4 + 8 * slot;
        write_u32(self.area, at, elem.id)?;
        write_u32(self.area, at + 4, elem.len)
    }

    /// The `avail_event` field, used only with `F_RING_EVENT_IDX`.
    #[must_use]
    pub fn avail_event(&self) -> u16 {
        read_u16(self.area, 4 + 8 * self.entries()).unwrap_or(0)
    }
}

/// Why a descriptor chain could not be built or freed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ChainError {
    /// The chain is empty, or its parts and indices disagree in number.
    Shape,
    /// An index is outside the queue, or the table is too short for it.
    BadIndex,
    /// A descriptor in the chain is not allocated, so it cannot be freed.
    NotAllocated,
    /// The chain links back on itself or runs longer than the queue.
    Loop,
}

/// Allocator for the descriptors of one queue, with no heap.
///
/// `CAP` is the largest queue size the allocator supports. A queue of size
/// `n <= CAP` uses the first `n` slots, and the rest are kept as allocated so
/// they are never handed out.
#[derive(Debug)]
pub struct DescAllocator<const CAP: usize> {
    in_use: [bool; CAP],
    queue_size: usize,
}

impl<const CAP: usize> DescAllocator<CAP> {
    /// An allocator for a queue of `queue_size` descriptors, all free.
    pub fn new(queue_size: u16) -> Result<Self, LayoutError> {
        let n = queue_size as usize;
        if queue_size == 0 || !queue_size.is_power_of_two() || n > CAP {
            return Err(LayoutError::BadQueueSize);
        }
        let mut in_use = [false; CAP];
        for slot in in_use.iter_mut().skip(n) {
            *slot = true;
        }
        Ok(Self {
            in_use,
            queue_size: n,
        })
    }

    /// Number of free descriptors.
    #[must_use]
    pub fn free_count(&self) -> usize {
        self.in_use[..self.queue_size]
            .iter()
            .filter(|used| !**used)
            .count()
    }

    /// Takes `out.len()` free descriptors, lowest index first, and writes their
    /// indices into `out`. Returns the head, which is `out[0]`, or `None` if
    /// there are too few free descriptors or `out` is empty. Nothing is taken
    /// on failure.
    pub fn alloc(&mut self, out: &mut [u16]) -> Option<u16> {
        if out.is_empty() || self.free_count() < out.len() {
            return None;
        }
        let mut taken = 0;
        for index in 0..self.queue_size {
            if taken == out.len() {
                break;
            }
            if !self.in_use[index] {
                self.in_use[index] = true;
                out[taken] = index as u16;
                taken += 1;
            }
        }
        Some(out[0])
    }

    /// Frees the chain that starts at `head`, following `DESC_F_NEXT` through
    /// `table`. The chain is checked before anything is freed, so a bad chain
    /// changes nothing. Returns the number of descriptors freed.
    pub fn free_chain(&mut self, table: &[u8], head: u16) -> Result<usize, ChainError> {
        let mut indices = [0u16; CAP];
        let mut count = 0usize;
        let mut index = head;
        loop {
            if index as usize >= self.queue_size {
                return Err(ChainError::BadIndex);
            }
            if count == self.queue_size {
                return Err(ChainError::Loop);
            }
            if !self.in_use[index as usize] {
                return Err(ChainError::NotAllocated);
            }
            indices[count] = index;
            count += 1;
            let desc = read_descriptor(table, index).ok_or(ChainError::BadIndex)?;
            if desc.flags & DESC_F_NEXT == 0 {
                break;
            }
            index = desc.next;
        }
        for &i in &indices[..count] {
            self.in_use[i as usize] = false;
        }
        Ok(count)
    }
}

/// One buffer in a chain to be written: address, length and flags. The
/// `DESC_F_NEXT` flag is set by [`fill_chain`] and need not be given.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ChainPart {
    /// Guest-physical address of the buffer.
    pub addr: u64,
    /// Length in bytes.
    pub len: u32,
    /// `DESC_F_WRITE` and friends, without `DESC_F_NEXT`.
    pub flags: u16,
}

/// Writes a chain into the descriptor table: part `i` goes to `indices[i]`,
/// and each part but the last gets `DESC_F_NEXT` and a `next` index.
pub fn fill_chain(
    table: &mut [u8],
    indices: &[u16],
    parts: &[ChainPart],
) -> Result<(), ChainError> {
    if parts.is_empty() || parts.len() != indices.len() {
        return Err(ChainError::Shape);
    }
    let last = parts.len() - 1;
    for (i, part) in parts.iter().enumerate() {
        let (flags, next) = if i == last {
            (part.flags & !DESC_F_NEXT, 0)
        } else {
            (part.flags | DESC_F_NEXT, indices[i + 1])
        };
        let desc = Descriptor {
            addr: part.addr,
            len: part.len,
            flags,
            next,
        };
        write_descriptor(table, indices[i], &desc).ok_or(ChainError::BadIndex)?;
    }
    Ok(())
}

/// virtio-blk request types.
pub mod blk {
    use super::SECTOR_SIZE;

    /// Read sectors into the data buffer.
    pub const T_IN: u32 = 0;
    /// Write sectors from the data buffer.
    pub const T_OUT: u32 = 1;
    /// Flush the device's write cache.
    pub const T_FLUSH: u32 = 4;

    /// The request completed.
    pub const S_OK: u8 = 0;
    /// The device hit an I/O error.
    pub const S_IOERR: u8 = 1;
    /// The device does not support the request.
    pub const S_UNSUPP: u8 = 2;

    /// Size of the request header.
    pub const HEADER_LEN: usize = 16;
    /// Size of the status trailer.
    pub const STATUS_LEN: u32 = 1;

    /// The 16-byte request header: type, reserved (zero), sector.
    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    pub struct Header {
        /// `T_IN`, `T_OUT` or `T_FLUSH`.
        pub kind: u32,
        /// First sector of the transfer.
        pub sector: u64,
    }

    impl Header {
        /// The wire form.
        #[must_use]
        pub fn to_bytes(&self) -> [u8; HEADER_LEN] {
            let mut b = [0u8; HEADER_LEN];
            b[0..4].copy_from_slice(&self.kind.to_le_bytes());
            b[8..16].copy_from_slice(&self.sector.to_le_bytes());
            b
        }

        /// Parses the wire form.
        #[must_use]
        pub fn from_bytes(b: &[u8; HEADER_LEN]) -> Self {
            Self {
                kind: u32::from_le_bytes([b[0], b[1], b[2], b[3]]),
                sector: u64::from_le_bytes([b[8], b[9], b[10], b[11], b[12], b[13], b[14], b[15]]),
            }
        }
    }

    /// Why a block request was refused before it reached the queue.
    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    pub enum RequestError {
        /// A read or write has no data, zero length, or a length that is not
        /// a whole number of sectors.
        BadLength,
        /// A flush was given a data buffer.
        UnexpectedData,
        /// The transfer runs past the end of the device.
        OutOfRange,
    }

    /// Device configuration of a virtio-blk device.
    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    pub struct Config {
        /// Capacity in 512-byte sectors.
        pub capacity: u64,
        /// Logical block size, if the device reports one.
        pub block_size: Option<u32>,
    }

    impl Config {
        /// Decodes the device config space. Capacity is at offset 0. The block
        /// size at offset 20 is read only when `block_size_negotiated` is set,
        /// which means `BLK_F_BLK_SIZE` was accepted.
        #[must_use]
        pub fn decode(config: &[u8], block_size_negotiated: bool) -> Option<Self> {
            let capacity = u64::from_le_bytes(config.get(0..8)?.try_into().ok()?);
            let block_size = if block_size_negotiated {
                Some(u32::from_le_bytes(config.get(20..24)?.try_into().ok()?))
            } else {
                None
            };
            Some(Self {
                capacity,
                block_size,
            })
        }

        /// Checks that `len` bytes starting at `sector` lie inside the device.
        pub fn check_range(&self, sector: u64, len: u32) -> Result<(), RequestError> {
            let sectors = u64::from(len / SECTOR_SIZE);
            let end = sector
                .checked_add(sectors)
                .ok_or(RequestError::OutOfRange)?;
            if end > self.capacity {
                Err(RequestError::OutOfRange)
            } else {
                Ok(())
            }
        }
    }

    /// The descriptor chain for one request: header, optional data, status.
    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    pub struct Chain {
        parts: [super::ChainPart; 3],
        count: usize,
    }

    impl Chain {
        /// The parts in chain order, to pass to [`super::fill_chain`].
        #[must_use]
        pub fn parts(&self) -> &[super::ChainPart] {
            &self.parts[..self.count]
        }
    }

    /// Builds the chain for a request.
    ///
    /// The header is device-readable. A read's data buffer is device-writable,
    /// a write's is device-readable, and the status byte is device-writable. A
    /// flush has no data buffer, so its chain is two parts. `data` is the
    /// buffer address and length.
    pub fn chain(
        kind: u32,
        data: Option<(u64, u32)>,
        header_addr: u64,
        status_addr: u64,
    ) -> Result<Chain, RequestError> {
        let header = super::ChainPart {
            addr: header_addr,
            len: HEADER_LEN as u32,
            flags: 0,
        };
        let status = super::ChainPart {
            addr: status_addr,
            len: STATUS_LEN,
            flags: super::DESC_F_WRITE,
        };
        match (kind, data) {
            (T_FLUSH, None) => Ok(Chain {
                parts: [header, status, status],
                count: 2,
            }),
            (T_FLUSH, Some(_)) => Err(RequestError::UnexpectedData),
            (T_IN | T_OUT, Some((addr, len))) => {
                if len == 0 || len % SECTOR_SIZE != 0 {
                    return Err(RequestError::BadLength);
                }
                let flags = if kind == T_IN { super::DESC_F_WRITE } else { 0 };
                let data = super::ChainPart { addr, len, flags };
                Ok(Chain {
                    parts: [header, data, status],
                    count: 3,
                })
            }
            _ => Err(RequestError::BadLength),
        }
    }
}

/// The device status steps a driver must take, in order.
///
/// The driver writes status bits one step at a time: ACKNOWLEDGE, then DRIVER,
/// then FEATURES_OK, then DRIVER_OK. A step may be taken only once, and only
/// after the step before it. `FAILED` may be set at any time, after which only a
/// reset (writing zero) is accepted.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct StatusMachine {
    status: u8,
}

/// Why a status step was refused.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StatusError {
    /// The step needs an earlier step that has not happened.
    OutOfOrder,
    /// The step has already happened since the last reset.
    AlreadySet,
    /// The device has failed; only a reset is allowed.
    Failed,
    /// The value is not a single step bit.
    NotAStep,
}

impl Default for StatusMachine {
    fn default() -> Self {
        Self::new()
    }
}

impl StatusMachine {
    /// A device at reset: status zero.
    #[must_use]
    pub const fn new() -> Self {
        Self { status: 0 }
    }

    /// The current status byte.
    #[must_use]
    pub const fn status(&self) -> u8 {
        self.status
    }

    /// True once DRIVER_OK is set, meaning the queues may be used.
    #[must_use]
    pub const fn is_live(&self) -> bool {
        self.status & STATUS_DRIVER_OK != 0 && self.status & STATUS_FAILED == 0
    }

    /// Writes zero: the device is reset and every step must be taken again.
    pub fn reset(&mut self) {
        self.status = 0;
    }

    /// Takes one step. `step` is one of ACKNOWLEDGE, DRIVER, FEATURES_OK,
    /// DRIVER_OK or FAILED.
    pub fn step(&mut self, step: u8) -> Result<(), StatusError> {
        if step == STATUS_FAILED {
            self.status |= STATUS_FAILED;
            return Ok(());
        }
        let prerequisite = match step {
            STATUS_ACKNOWLEDGE => 0,
            STATUS_DRIVER => STATUS_ACKNOWLEDGE,
            STATUS_FEATURES_OK => STATUS_DRIVER,
            STATUS_DRIVER_OK => STATUS_FEATURES_OK,
            _ => return Err(StatusError::NotAStep),
        };
        if self.status & STATUS_FAILED != 0 {
            return Err(StatusError::Failed);
        }
        if self.status & step != 0 {
            return Err(StatusError::AlreadySet);
        }
        if prerequisite != 0 && self.status & prerequisite != prerequisite {
            return Err(StatusError::OutOfOrder);
        }
        self.status |= step;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::vec;

    #[test]
    fn descriptor_round_trips() {
        let d = Descriptor {
            addr: 0x1234_5678_9ABC_DEF0,
            len: 512,
            flags: DESC_F_NEXT | DESC_F_WRITE,
            next: 7,
        };
        let bytes = d.to_bytes();
        assert_eq!(Descriptor::from_bytes(&bytes), d);
        assert_eq!(&bytes[0..8], &0x1234_5678_9ABC_DEF0u64.to_le_bytes());
    }

    #[test]
    fn descriptor_table_access_is_bounds_checked() {
        let mut table = vec![0u8; 32];
        let d = Descriptor {
            addr: 1,
            len: 2,
            flags: 3,
            next: 4,
        };
        assert_eq!(write_descriptor(&mut table, 1, &d), Some(()));
        assert_eq!(read_descriptor(&table, 1), Some(d));
        assert_eq!(write_descriptor(&mut table, 2, &d), None);
        assert_eq!(read_descriptor(&table, 2), None);
    }

    #[test]
    fn layout_for_eight_entries() {
        let l = QueueLayout::new(8).unwrap();
        assert_eq!(l.desc_offset, 0);
        assert_eq!(l.avail_offset, 128);
        assert_eq!(l.used_offset, 152);
        assert_eq!(l.total_size, 222);
        assert_eq!(l.avail_offset % AVAIL_ALIGN, 0);
        assert_eq!(l.used_offset % USED_ALIGN, 0);
    }

    #[test]
    fn layout_for_256_entries() {
        let l = QueueLayout::new(256).unwrap();
        assert_eq!(l.avail_offset, 4096);
        assert_eq!(l.used_offset, 4616);
        assert_eq!(l.total_size, 6670);
    }

    #[test]
    fn layout_refuses_bad_sizes() {
        assert_eq!(QueueLayout::new(0), Err(LayoutError::BadQueueSize));
        assert_eq!(QueueLayout::new(6), Err(LayoutError::BadQueueSize));
        assert_eq!(QueueLayout::new(100), Err(LayoutError::BadQueueSize));
        assert!(QueueLayout::new(MAX_QUEUE_SIZE).is_ok());
        assert!(QueueLayout::new(1).is_ok());
    }

    #[test]
    fn avail_ring_fields_sit_at_spec_offsets() {
        let l = QueueLayout::new(8).unwrap();
        let mut area = vec![0u8; 6 + 2 * l.queue_size as usize];
        let mut ring = AvailRing::new(&mut area).unwrap();
        assert_eq!(ring.entries(), 8);
        ring.set_flags(1);
        ring.set_idx(5);
        ring.set_ring(3, 42).unwrap();
        ring.set_used_event(9);
        assert_eq!(ring.flags(), 1);
        assert_eq!(ring.idx(), 5);
        assert_eq!(ring.ring(3), Some(42));
        assert_eq!(ring.ring(8), None);
        assert_eq!(ring.used_event(), 9);
        assert_eq!(ring.set_ring(8, 1), None);
        // idx is the second field, at byte 2; ring[3] at 4 + 2*3.
        assert_eq!(&area[2..4], &5u16.to_le_bytes());
        assert_eq!(&area[10..12], &42u16.to_le_bytes());
        assert_eq!(&area[20..22], &9u16.to_le_bytes());
    }

    #[test]
    fn used_ring_elements_sit_at_spec_offsets() {
        let l = QueueLayout::new(256).unwrap();
        let mut area = vec![0u8; l.total_size - l.used_offset];
        assert_eq!(area.len(), 6 + 8 * 256);
        let mut ring = UsedRing::new(&mut area).unwrap();
        assert_eq!(ring.entries(), 256);
        ring.set_elem(255, UsedElem { id: 7, len: 513 }).unwrap();
        ring.set_idx(3);
        assert_eq!(ring.elem(255), Some(UsedElem { id: 7, len: 513 }));
        assert_eq!(ring.elem(256), None);
        assert_eq!(ring.idx(), 3);
        assert_eq!(ring.avail_event(), 0);
        // Element 255 starts at 4 + 8*255 = 2044.
        assert_eq!(&area[2044..2048], &7u32.to_le_bytes());
        assert_eq!(&area[2048..2052], &513u32.to_le_bytes());
    }

    #[test]
    fn ring_views_refuse_mismatched_lengths() {
        let mut odd = vec![0u8; 7];
        assert!(AvailRing::new(&mut odd).is_none());
        let mut wrong = vec![0u8; 9];
        assert!(UsedRing::new(&mut wrong).is_none());
    }

    #[test]
    fn allocator_hands_out_and_reuses_chains() {
        let mut alloc = DescAllocator::<8>::new(8).unwrap();
        let mut table = vec![0u8; 16 * 8];
        assert_eq!(alloc.free_count(), 8);

        let mut first = [0u16; 3];
        assert_eq!(alloc.alloc(&mut first), Some(0));
        assert_eq!(first, [0, 1, 2]);
        let mut second = [0u16; 5];
        assert_eq!(alloc.alloc(&mut second), Some(3));
        assert_eq!(alloc.free_count(), 0);
        assert_eq!(alloc.alloc(&mut [0u16; 1]), None);

        // Free the first chain and allocate again: the slots come back.
        let parts = [
            ChainPart {
                addr: 0x1000,
                len: 16,
                flags: 0,
            },
            ChainPart {
                addr: 0x2000,
                len: 512,
                flags: DESC_F_WRITE,
            },
            ChainPart {
                addr: 0x3000,
                len: 1,
                flags: DESC_F_WRITE,
            },
        ];
        fill_chain(&mut table, &first, &parts).unwrap();
        assert_eq!(alloc.free_chain(&table, 0), Ok(3));
        assert_eq!(alloc.free_count(), 3);
        // Freeing again is refused: those descriptors are no longer allocated.
        assert_eq!(alloc.free_chain(&table, 0), Err(ChainError::NotAllocated));
        let mut reuse = [0u16; 2];
        assert_eq!(alloc.alloc(&mut reuse), Some(0));
        assert_eq!(reuse, [0, 1]);
    }

    #[test]
    fn allocator_keeps_slots_beyond_queue_size_out_of_use() {
        let mut alloc = DescAllocator::<8>::new(4).unwrap();
        assert_eq!(alloc.free_count(), 4);
        let mut all = [0u16; 4];
        alloc.alloc(&mut all).unwrap();
        assert_eq!(alloc.alloc(&mut [0u16; 1]), None);
        assert!(DescAllocator::<4>::new(8).is_err());
    }

    #[test]
    fn free_chain_refuses_loops_without_freeing_anything() {
        let mut alloc = DescAllocator::<4>::new(4).unwrap();
        let mut table = vec![0u8; 64];
        let mut idx = [0u16; 2];
        alloc.alloc(&mut idx).unwrap();
        // Descriptor 0 points at 1, and 1 points back at 0 with NEXT set.
        write_descriptor(
            &mut table,
            0,
            &Descriptor {
                addr: 0,
                len: 0,
                flags: DESC_F_NEXT,
                next: 1,
            },
        )
        .unwrap();
        write_descriptor(
            &mut table,
            1,
            &Descriptor {
                addr: 0,
                len: 0,
                flags: DESC_F_NEXT,
                next: 0,
            },
        )
        .unwrap();
        assert_eq!(alloc.free_chain(&table, 0), Err(ChainError::Loop));
        assert_eq!(alloc.free_count(), 2);
    }

    #[test]
    fn fill_chain_links_all_but_the_last_part() {
        let mut table = vec![0u8; 64];
        let parts = [
            ChainPart {
                addr: 10,
                len: 16,
                flags: 0,
            },
            ChainPart {
                addr: 20,
                len: 1,
                flags: DESC_F_WRITE | DESC_F_NEXT,
            },
        ];
        fill_chain(&mut table, &[2, 3], &parts).unwrap();
        let a = read_descriptor(&table, 2).unwrap();
        let b = read_descriptor(&table, 3).unwrap();
        assert_eq!(a.flags, DESC_F_NEXT);
        assert_eq!(a.next, 3);
        assert_eq!(b.flags, DESC_F_WRITE);
        assert_eq!(fill_chain(&mut table, &[1], &parts), Err(ChainError::Shape));
        assert_eq!(fill_chain(&mut table, &[], &[]), Err(ChainError::Shape));
    }

    #[test]
    fn blk_read_chain_marks_data_writable() {
        let c = blk::chain(blk::T_IN, Some((0x2000, 1024)), 0x1000, 0x3000).unwrap();
        let p = c.parts();
        assert_eq!(p.len(), 3);
        assert_eq!(p[0].flags, 0); // header: device reads
        assert_eq!(p[1].flags, DESC_F_WRITE); // data: device writes on IN
        assert_eq!(p[1].len, 1024);
        assert_eq!(p[2].flags, DESC_F_WRITE); // status: device writes
        assert_eq!(p[2].len, 1);
    }

    #[test]
    fn blk_write_chain_marks_data_readable() {
        let c = blk::chain(blk::T_OUT, Some((0x2000, 512)), 0x1000, 0x3000).unwrap();
        assert_eq!(c.parts()[1].flags, 0);
    }

    #[test]
    fn blk_flush_has_no_data_part_and_bad_requests_are_refused() {
        let c = blk::chain(blk::T_FLUSH, None, 0x1000, 0x3000).unwrap();
        assert_eq!(c.parts().len(), 2);
        assert_eq!(
            blk::chain(blk::T_FLUSH, Some((1, 512)), 0, 0),
            Err(blk::RequestError::UnexpectedData)
        );
        assert_eq!(
            blk::chain(blk::T_IN, Some((1, 100)), 0, 0),
            Err(blk::RequestError::BadLength)
        );
        assert_eq!(
            blk::chain(blk::T_OUT, None, 0, 0),
            Err(blk::RequestError::BadLength)
        );
    }

    #[test]
    fn blk_header_bytes_are_little_endian_with_zero_reserved() {
        let h = blk::Header {
            kind: blk::T_OUT,
            sector: 0x0102_0304_0506_0708,
        };
        let b = h.to_bytes();
        assert_eq!(&b[0..4], &[1, 0, 0, 0]);
        assert_eq!(&b[4..8], &[0, 0, 0, 0]);
        assert_eq!(&b[8..16], &[8, 7, 6, 5, 4, 3, 2, 1]);
        assert_eq!(blk::Header::from_bytes(&b), h);
        assert_eq!(blk::S_IOERR, 1);
        assert_eq!(blk::S_UNSUPP, 2);
    }

    #[test]
    fn blk_config_decodes_capacity_and_optional_block_size() {
        let mut cfg = vec![0u8; 24];
        cfg[0..8].copy_from_slice(&2048u64.to_le_bytes());
        cfg[20..24].copy_from_slice(&4096u32.to_le_bytes());
        let with = blk::Config::decode(&cfg, true).unwrap();
        assert_eq!(with.capacity, 2048);
        assert_eq!(with.block_size, Some(4096));
        let without = blk::Config::decode(&cfg, false).unwrap();
        assert_eq!(without.block_size, None);
        assert_eq!(blk::Config::decode(&cfg[..4], false), None);
    }

    #[test]
    fn blk_range_check_refuses_transfers_past_the_end() {
        let cfg = blk::Config {
            capacity: 10,
            block_size: None,
        };
        assert_eq!(cfg.check_range(8, 1024), Ok(()));
        assert_eq!(cfg.check_range(9, 1024), Err(blk::RequestError::OutOfRange));
        assert_eq!(
            cfg.check_range(u64::MAX, 512),
            Err(blk::RequestError::OutOfRange)
        );
    }

    #[test]
    fn status_steps_must_follow_the_order() {
        let mut s = StatusMachine::new();
        assert_eq!(s.step(STATUS_DRIVER), Err(StatusError::OutOfOrder));
        s.step(STATUS_ACKNOWLEDGE).unwrap();
        s.step(STATUS_DRIVER).unwrap();
        // DRIVER_OK before FEATURES_OK is refused.
        assert_eq!(s.step(STATUS_DRIVER_OK), Err(StatusError::OutOfOrder));
        s.step(STATUS_FEATURES_OK).unwrap();
        assert!(!s.is_live());
        s.step(STATUS_DRIVER_OK).unwrap();
        assert!(s.is_live());
        assert_eq!(s.status(), 1 | 2 | 8 | 4);
        assert_eq!(s.step(STATUS_DRIVER), Err(StatusError::AlreadySet));
        assert_eq!(s.step(0x10), Err(StatusError::NotAStep));
    }

    #[test]
    fn failed_device_accepts_only_a_reset() {
        let mut s = StatusMachine::new();
        s.step(STATUS_ACKNOWLEDGE).unwrap();
        s.step(STATUS_FAILED).unwrap();
        assert_eq!(s.step(STATUS_DRIVER), Err(StatusError::Failed));
        assert!(!s.is_live());
        s.reset();
        assert_eq!(s.status(), 0);
        s.step(STATUS_ACKNOWLEDGE).unwrap();
        assert_eq!(s.status(), STATUS_ACKNOWLEDGE);
    }
}
