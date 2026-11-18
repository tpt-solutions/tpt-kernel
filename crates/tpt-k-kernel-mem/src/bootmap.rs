// SPDX-License-Identifier: MIT OR Apache-2.0
//! The boot handoff: the record the loader passes to the kernel, and the UEFI
//! memory map it points at. The kernel builds its frame allocator from the map.
//!
//! The record layout is mirrored in the boot loader (`tpt-k-boot`), which writes
//! it. The two must agree. The layout is 80 bytes, little-endian.
//!
//! Versioning rule: the loader and the kernel are built from the same tree, so
//! the kernel accepts exactly one version. Any change to the layout bumps
//! [`VERSION`], and the kernel refuses every other value.

use core::ptr;

use crate::{Frame, FrameAllocator, FRAME_SIZE};

/// The record's magic: the ASCII bytes `TPTKBOOT`.
pub const MAGIC: u64 = u64::from_le_bytes(*b"TPTKBOOT");

/// The record's version. The kernel refuses other versions.
pub const VERSION: u32 = 2;

/// Pixel format of the framebuffer, as the loader reports it.
pub mod pixel_format {
    /// No framebuffer was found.
    pub const NONE: u32 = 0;
    /// 32-bit pixels, red in the low byte (`PixelRedGreenBlueReserved8BitPerColor`).
    pub const RGBX: u32 = 1;
    /// 32-bit pixels, blue in the low byte (`PixelBlueGreenRedReserved8BitPerColor`).
    pub const BGRX: u32 = 2;
    /// Any other layout. The masks are not passed on, so use only the address.
    pub const OTHER: u32 = 3;
}

/// The UEFI memory type of conventional (free) memory.
const EFI_CONVENTIONAL_MEMORY: u32 = 7;

/// The first megabyte holds the real-mode vectors, the BIOS data area and firmware
/// data. Firmware may list it as conventional memory, so frames below this address
/// are never freed into the allocator, and never handed out.
pub const LOW_MEMORY_END: u64 = 0x10_0000;

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
    /// Physical address of the ACPI 2.0+ RSDP, or zero if firmware has none.
    pub rsdp_phys: u64,
    /// Physical address of the linear framebuffer, or zero if there is none.
    pub fb_base: u64,
    /// Bytes of the framebuffer.
    pub fb_size: u64,
    /// Visible width in pixels.
    pub fb_width: u32,
    /// Visible height in pixels.
    pub fb_height: u32,
    /// Pixels from one row to the next (may exceed `fb_width`).
    pub fb_stride: u32,
    /// One of the [`pixel_format`] values.
    pub fb_format: u32,
}

const _: () = assert!(core::mem::size_of::<BootInfo>() == 80);

