// SPDX-License-Identifier: MIT OR Apache-2.0
//! tpt-k-boot: UEFI loader (Phase 1).
//!
//! The loader reads `TPTK\KERNEL.ELF` from the boot volume, places its
//! `PT_LOAD` segments at their physical addresses, leaves boot services, and
//! jumps to the entry point. The kernel receives an 80-byte handoff record in
//! `rdi`, which points to the UEFI memory map, the ACPI RSDP and the GOP
//! framebuffer. The layout is defined by `BootInfo` below.

#![no_std]
#![no_main]

use core::convert::Infallible;
use core::ffi::c_void;
use core::fmt::Write as _;
use core::panic::PanicInfo;
use core::ptr;
use core::slice;

use tpt_k_boot_efi::{
    allocate_pages, encode_path, exit_boot_services, find_configuration_table, get_memory_map,
    handle_protocol, locate_handles, pages_for, stall, BootServices, Console, File, GraphicsOutput,
    Handle, LoadedImage, SimpleFileSystem, Status, SystemTable, ACPI_20_TABLE_GUID,
    ALLOCATE_ADDRESS, ALLOCATE_ANY_PAGES, EFI_BUFFER_TOO_SMALL, EFI_INVALID_PARAMETER, EFI_SUCCESS,
    FILE_INFO_GUID, FILE_MODE_READ, GRAPHICS_OUTPUT_GUID, LOADED_IMAGE_GUID, LOADER_DATA,
    SIMPLE_FILE_SYSTEM_GUID,
};
use tpt_k_boot_fs::elf::{Elf, ElfError};

/// Handoff record for the kernel, passed in rdi. The layout is 80 bytes and is
/// mirrored in `tpt-k-kernel-mem::bootmap`; the two must agree.
#[repr(C)]
#[derive(Clone, Copy)]
struct BootInfo {
    magic: u64,
    version: u32,
    reserved: u32,
    map_phys: u64,
    map_bytes: u64,
    descriptor_size: u64,
    rsdp_phys: u64,
    fb_base: u64,
    fb_size: u64,
    fb_width: u32,
    fb_height: u32,
    fb_stride: u32,
    fb_format: u32,
}

/// The record's magic and version.
const BOOT_INFO_MAGIC: u64 = u64::from_le_bytes(*b"TPTKBOOT");
const BOOT_INFO_VERSION: u32 = 2;

/// Pixel formats, as `tpt-k-kernel-mem::bootmap::pixel_format` names them.
const FB_NONE: u32 = 0;
const FB_RGBX: u32 = 1;
const FB_BGRX: u32 = 2;
const FB_OTHER: u32 = 3;

/// The framebuffer the loader found, or `None` if the firmware has no GOP.
#[derive(Clone, Copy)]
struct Framebuffer {
    base: u64,
    size: u64,
    width: u32,
    height: u32,
    stride: u32,
    format: u32,
}

/// Finds the first graphics output device and reads its current mode.
///
/// # Safety
///
/// `bs` must be live boot services.
unsafe fn find_framebuffer(bs: *mut BootServices) -> Option<Framebuffer> {
    // SAFETY: the caller guarantees live boot services. The protocol pointers
    // come from firmware and stay valid while their handles do.
    unsafe {
        let mut handles = [ptr::null_mut(); 8];
        let count = locate_handles(bs, &GRAPHICS_OUTPUT_GUID, &mut handles).ok()?;
        for &handle in &handles[..count] {
            let gop: *mut GraphicsOutput = match handle_protocol(bs, handle, &GRAPHICS_OUTPUT_GUID)
            {
                Ok(gop) => gop,
                Err(_) => continue,
            };
            let mode = (*gop).mode;
            if mode.is_null() {
                continue;
            }
            let info = (*mode).info;
            if info.is_null() || (*mode).frame_buffer_base == 0 {
                continue;
            }
            let format = match (*info).pixel_format {
                0 => FB_RGBX,
                1 => FB_BGRX,
                _ => FB_OTHER,
            };
            return Some(Framebuffer {
                base: (*mode).frame_buffer_base,
                size: (*mode).frame_buffer_size as u64,
                width: (*info).horizontal_resolution,
                height: (*info).vertical_resolution,
                stride: (*info).pixels_per_scan_line,
                format,
            });
        }
        None
    }
}

