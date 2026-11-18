// SPDX-License-Identifier: MIT OR Apache-2.0
//! UEFI ABI definitions and the calls the tpt-k loader needs (UEFI 2.10).
//!
//! Only the table entries the loader calls have typed function pointers. Entries
//! before them are reserved slots of the right size, so the offsets match the
//! specification. Entries after `Stall` are not declared, because the loader
//! never reads past it.
//!
//! The raw calls are `unsafe`. Each one states the firmware state it needs. The
//! loader is the only caller and runs single-threaded before
//! `ExitBootServices`.

#![no_std]

use core::ffi::c_void;
use core::ptr;

/// A UEFI status code. The high bit marks errors.
pub type Status = usize;

/// Success.
pub const EFI_SUCCESS: Status = 0;
const ERROR: Status = 1 << (usize::BITS - 1);
/// The buffer is too small. The required size is written back.
pub const EFI_BUFFER_TOO_SMALL: Status = ERROR | 5;
/// The map key is stale, or an argument is invalid.
pub const EFI_INVALID_PARAMETER: Status = ERROR | 2;

/// An opaque firmware handle.
pub type Handle = *mut c_void;

/// `EFI_GUID`.
#[repr(C)]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Guid {
    /// First 32 bits.
    pub data1: u32,
    /// Next 16 bits.
    pub data2: u16,
    /// Next 16 bits.
    pub data3: u16,
    /// Final 8 bytes.
    pub data4: [u8; 8],
}

/// `EFI_LOADED_IMAGE_PROTOCOL`.
pub const LOADED_IMAGE_GUID: Guid = Guid {
    data1: 0x5B1B_31A1,
    data2: 0x9562,
    data3: 0x11D2,
    data4: [0x8E, 0x3F, 0x00, 0xA0, 0xC9, 0x69, 0x72, 0x3B],
};

/// `EFI_SIMPLE_FILE_SYSTEM_PROTOCOL`.
pub const SIMPLE_FILE_SYSTEM_GUID: Guid = Guid {
    data1: 0x964E_5B22,
    data2: 0x6459,
    data3: 0x11D2,
    data4: [0x8E, 0x39, 0x00, 0xA0, 0xC9, 0x69, 0x72, 0x3B],
};

/// `EFI_FILE_INFO` identifier for `GetInfo`.
pub const FILE_INFO_GUID: Guid = Guid {
    data1: 0x0957_6E92,
    data2: 0x6D3F,
    data3: 0x11D2,
    data4: [0x8E, 0x39, 0x00, 0xA0, 0xC9, 0x69, 0x72, 0x3B],
};

/// Memory type for loader-owned data (`EfiLoaderData`).
pub const LOADER_DATA: u32 = 2;

/// `AllocateType` value for "any free pages".
pub const ALLOCATE_ANY_PAGES: u32 = 0;
/// `AllocateType` value for "these exact pages".
pub const ALLOCATE_ADDRESS: u32 = 2;

/// `EFI_FILE_MODE_READ`.
pub const FILE_MODE_READ: u64 = 1;

/// `EFI_GRAPHICS_OUTPUT_PROTOCOL`.
pub const GRAPHICS_OUTPUT_GUID: Guid = Guid {
    data1: 0x9042_A9DE,
    data2: 0x23DC,
    data3: 0x4A38,
    data4: [0x96, 0xFB, 0x7A, 0xDE, 0xD0, 0x80, 0x51, 0x6A],
};

/// `EFI_ACPI_20_TABLE_GUID`: the configuration-table entry for the ACPI 2.0+ RSDP.
pub const ACPI_20_TABLE_GUID: Guid = Guid {
    data1: 0x8868_E871,
    data2: 0xE4F1,
    data3: 0x11D3,
    data4: [0xBC, 0x22, 0x00, 0x80, 0xC7, 0x3C, 0x88, 0x81],
};

/// `EFI_GRAPHICS_OUTPUT_PROTOCOL`, up to `Mode`.
#[repr(C)]
pub struct GraphicsOutput {
    /// `QueryMode`. Not used.
    pub query_mode: *const c_void,
    /// `SetMode`. Not used.
    pub set_mode: *const c_void,
    /// `Blt`. Not used.
    pub blt: *const c_void,
    /// The current mode.
    pub mode: *const GraphicsOutputMode,
}