/// Reads the handoff record at `address`, or returns `None` if it is absent or
/// has the wrong magic or version.
///
/// # Safety
///
/// `address` must be zero (no record), or readable for `size_of::<BootInfo>()`
/// (80) bytes and hold a record the loader wrote. Every field is an integer, so
/// any bit pattern is a valid `BootInfo`, and the magic and version are checked
/// after the read.
pub unsafe fn read(address: u64) -> Option<BootInfo> {
    if address == 0 {
        return None;
    }
    // SAFETY: `address` is non-zero, so the caller's contract gives 80 readable
    // bytes there. `read_unaligned` makes no alignment demand on the record.
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
                let address = start + page * FRAME_SIZE;
                if address < LOW_MEMORY_END {
                    continue;
                }
                if let Some(frame) = Frame::from_address(address) {
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
            rsdp_phys: 0,
            fb_base: 0,
            fb_size: 0,
            fb_width: 0,
            fb_height: 0,
            fb_stride: 0,
            fb_format: pixel_format::NONE,
        }
    }

    #[test]
    fn record_layout_and_magic() {
        assert_eq!(MAGIC.to_le_bytes(), *b"TPTKBOOT");
        let mut bytes = [0u8; 80];
        bytes[..8].copy_from_slice(&MAGIC.to_le_bytes());
        bytes[8..12].copy_from_slice(&VERSION.to_le_bytes());
        // SAFETY: the array is 80 readable bytes.
        let info = unsafe { read(bytes.as_ptr() as u64) }.unwrap();
        assert_eq!(info.version, VERSION);
        // Version 1 (the 40-byte record) is refused.
        bytes[8..12].copy_from_slice(&1u32.to_le_bytes());
        // SAFETY: as above.
        assert_eq!(unsafe { read(bytes.as_ptr() as u64) }, None);
        // SAFETY: zero means no record.
        assert_eq!(unsafe { read(0) }, None);
    }

    #[test]
    fn firmware_fields_are_read_back() {
        let mut bytes = [0u8; 80];
        bytes[..8].copy_from_slice(&MAGIC.to_le_bytes());
        bytes[8..12].copy_from_slice(&VERSION.to_le_bytes());
        bytes[40..48].copy_from_slice(&0xE_0000u64.to_le_bytes()); // rsdp
        bytes[48..56].copy_from_slice(&0x8000_0000u64.to_le_bytes()); // fb base
        bytes[56..64].copy_from_slice(&0x0080_0000u64.to_le_bytes()); // fb size
        bytes[64..68].copy_from_slice(&1024u32.to_le_bytes()); // width
        bytes[68..72].copy_from_slice(&768u32.to_le_bytes()); // height
        bytes[72..76].copy_from_slice(&1024u32.to_le_bytes()); // stride
        bytes[76..80].copy_from_slice(&pixel_format::BGRX.to_le_bytes());
        // SAFETY: the array is 80 readable bytes.
        let info = unsafe { read(bytes.as_ptr() as u64) }.unwrap();
        assert_eq!(info.rsdp_phys, 0xE_0000);
        assert_eq!((info.fb_base, info.fb_size), (0x8000_0000, 0x0080_0000));
        assert_eq!(
            (info.fb_width, info.fb_height, info.fb_stride),
            (1024, 768, 1024)
        );
        assert_eq!(info.fb_format, pixel_format::BGRX);
    }

    #[test]
    fn only_conventional_regions_above_the_low_megabyte_are_freed() {
        // Frames from the 1 MiB line: conventional at 0x101000, loader data
        // (type 2) at 0x102000, conventional at 0x103000 (two pages), boot
        // services (type 4) at 0x105000. A conventional frame below 1 MiB, at
        // 0x1000, is firmware's and must stay in use.
        let mut map = [0u8; 5 * 40];
        descriptor(&mut map, 0, 7, 0x1000, 1);
        descriptor(&mut map, 1, 7, 0x10_1000, 1);
        descriptor(&mut map, 2, 2, 0x10_2000, 1);
        descriptor(&mut map, 3, 7, 0x10_3000, 2);
        descriptor(&mut map, 4, 4, 0x10_5000, 1);
        let info = info_for(&map);
        let frames = 0x106;
        let mut bits = [0u64; 5]; // 262 frames need five words
                                  // SAFETY: the map array is readable for its length.
        let mut allocator = unsafe { allocator_from_map(&info, frames, &mut bits) }.unwrap();
        assert_eq!(allocator.free_frames(), 3);
        assert!(allocator.is_allocated(Frame::from_address(0x1000).unwrap()));
        assert!(!allocator.is_allocated(Frame::from_address(0x10_1000).unwrap()));
        assert!(allocator.is_allocated(Frame::from_address(0x10_2000).unwrap()));
        assert!(!allocator.is_allocated(Frame::from_address(0x10_3000).unwrap()));
        assert!(!allocator.is_allocated(Frame::from_address(0x10_4000).unwrap()));
        assert!(allocator.is_allocated(Frame::from_address(0x10_5000).unwrap()));
        assert!(allocator.is_allocated(Frame::from_address(0x0).unwrap()));
        let frame = allocator.allocate().unwrap();
        assert!([0x10_1000, 0x10_3000, 0x10_4000].contains(&frame.start()));
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