/// The framebuffer fields of a record that has none.
const NO_FRAMEBUFFER: Framebuffer = Framebuffer {
    base: 0,
    size: 0,
    width: 0,
    height: 0,
    stride: 0,
    format: FB_NONE,
};

/// Kernel image path on the boot volume (8.3 names, so FAT needs no long names).
const KERNEL_PATH: &str = "TPTK\\KERNEL.ELF";

const PAGE_SIZE: u64 = 4096;
/// How many times to retry `ExitBootServices` when the map key goes stale.
const EXIT_ATTEMPTS: usize = 3;
/// Status returned to firmware when the loader fails.
const EFI_LOAD_ERROR: Status = (1 << (usize::BITS - 1)) | 1;

/// A failure before the handoff, with the firmware status when there is one.
#[derive(Debug, Clone, Copy)]
struct Failure {
    what: &'static str,
    status: Status,
}

impl Failure {
    const fn new(what: &'static str, status: Status) -> Self {
        Self { what, status }
    }
}

fn check(status: Status, what: &'static str) -> Result<(), Failure> {
    if status == EFI_SUCCESS {
        Ok(())
    } else {
        Err(Failure::new(what, status))
    }
}

fn elf_message(error: ElfError) -> &'static str {
    match error {
        ElfError::TooShort => "kernel image is truncated",
        ElfError::BadMagic | ElfError::NotElf64 | ElfError::NotLittleEndian => {
            "kernel image is not ELF64"
        }
        ElfError::NotExecutable => "kernel image is not an executable",
        ElfError::WrongMachine => "kernel image is not x86_64",
        ElfError::BadSegment => "kernel image has a bad program header",
        ElfError::EntryOutsideLoad => "kernel entry point is outside its segments",
    }
}

/// UEFI entry point. The firmware calls it once with the image handle and the
/// system table, which stay valid until `ExitBootServices`.
/// # Safety
///
/// Called by firmware with the image handle and system table it passed to the
/// image. Both are valid until `ExitBootServices`.
#[unsafe(no_mangle)]
pub unsafe extern "efiapi" fn efi_main(image: Handle, system_table: *mut SystemTable) -> Status {
    if system_table.is_null() {
        return EFI_INVALID_PARAMETER;
    }
    // SAFETY: firmware passes a valid system table (checked non-null above).
    // Its console and boot-services pointers are valid until ExitBootServices,
    // and this function runs before that call.
    let (mut console, boot_services) = unsafe {
        (
            Console::new((*system_table).con_out),
            (*system_table).boot_services,
        )
    };
    // SAFETY: as above. The loader runs single-threaded, and this is the only
    // call that leaves boot services.
    match unsafe { load_and_jump(image, system_table, &mut console) } {
        Ok(never) => match never {},
        Err(failure) => {
            let _ = writeln!(
                console,
                "tpt-k boot: {} (status {:#x})",
                failure.what, failure.status
            );
            // SAFETY: boot services have not been exited on this path.
            unsafe { stall(boot_services, 5_000_000) };
            EFI_LOAD_ERROR
        }
    }
}

/// Opens the kernel file on the volume that `device` exposes. Returns the file
/// handle, or the firmware status if the handle has no file system or no kernel.
///
/// # Safety
///
/// `bs` must be live boot services. `device` must be a handle from firmware.
unsafe fn open_kernel(bs: *mut BootServices, device: Handle) -> Result<*mut File, Status> {
    // SAFETY: the caller guarantees live boot services. Each protocol pointer
    // comes from firmware and stays valid while its handle does, and the
    // root directory is closed before this returns.
    unsafe {
        let fs: *mut SimpleFileSystem = handle_protocol(bs, device, &SIMPLE_FILE_SYSTEM_GUID)?;
        let mut root: *mut File = ptr::null_mut();
        let status = ((*fs).open_volume)(fs, &mut root);
        if status != EFI_SUCCESS {
            return Err(status);
        }
        let mut path_buf = [0u16; 32];
        let path = encode_path(KERNEL_PATH, &mut path_buf).ok_or(EFI_INVALID_PARAMETER)?;
        let mut file: *mut File = ptr::null_mut();
        let status = ((*root).open)(root, &mut file, path.as_ptr(), FILE_MODE_READ, 0);
        let _ = ((*root).close)(root);
        if status == EFI_SUCCESS {
            Ok(file)
        } else {
            Err(status)
        }
    }
}