/// `EFI_GRAPHICS_OUTPUT_PROTOCOL_MODE`.
#[repr(C)]
pub struct GraphicsOutputMode {
    /// Number of modes the device supports.
    pub max_mode: u32,
    /// The current mode number.
    pub mode: u32,
    /// Information about the current mode.
    pub info: *const GraphicsOutputModeInfo,
    /// Size of `info` in bytes.
    pub size_of_info: usize,
    /// Physical address of the linear framebuffer.
    pub frame_buffer_base: u64,
    /// Size of the framebuffer in bytes.
    pub frame_buffer_size: usize,
}

/// `EFI_GRAPHICS_OUTPUT_MODE_INFORMATION`.
#[repr(C)]
pub struct GraphicsOutputModeInfo {
    /// Structure version.
    pub version: u32,
    /// Visible width in pixels.
    pub horizontal_resolution: u32,
    /// Visible height in pixels.
    pub vertical_resolution: u32,
    /// `EFI_GRAPHICS_PIXEL_FORMAT`: 0 RGBX, 1 BGRX, 2 bit mask, 3 blt only.
    pub pixel_format: u32,
    /// Masks, used only with the bit-mask format.
    pub pixel_information: [u32; 4],
    /// Pixels from one row to the next.
    pub pixels_per_scan_line: u32,
}

/// `EFI_CONFIGURATION_TABLE` entry.
#[repr(C)]
pub struct ConfigurationTable {
    /// Identifies the table.
    pub vendor_guid: Guid,
    /// The table's address.
    pub vendor_table: *mut c_void,
}

/// Returns the address of the configuration table entry with `guid`, if the
/// firmware has one.
///
/// # Safety
///
/// `system_table` must be the live system table, and its configuration table
/// must hold `number_of_table_entries` entries.
pub unsafe fn find_configuration_table(
    system_table: *const SystemTable,
    guid: &Guid,
) -> Option<*mut c_void> {
    // SAFETY: the caller guarantees `system_table` is live, so reading its
    // fields is valid. `entries` is null-checked before use, and the iterator
    // only indexes `0..count`, the number of entries firmware reports.
    unsafe {
        let count = (*system_table).number_of_table_entries;
        let entries = (*system_table).configuration_table as *const ConfigurationTable;
        if entries.is_null() {
            return None;
        }
        (0..count)
            .map(|index| &*entries.add(index))
            .find(|entry| entry.vendor_guid == *guid)
            .map(|entry| entry.vendor_table)
    }
}

/// `EFI_TABLE_HEADER`.
#[repr(C)]
pub struct TableHeader {
    /// Table signature.
    pub signature: u64,
    /// Specification revision.
    pub revision: u32,
    /// Size of the whole table in bytes.
    pub header_size: u32,
    /// CRC-32 of the table.
    pub crc32: u32,
    /// Reserved, zero.
    pub reserved: u32,
}

/// `EFI_SYSTEM_TABLE`.
#[repr(C)]
pub struct SystemTable {
    /// Table header.
    pub hdr: TableHeader,
    /// Vendor name, UTF-16.
    pub firmware_vendor: *const u16,
    /// Firmware revision.
    pub firmware_revision: u32,
    /// Handle of the console input device.
    pub console_in_handle: Handle,
    /// Console input protocol.
    pub con_in: *mut c_void,
    /// Handle of the console output device.
    pub console_out_handle: Handle,
    /// Console output protocol.
    pub con_out: *mut SimpleTextOutput,
    /// Handle of the standard error device.
    pub standard_error_handle: Handle,
    /// Standard error protocol.
    pub std_err: *mut SimpleTextOutput,
    /// Runtime services table.
    pub runtime_services: *mut c_void,
    /// Boot services table.
    pub boot_services: *mut BootServices,
    /// Number of configuration table entries.
    pub number_of_table_entries: usize,
    /// Configuration table.
    pub configuration_table: *mut c_void,
}

/// `EFI_SIMPLE_TEXT_OUTPUT_PROTOCOL`, up to `OutputString`.
#[repr(C)]
pub struct SimpleTextOutput {
    /// `Reset`. Not used.
    pub reset: *const c_void,
    /// `OutputString`: writes a NUL-terminated UTF-16 string.
    pub output_string: unsafe extern "efiapi" fn(*mut SimpleTextOutput, *const u16) -> Status,
}

