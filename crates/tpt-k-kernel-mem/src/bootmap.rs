// SPDX-License-Identifier: MIT OR Apache-2.0
//! The boot handoff: the record the loader passes to the kernel, and the UEFI
//! memory map it points at. The kernel builds its frame allocator from the map.
//!
//! The record layout is mirrored in the boot loader (`tpt-k-boot`), which writes
//! it. The two must agree. The layout is 40 bytes, little-endian.

use core::ptr;

use crate::{Frame, FrameAllocator, FRAME_SIZE};

/// The record's magic: the ASCII bytes `TPTKBOOT`.
pub const MAGIC: u64 = u64::from_le_bytes(*b"TPTKBOOT");

/// The record's version. The kernel refuses other versions.
pub const VERSION: u32 = 1;

/// The UEFI memory type of conventional (free) memory.
const EFI_CONVENTIONAL_MEMORY: u32 = 7;

/// Smallest possible UEFI memory descriptor.
const MIN_DESCRIPTOR_SIZE: u64 = 40;

/// The handoff record.
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BootInfo {
    /// Must be [`MAGIC`].
    pub magic: u64,
    /// Must be [`VERSION`].
    pub version: u32,
    /// Reserved, zero.
    pub reserved: u32,
    /// Physical address of the memory map the loader read.
    pub map_phys: u64,
    /// Bytes of the map that hold descriptors.
    pub map_bytes: u64,
    /// Bytes from one descriptor to the next.
    pub descriptor_size: u64,
}

const _: () = assert!(core::mem::size_of::<BootInfo>() == 40);

/// Reads the handoff record at `address`, or returns `None` if it is absent or
/// has the wrong magic or version.
///
/// # Safety
///
/// `address` must be readable for 40 bytes. It is either zero (no record), or
/// the address of a record the loader wrote.
pub unsafe fn read(address: u64) -> Option<BootInfo> {
    if address == 0 {
        return None;
    }
    // SAFETY: the caller guarantees 40 readable bytes at `address`.
    let info = unsafe { ptr::read_unaligned(address as *const BootInfo) };
    (info.magic == MAGIC && info.version == VERSION).then_some(info)
}

/// Calls `visit(start, pages)` for every conventional-memory region in the map.
///
/// # Safety
///
/// The map must be readable for `info.map_bytes` bytes at `info.map_phys`.
pub unsafe fn for_each_conventional(info: &BootInfo, mut visit: impl FnMut(u64, u64)) {
    if info.descriptor_size < MIN_DESCRIPTOR_SIZE {
        return;
    }
    let count = info.map_bytes / info.descriptor_size;
    for index in 0..count {
        let base = info.map_phys + index * info.descriptor_size;
        // SAFETY: `index < count`, so the descriptor lies inside the map, which
        // the caller guarantees is readable.
        let (kind, start, pages) = unsafe {
            (
                ptr::read_unaligned(base as *const u32),
                ptr::read_unaligned((base + 8) as *const u64),
                ptr::read_unaligned((base + 24) as *const u64),
            )
        };
        if kind == EFI_CONVENTIONAL_MEMORY {
            visit(start, pages);
        }
    }
}

/// Builds a frame allocator over the first `frames` frames of physical memory.
/// Every frame starts in use, and the frames of conventional memory in the map
/// are freed. Frames the map does not list stay in use: firmware, the loader,
/// the kernel image and anything else the loader allocated.
///
/// Returns `None` if `bits` is too short for `frames`.
///
/// # Safety
///
/// The map must be readable (see [`for_each_conventional`]).
pub unsafe fn allocator_from_map<'a>(
    info: &BootInfo,
    frames: usize,
    bits: &'a mut [u64],
) -> Option<FrameAllocator<'a>> {
    let mut allocator = FrameAllocator::new(0, frames, bits)?;
    for index in 0..frames {
        let frame = Frame::from_address(index as u64 * FRAME_SIZE)?;
        // Every frame is free after `new`, so reserving each one cannot fail.
        let _ = allocator.reserve(frame);
    }
    // SAFETY: forwarded from the caller.
    unsafe {
        for_each_conventional(info, |start, pages| {
            for page in 0..pages {
                if let Some(frame) = Frame::from_address(start + page * FRAME_SIZE) {
                    // Frames past the managed range are refused, and ignored.
                    let _ = allocator.free(frame);
                }
            }
        });
    }
    Some(allocator)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Writes one 40-byte descriptor.
    fn descriptor(map: &mut [u8], index: usize, kind: u32, start: u64, pages: u64) {
        let base = index * 40;
        map[base..base + 4].copy_from_slice(&kind.to_le_bytes());
        map[base + 8..base + 16].copy_from_slice(&start.to_le_bytes());
        map[base + 24..base + 32].copy_from_slice(&pages.to_le_bytes());
    }

    fn info_for(map: &[u8]) -> BootInfo {
        BootInfo {
            magic: MAGIC,
            version: VERSION,
            reserved: 0,
            map_phys: map.as_ptr() as u64,
            map_bytes: map.len() as u64,
            descriptor_size: 40,
        }
    }

    #[test]
    fn record_layout_and_magic() {
        assert_eq!(MAGIC.to_le_bytes(), *b"TPTKBOOT");
        let mut bytes = [0u8; 40];
        bytes[..8].copy_from_slice(&MAGIC.to_le_bytes());
        bytes[8..12].copy_from_slice(&VERSION.to_le_bytes());
        // SAFETY: the array is 40 readable bytes.
        let info = unsafe { read(bytes.as_ptr() as u64) }.unwrap();
        assert_eq!(info.version, VERSION);
        bytes[8..12].copy_from_slice(&2u32.to_le_bytes());
        // SAFETY: as above.
        assert_eq!(unsafe { read(bytes.as_ptr() as u64) }, None);
        // SAFETY: zero means no record.
        assert_eq!(unsafe { read(0) }, None);
    }

    #[test]
    fn only_conventional_regions_are_freed() {
        // Frames 0..8 are 4 KiB each. Conventional: frame 1, and frames 3 and 4.
        // Loader data (type 2) at frame 2 and boot services (type 4) at frame 5.
        let mut map = [0u8; 4 * 40];
        descriptor(&mut map, 0, 7, 0x1000, 1);
        descriptor(&mut map, 1, 2, 0x2000, 1);
        descriptor(&mut map, 2, 7, 0x3000, 2);
        descriptor(&mut map, 3, 4, 0x5000, 1);
        let info = info_for(&map);
        let mut bits = [0u64; 1];
        // SAFETY: the map array is readable for its length.
        let mut allocator = unsafe { allocator_from_map(&info, 8, &mut bits) }.unwrap();
        assert_eq!(allocator.free_frames(), 3);
        assert!(!allocator.is_allocated(Frame::from_address(0x1000).unwrap()));
        assert!(allocator.is_allocated(Frame::from_address(0x2000).unwrap()));
        assert!(!allocator.is_allocated(Frame::from_address(0x4000).unwrap()));
        assert!(allocator.is_allocated(Frame::from_address(0x5000).unwrap()));
        assert!(allocator.is_allocated(Frame::from_address(0x0).unwrap()));
        let frame = allocator.allocate().unwrap();
        assert!([0x1000, 0x3000, 0x4000].contains(&frame.start()));
    }

    #[test]
    fn short_descriptors_are_ignored() {
        let map = [0u8; 40];
        let info = BootInfo {
            descriptor_size: 8,
            ..info_for(&map)
        };
        let mut seen = 0;
        // SAFETY: the map is readable.
        unsafe { for_each_conventional(&info, |_, _| seen += 1) };
        assert_eq!(seen, 0);
    }
}