/// Loads the kernel and jumps to it. Returns only on failure.
///
/// # Safety
///
/// `image` and `system_table` must be the arguments firmware passed to
/// `efi_main`, and boot services must still be active.
unsafe fn load_and_jump(
    image: Handle,
    system_table: *mut SystemTable,
    console: &mut Console,
) -> Result<Infallible, Failure> {
    // SAFETY: the caller guarantees the table is live and boot services are
    // active. Pointers read from protocols are valid while the protocol's
    // handle is, and every call below is on a live protocol.
    unsafe {
        let bs: *mut BootServices = (*system_table).boot_services;
        let _ = writeln!(console, "tpt-k boot: loading {KERNEL_PATH}");

        // The kernel lives on a volume with a file system. The image's own device
        // is tried first. Partitioned disks put the file system on a child
        // handle, so then every file-system handle is tried in turn.
        let loaded: *mut LoadedImage = handle_protocol(bs, image, &LOADED_IMAGE_GUID)
            .map_err(|status| Failure::new("no loaded-image protocol", status))?;
        let mut file = open_kernel(bs, (*loaded).device_handle).ok();
        if file.is_none() {
            let mut candidates = [ptr::null_mut::<c_void>(); 32];
            let count = locate_handles(bs, &SIMPLE_FILE_SYSTEM_GUID, &mut candidates)
                .map_err(|status| Failure::new("cannot list file systems", status))?;
            for &candidate in &candidates[..count] {
                if let Ok(opened) = open_kernel(bs, candidate) {
                    file = Some(opened);
                    break;
                }
            }
        }
        let file = file.ok_or(Failure::new("kernel file not found on any volume", 0))?;

        // The kernel file size from the file info.
        let mut info = [0u8; 256];
        let mut info_size = info.len();
        check(
            ((*file).get_info)(
                file,
                &FILE_INFO_GUID,
                &mut info_size,
                info.as_mut_ptr().cast::<c_void>(),
            ),
            "cannot read the kernel file size",
        )?;
        let mut size_bytes = [0u8; 8];
        size_bytes.copy_from_slice(&info[8..16]); // EFI_FILE_INFO.FileSize
        let file_size = u64::from_le_bytes(size_bytes);
        let file_len =
            usize::try_from(file_size).map_err(|_| Failure::new("kernel file is too large", 0))?;

        // Read the whole file into loader-owned pages.
        let buffer = allocate_pages(bs, ALLOCATE_ANY_PAGES, LOADER_DATA, pages_for(file_size), 0)
            .map_err(|status| Failure::new("no memory for the kernel image", status))?;
        let mut read = file_len;
        check(
            ((*file).read)(file, &mut read, buffer as *mut c_void),
            "cannot read the kernel file",
        )?;
        if read != file_len {
            return Err(Failure::new("kernel file read was short", 0));
        }
        let _ = ((*file).close)(file);
        // SAFETY: `buffer` holds `file_len` bytes that were just read, in pages
        // this loader owns and does not free.
        let image_bytes = slice::from_raw_parts(buffer as *const u8, file_len);

        // Place the segments. Compute the span first, so one AllocateAddress
        // call covers it even when two segments share a page.
        let elf = Elf::parse(image_bytes).map_err(|e| Failure::new(elf_message(e), 0))?;
        let mut low = u64::MAX;
        let mut high = 0u64;
        for segment in elf.load_segments() {
            let segment = segment.map_err(|e| Failure::new(elf_message(e), 0))?;
            low = low.min(segment.paddr);
            high = high.max(segment.paddr + segment.mem_size);
        }
        if high == 0 {
            return Err(Failure::new("kernel image has no loadable segment", 0));
        }
        let base = low & !(PAGE_SIZE - 1);
        let span = high - base;
        allocate_pages(bs, ALLOCATE_ADDRESS, LOADER_DATA, pages_for(span), base)
            .map_err(|status| Failure::new("kernel load address is not free", status))?;
        for segment in elf.load_segments() {
            let segment = segment.map_err(|e| Failure::new(elf_message(e), 0))?;
            // SAFETY: the source range is inside `image_bytes`, checked by
            // `Elf::parse` and `load_segments`. The destination was allocated
            // above and covers `paddr..paddr + mem_size`. The two buffers are
            // distinct allocations, so the copy does not overlap.
            ptr::copy_nonoverlapping(
                image_bytes.as_ptr().add(segment.file_offset as usize),
                segment.paddr as *mut u8,
                segment.file_size as usize,
            );
            // SAFETY: the zeroed range lies inside the allocation (see above).
            ptr::write_bytes(
                (segment.paddr + segment.file_size) as *mut u8,
                0,
                (segment.mem_size - segment.file_size) as usize,
            );
        }
        let entry = elf.entry();
        let _ = writeln!(
            console,
            "tpt-k boot: entry {entry:#x}, leaving boot services"
        );

        // Firmware facts the kernel cannot read once boot services are gone. Both
        // are optional: a missing RSDP or GOP is recorded as zero, not a failure.
        // Neither lookup allocates, so the map key below stays valid.
        let rsdp_phys = find_configuration_table(system_table, &ACPI_20_TABLE_GUID)
            .map_or(0, |table| table as u64);
        let framebuffer = find_framebuffer(bs).unwrap_or(NO_FRAMEBUFFER);

        // The memory map. Allocate its buffer before the final read, because an
        // allocation after `GetMemoryMap` makes the key stale.
        let (probe_status, probe) = get_memory_map(bs, &mut []);
        if probe_status != EFI_BUFFER_TOO_SMALL {
            return Err(Failure::new("cannot size the memory map", probe_status));
        }
        // The handoff record. Its page is allocated before the map is read, so the
        // map already counts it as in use.
        let info_addr = allocate_pages(bs, ALLOCATE_ANY_PAGES, LOADER_DATA, 1, 0)
            .map_err(|status| Failure::new("no memory for the handoff record", status))?;
        let map_bytes = probe.size + 16 * probe.descriptor_size;
        let map_pages = pages_for(map_bytes as u64);
        let map_addr = allocate_pages(bs, ALLOCATE_ANY_PAGES, LOADER_DATA, map_pages, 0)
            .map_err(|status| Failure::new("no memory for the memory map", status))?;
        let map = slice::from_raw_parts_mut(map_addr as *mut u8, map_pages * PAGE_SIZE as usize);

        let mut last_status = EFI_SUCCESS;
        let mut map_info = None;
        for _ in 0..EXIT_ATTEMPTS {
            let (status, info) = get_memory_map(bs, map);
            check(status, "cannot read the memory map")?;
            map_info = Some(info);
            last_status = exit_boot_services(bs, image, info.key);
            if last_status == EFI_SUCCESS {
                break;
            }
        }
        check(last_status, "cannot leave boot services")?;
        let map_info = map_info.ok_or(Failure::new("no memory map", 0))?;

        // Fill in the record. Writing to our own page allocates nothing.
        (info_addr as *mut BootInfo).write(BootInfo {
            magic: BOOT_INFO_MAGIC,
            version: BOOT_INFO_VERSION,
            reserved: 0,
            map_phys: map_addr,
            map_bytes: map_info.size as u64,
            descriptor_size: map_info.descriptor_size as u64,
            rsdp_phys,
            fb_base: framebuffer.base,
            fb_size: framebuffer.size,
            fb_width: framebuffer.width,
            fb_height: framebuffer.height,
            fb_stride: framebuffer.stride,
            fb_format: framebuffer.format,
        });

        // Boot services are gone: no console output from here on. The kernel
        // takes its handoff argument in rdi, and the SysV ABI is what the
        // kernel's `_start` expects.
        let entry_addr = usize::try_from(entry).map_err(|_| Failure::new("entry address", 0))?;
        let kernel: extern "sysv64" fn(u64) -> ! = core::mem::transmute(entry_addr);
        kernel(info_addr)
    }
}

#[panic_handler]
fn panic(_info: &PanicInfo) -> ! {
    loop {
        core::hint::spin_loop();
    }
}