/// `EFI_BOOT_SERVICES`, up to `ConnectController`.
#[repr(C)]
pub struct BootServices {
    /// Table header.
    pub hdr: TableHeader,
    /// Slot 2, `RaiseTPL`.
    pub raise_tpl: *const c_void,
    /// Slot 3, `RestoreTPL`.
    pub restore_tpl: *const c_void,
    /// Slot 4, `AllocatePages`.
    pub allocate_pages: unsafe extern "efiapi" fn(u32, u32, usize, *mut u64) -> Status,
    /// Slot 5, `FreePages`.
    pub free_pages: *const c_void,
    /// Slot 6, `GetMemoryMap`.
    pub get_memory_map: unsafe extern "efiapi" fn(
        *mut usize,
        *mut c_void,
        *mut usize,
        *mut usize,
        *mut u32,
    ) -> Status,
    /// Slots 7 to 17: pool, event, timer and protocol-database services.
    pub reserved_pool_and_events: [*const c_void; 11],
    /// Slot 18, `HandleProtocol`.
    pub handle_protocol: unsafe extern "efiapi" fn(Handle, *const Guid, *mut *mut c_void) -> Status,
    /// Slot 19, reserved.
    pub reserved: *const c_void,
    /// Slot 20, `RegisterProtocolNotify`. Not used.
    pub register_protocol_notify: *const c_void,
    /// Slot 21, `LocateHandle`.
    pub locate_handle: unsafe extern "efiapi" fn(
        u32,
        *const Guid,
        *const c_void,
        *mut usize,
        *mut Handle,
    ) -> Status,
    /// Slots 22 to 27: device path, configuration and image services.
    pub reserved_images: [*const c_void; 6],
    /// Slot 28, `ExitBootServices`.
    pub exit_boot_services: unsafe extern "efiapi" fn(Handle, usize) -> Status,
    /// Slots 29 and later are not declared.
    pub reserved_tail: [*const c_void; 1],
    /// Slot 30, `Stall`: busy-waits for microseconds.
    pub stall: unsafe extern "efiapi" fn(usize) -> Status,
    /// Slot 31, `SetWatchdogTimer`. Not used.
    pub set_watchdog_timer: *const c_void,
    /// Slot 32, `ConnectController`: starts drivers on a device.
    pub connect_controller:
        unsafe extern "efiapi" fn(Handle, *mut Handle, *const c_void, u8) -> Status,
}

/// `EFI_LOADED_IMAGE_PROTOCOL`.
#[repr(C)]
pub struct LoadedImage {
    /// Protocol revision.
    pub revision: u32,
    /// Parent image handle.
    pub parent_handle: Handle,
    /// System table pointer.
    pub system_table: *mut SystemTable,
    /// Device the image was loaded from.
    pub device_handle: Handle,
    /// File path of the image.
    pub file_path: *mut c_void,
    /// Reserved.
    pub reserved: *mut c_void,
    /// Size of the load options in bytes.
    pub load_options_size: u32,
    /// Load options.
    pub load_options: *mut c_void,
    /// Image base address.
    pub image_base: *mut c_void,
    /// Image size in bytes.
    pub image_size: u64,
    /// Memory type of the code.
    pub image_code_type: u32,
    /// Memory type of the data.
    pub image_data_type: u32,
    /// Unload function. Not used.
    pub unload: *const c_void,
}

/// `EFI_SIMPLE_FILE_SYSTEM_PROTOCOL`.
#[repr(C)]
pub struct SimpleFileSystem {
    /// Protocol revision.
    pub revision: u64,
    /// `OpenVolume`: returns the root directory.
    pub open_volume: unsafe extern "efiapi" fn(*mut SimpleFileSystem, *mut *mut File) -> Status,
}

/// `EFI_FILE_PROTOCOL`, up to `GetInfo` and `Read`.
#[repr(C)]
pub struct File {
    /// Protocol revision.
    pub revision: u64,
    /// `Open`: opens a file relative to this handle.
    pub open: unsafe extern "efiapi" fn(*mut File, *mut *mut File, *const u16, u64, u64) -> Status,
    /// `Close`.
    pub close: unsafe extern "efiapi" fn(*mut File) -> Status,
    /// `Delete`. Not used.
    pub delete: *const c_void,
    /// `Read`: reads up to `*size` bytes into `buffer`, updating `*size`.
    pub read: unsafe extern "efiapi" fn(*mut File, *mut usize, *mut c_void) -> Status,
    /// `Write`. Not used.
    pub write: *const c_void,
    /// `GetPosition`. Not used.
    pub get_position: *const c_void,
    /// `SetPosition`. Not used.
    pub set_position: *const c_void,
    /// `GetInfo`: fills a buffer with the file information for `info_type`.
    pub get_info:
        unsafe extern "efiapi" fn(*mut File, *const Guid, *mut usize, *mut c_void) -> Status,
}

/// Console output through `SimpleTextOutput`. ASCII only: other characters are
/// printed as `?`.
pub struct Console {
    out: *mut SimpleTextOutput,
}

impl Console {
    /// Wraps the console output protocol.
    ///
    /// # Safety
    ///
    /// `out` must be null or a live `SimpleTextOutput` from the system table,
    /// and it must stay valid while the console is used.
    #[must_use]
    pub unsafe fn new(out: *mut SimpleTextOutput) -> Self {
        Self { out }
    }

    /// Writes `text`, translating `\n` to `\r\n`. Does nothing without an
    /// output protocol.
    pub fn write_str(&mut self, text: &str) {
        if self.out.is_null() {
            return;
        }
        let mut buf = [0u16; 64];
        let mut len = 0;
        for byte in text.bytes() {
            if byte == b'\n' {
                len = self.push(&mut buf, len, u16::from(b'\r'));
            }
            let unit = if byte.is_ascii() {
                u16::from(byte)
            } else {
                u16::from(b'?')
            };
            len = self.push(&mut buf, len, unit);
        }
        self.flush(&mut buf, len);
    }

    fn push(&mut self, buf: &mut [u16; 64], len: usize, unit: u16) -> usize {
        if len + 1 >= buf.len() {
            self.flush(buf, len);
            buf[0] = unit;
            return 1;
        }
        buf[len] = unit;
        len + 1
    }

    fn flush(&mut self, buf: &mut [u16; 64], len: usize) {
        buf[len] = 0;
        // SAFETY: `new` guarantees `out` is a live protocol, and `buf` is
        // NUL-terminated at `len`.
        unsafe {
            ((*self.out).output_string)(self.out, buf.as_ptr());
        }
    }
}

impl core::fmt::Write for Console {
    fn write_str(&mut self, text: &str) -> core::fmt::Result {
        Console::write_str(self, text);
        Ok(())
    }
}

/// Encodes an ASCII path as a NUL-terminated UTF-16 string in `out`.
///
/// Returns `None` if `out` is too small for the path and its terminator.
pub fn encode_path<'a>(path: &str, out: &'a mut [u16]) -> Option<&'a [u16]> {
    if path.len() + 1 > out.len() {
        return None;
    }
    for (slot, byte) in out.iter_mut().zip(path.bytes()) {
        *slot = u16::from(byte);
    }
    let end = path.len();
    out[end] = 0;
    Some(&out[..=end])
}

/// Looks up a protocol on a handle.
///
/// # Safety
///
/// `bs` must be the live boot services table. Boot services must not have
/// been exited.
pub unsafe fn handle_protocol<T>(
    bs: *mut BootServices,
    handle: Handle,
    guid: &Guid,
) -> Result<*mut T, Status> {
    let mut interface: *mut c_void = ptr::null_mut();
    // SAFETY: the caller guarantees `bs` is live. `guid` and `interface` are
    // valid for the duration of the call.
    let status = unsafe { ((*bs).handle_protocol)(handle, guid, &mut interface) };
    if status == EFI_SUCCESS {
        Ok(interface.cast::<T>())
    } else {
        Err(status)
    }
}

/// `LocateHandle` search type for handles that support a protocol.
pub const BY_PROTOCOL: u32 = 2;

/// Lists the handles that support `guid`, writing them into `out`.
///
/// Returns the number of handles found. If `out` is too small, returns
/// [`EFI_BUFFER_TOO_SMALL`] and the caller cannot use the list.
///
/// # Safety
///
/// `bs` must be the live boot services table, and boot services must not have
/// been exited.
pub unsafe fn locate_handles(
    bs: *mut BootServices,
    guid: &Guid,
    out: &mut [Handle],
) -> Result<usize, Status> {
    let mut size = core::mem::size_of_val(out);
    // SAFETY: the caller guarantees `bs` is live. `out` is a live slice of
    // `size_of_val(out)` bytes, which is the `BufferSize` firmware is given, so
    // firmware cannot write past it. `guid` and `&mut size` are locals that
    // outlive the call. The count is only derived on success.
    let status = unsafe {
        ((*bs).locate_handle)(BY_PROTOCOL, guid, ptr::null(), &mut size, out.as_mut_ptr())
    };
    if status == EFI_SUCCESS {
        Ok(size / core::mem::size_of::<Handle>())
    } else {
        Err(status)
    }
}

/// Allocates `pages` 4 KiB pages. With [`ALLOCATE_ADDRESS`], `address` must be
/// the first page wanted. Returns the first page's physical address.
///
/// # Safety
///
/// `bs` must be the live boot services table, and boot services must not have
/// been exited.
pub unsafe fn allocate_pages(
    bs: *mut BootServices,
    kind: u32,
    memory_type: u32,
    pages: usize,
    address: u64,
) -> Result<u64, Status> {
    let mut out = address;
    // SAFETY: the caller guarantees `bs` is live. `out` is a local that lives
    // across the call, so the pointer written by firmware is valid. The value
    // is only read back on success.
    let status = unsafe { ((*bs).allocate_pages)(kind, memory_type, pages, &mut out) };
    if status == EFI_SUCCESS {
        Ok(out)
    } else {
        Err(status)
    }
}

/// Memory map information from `GetMemoryMap`.
#[derive(Clone, Copy, Debug)]
pub struct MapInfo {
    /// Key for `ExitBootServices`.
    pub key: usize,
    /// Size of one descriptor.
    pub descriptor_size: usize,
    /// Bytes the map needs. Equals the bytes written when the call succeeds.
    pub size: usize,
}

/// Reads the memory map into `buf`. Returns the status with the map info. The
/// info is filled even when the status is an error.
///
/// Pass an empty slice to learn the size: the status is then
/// [`EFI_BUFFER_TOO_SMALL`], and `MapInfo::size` gives the bytes needed.
///
/// # Safety
///
/// `bs` must be the live boot services table. `buf` must be valid for writes
/// of its length.
pub unsafe fn get_memory_map(bs: *mut BootServices, buf: &mut [u8]) -> (Status, MapInfo) {
    let mut size = buf.len();
    let mut key = 0usize;
    let mut descriptor_size = 0usize;
    let mut version = 0u32;
    // SAFETY: the caller guarantees `bs` is live. `buf` is a live slice whose
    // length is passed as `size`, so firmware writes at most `buf.len()` bytes.
    // An empty slice gives a dangling but aligned pointer that firmware does not
    // write through, because it reports the required size instead. `key`,
    // `descriptor_size` and `version` are locals that outlive the call.
    let status = unsafe {
        ((*bs).get_memory_map)(
            &mut size,
            buf.as_mut_ptr().cast::<c_void>(),
            &mut key,
            &mut descriptor_size,
            &mut version,
        )
    };
    (
        status,
        MapInfo {
            key,
            descriptor_size,
            size,
        },
    )
}

/// Leaves boot services. After a successful call, no boot service may be used.
///
/// # Safety
///
/// `key` must come from the most recent successful [`get_memory_map`] call, and
/// nothing may have allocated or freed memory since. On failure the caller must
/// read the memory map again and retry.
pub unsafe fn exit_boot_services(bs: *mut BootServices, image: Handle, key: usize) -> Status {
    // SAFETY: the caller guarantees `bs` is live and `key` is current.
    unsafe { ((*bs).exit_boot_services)(image, key) }
}

/// Busy-waits for `micros` microseconds.
///
/// # Safety
///
/// `bs` must be the live boot services table.
pub unsafe fn stall(bs: *mut BootServices, micros: usize) {
    // SAFETY: the caller guarantees `bs` is live. The result is ignored because
    // a failed stall only shortens the wait.
    unsafe {
        let _ = ((*bs).stall)(micros);
    }
}

/// Returns the number of pages that hold `bytes`.
#[must_use]
pub const fn pages_for(bytes: u64) -> usize {
    bytes.div_ceil(4096) as usize
}
