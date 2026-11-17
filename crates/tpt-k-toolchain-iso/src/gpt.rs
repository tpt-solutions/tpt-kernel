// SPDX-License-Identifier: MIT OR Apache-2.0
//! GPT disk wrapper: puts a FAT volume in an EFI System Partition on a raw
//! disk image.
//!
//! The image has a protective MBR at LBA 0, the primary GPT header at LBA 1,
//! the 128-entry partition array at LBA 2 to 33, and one ESP entry that starts
//! at LBA 2048. The volume follows, then the backup partition array and the
//! backup header at the end of the disk. UEFI firmware reads this layout
//! directly, so no ISO 9660 wrapper is used.
//!
//! GUIDs come from the caller, so the output is deterministic.

use std::vec::Vec;

/// Bytes per logical block. GPT and the MBR both use 512-byte sectors here.
pub const SECTOR: usize = 512;

/// First LBA of the partition entry array in the primary header.
const PRIMARY_ENTRIES_LBA: u64 = 2;
/// Number of entries in the partition array.
const ENTRY_COUNT: usize = 128;
/// Bytes per partition entry.
const ENTRY_SIZE: usize = 128;
/// Sectors used by the partition array.
const ENTRIES_SECTORS: u64 = (ENTRY_COUNT * ENTRY_SIZE / SECTOR) as u64;
/// First LBA of the ESP. 2048 keeps it 1 MiB aligned, as tools expect.
const ESP_FIRST_LBA: u64 = 2048;
/// First LBA that may hold a partition: after the primary header and array.
const FIRST_USABLE_LBA: u64 = PRIMARY_ENTRIES_LBA + ENTRIES_SECTORS;
/// Sectors after the ESP that the backup array and backup header take.
const BACKUP_TAIL_SECTORS: u64 = ENTRIES_SECTORS + 1;

/// The EFI System Partition type GUID, in on-disk form.
pub const ESP_TYPE_GUID: [u8; 16] = guid(
    0xC12A_7328,
    0xF81F,
    0x11D2,
    [0xBA, 0x4B, 0x00, 0xA0, 0xC9, 0x3E, 0xC9, 0x3B],
);

/// Why a GPT image could not be built.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GptError {
    /// The volume is empty, so there is nothing to put in the ESP.
    EmptyVolume,
    /// The volume length is not a whole number of 512-byte sectors.
    Misaligned,
}

impl core::fmt::Display for GptError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::EmptyVolume => write!(f, "the FAT volume is empty"),
            Self::Misaligned => write!(
                f,
                "the FAT volume is not a whole number of 512-byte sectors"
            ),
        }
    }
}

impl std::error::Error for GptError {}

/// The CRC-32 table for the IEEE 802.3 polynomial, reflected.
const CRC_TABLE: [u32; 256] = make_crc_table();

const fn make_crc_table() -> [u32; 256] {
    const POLY: u32 = 0xEDB8_8320;
    let mut table = [0u32; 256];
    let mut i = 0;
    while i < 256 {
        let mut crc = i as u32;
        let mut bit = 0;
        while bit < 8 {
            crc = if crc & 1 == 1 {
                (crc >> 1) ^ POLY
            } else {
                crc >> 1
            };
            bit += 1;
        }
        table[i] = crc;
        i += 1;
    }
    table
}

/// CRC-32 (IEEE 802.3, as used by GPT and zip) of `bytes`.
#[must_use]
pub fn crc32(bytes: &[u8]) -> u32 {
    let mut crc = 0xFFFF_FFFFu32;
    for &b in bytes {
        crc = CRC_TABLE[((crc ^ b as u32) & 0xFF) as usize] ^ (crc >> 8);
    }
    crc ^ 0xFFFF_FFFF
}

/// Encodes a GUID from its textual fields in the on-disk mixed-endian form.
///
/// The first three fields are stored little-endian and the last eight bytes
/// are stored as written. `guid(0xC12A7328, 0xF81F, 0x11D2, ...)` therefore
/// starts with the bytes `28 73 2A C1`.
#[must_use]
pub const fn guid(d1: u32, d2: u16, d3: u16, d4: [u8; 8]) -> [u8; 16] {
    let a = d1.to_le_bytes();
    let b = d2.to_le_bytes();
    let c = d3.to_le_bytes();
    [
        a[0], a[1], a[2], a[3], b[0], b[1], c[0], c[1], d4[0], d4[1], d4[2], d4[3], d4[4], d4[5],
        d4[6], d4[7],
    ]
}

/// Builds the 512-byte protective MBR for a disk of `total_sectors` sectors.
fn protective_mbr(total_sectors: u64) -> [u8; SECTOR] {
    let mut mbr = [0u8; SECTOR];
    // One partition of type 0xEE covering the disk after LBA 0. Its CHS
    // range is 0x000200 to 0xFFFFFF, the values the specification gives for
    // a disk too large for CHS.
    let entry = 446;
    mbr[entry + 1..entry + 4].copy_from_slice(&[0x00, 0x02, 0x00]);
    mbr[entry + 4] = 0xEE;
    mbr[entry + 5..entry + 8].copy_from_slice(&[0xFF, 0xFF, 0xFF]);
    mbr[entry + 8..entry + 12].copy_from_slice(&1u32.to_le_bytes());
    let size = (total_sectors - 1).min(u64::from(u32::MAX)) as u32;
    mbr[entry + 12..entry + 16].copy_from_slice(&size.to_le_bytes());
    mbr[510] = 0x55;
    mbr[511] = 0xAA;
    mbr
}

/// Builds the 128-entry partition array with one ESP entry at index 0.
fn partition_array(
    partition_guid: [u8; 16],
    first: u64,
    last: u64,
) -> [u8; ENTRY_COUNT * ENTRY_SIZE] {
    let mut array = [0u8; ENTRY_COUNT * ENTRY_SIZE];
    let e = &mut array[..ENTRY_SIZE];
    e[0..16].copy_from_slice(&ESP_TYPE_GUID);
    e[16..32].copy_from_slice(&partition_guid);
    e[32..40].copy_from_slice(&first.to_le_bytes());
    e[40..48].copy_from_slice(&last.to_le_bytes());
    // Attributes stay zero. The name is "EFI System" in UTF-16LE.
    for (i, unit) in "EFI System".encode_utf16().enumerate() {
        let at = 56 + 2 * i;
        e[at..at + 2].copy_from_slice(&unit.to_le_bytes());
    }
    array
}

/// The fields that differ between the primary and backup headers.
struct Header {
    my_lba: u64,
    alternate_lba: u64,
    first_usable: u64,
    last_usable: u64,
    disk_guid: [u8; 16],
    entries_lba: u64,
    entries_crc: u32,
}

/// Builds one 512-byte GPT header sector with its CRC filled in.
fn gpt_header(h: &Header) -> [u8; SECTOR] {
    let mut b = [0u8; 92];
    b[0..8].copy_from_slice(b"EFI PART");
    b[8..12].copy_from_slice(&0x0001_0000u32.to_le_bytes());
    b[12..16].copy_from_slice(&92u32.to_le_bytes());
    // b[16..20] is the header CRC, filled in below with the field zero.
    b[24..32].copy_from_slice(&h.my_lba.to_le_bytes());
    b[32..40].copy_from_slice(&h.alternate_lba.to_le_bytes());
    b[40..48].copy_from_slice(&h.first_usable.to_le_bytes());
    b[48..56].copy_from_slice(&h.last_usable.to_le_bytes());
    b[56..72].copy_from_slice(&h.disk_guid);
    b[72..80].copy_from_slice(&h.entries_lba.to_le_bytes());
    b[80..84].copy_from_slice(&(ENTRY_COUNT as u32).to_le_bytes());
    b[84..88].copy_from_slice(&(ENTRY_SIZE as u32).to_le_bytes());
    b[88..92].copy_from_slice(&h.entries_crc.to_le_bytes());
    let crc = crc32(&b);
    b[16..20].copy_from_slice(&crc.to_le_bytes());
    let mut sector = [0u8; SECTOR];
    sector[..92].copy_from_slice(&b);
    sector
}

/// Wraps a FAT volume in a GPT disk image with one ESP entry.
///
/// `volume` must be a whole number of 512-byte sectors. `disk_guid` and
/// `partition_guid` are written as given, in on-disk form (see [`guid`]).
/// The image is `2048 + sectors + 33` sectors long: the reserved gap, the
/// volume, then the backup array and backup header.
pub fn wrap(
    volume: &[u8],
    disk_guid: [u8; 16],
    partition_guid: [u8; 16],
) -> Result<Vec<u8>, GptError> {
    if volume.is_empty() {
        return Err(GptError::EmptyVolume);
    }
    if !volume.len().is_multiple_of(SECTOR) {
        return Err(GptError::Misaligned);
    }
    let sectors = (volume.len() / SECTOR) as u64;
    let esp_last = ESP_FIRST_LBA + sectors - 1;
    let total = esp_last + 1 + BACKUP_TAIL_SECTORS;
    let last = total - 1;
    let backup_entries_lba = last - ENTRIES_SECTORS;
    let last_usable = backup_entries_lba - 1;

    let array = partition_array(partition_guid, ESP_FIRST_LBA, esp_last);
    let entries_crc = crc32(&array);

    let primary = gpt_header(&Header {
        my_lba: 1,
        alternate_lba: last,
        first_usable: FIRST_USABLE_LBA,
        last_usable,
        disk_guid,
        entries_lba: PRIMARY_ENTRIES_LBA,
        entries_crc,
    });
    let backup = gpt_header(&Header {
        my_lba: last,
        alternate_lba: 1,
        first_usable: FIRST_USABLE_LBA,
        last_usable,
        disk_guid,
        entries_lba: backup_entries_lba,
        entries_crc,
    });

    let mut disk = vec![0u8; total as usize * SECTOR];
    disk[0..SECTOR].copy_from_slice(&protective_mbr(total));
    disk[SECTOR..2 * SECTOR].copy_from_slice(&primary);
    let primary_at = PRIMARY_ENTRIES_LBA as usize * SECTOR;
    disk[primary_at..primary_at + array.len()].copy_from_slice(&array);
    let esp_at = ESP_FIRST_LBA as usize * SECTOR;
    disk[esp_at..esp_at + volume.len()].copy_from_slice(volume);
    let backup_at = backup_entries_lba as usize * SECTOR;
    disk[backup_at..backup_at + array.len()].copy_from_slice(&array);
    let last_at = last as usize * SECTOR;
    disk[last_at..last_at + SECTOR].copy_from_slice(&backup);
    Ok(disk)
}

#[cfg(test)]
mod tests {
    use super::*;

    const DISK_GUID: [u8; 16] = guid(
        0x1111_2222,
        0x3333,
        0x4444,
        [0x55, 0x66, 0x77, 0x88, 0x99, 0xAA, 0xBB, 0xCC],
    );
    const PART_GUID: [u8; 16] = guid(
        0xDDDD_EEEE,
        0xFFFF,
        0x0102,
        [0x03, 0x04, 0x05, 0x06, 0x07, 0x08, 0x09, 0x0A],
    );

    fn le64(b: &[u8], at: usize) -> u64 {
        u64::from_le_bytes(b[at..at + 8].try_into().unwrap())
    }

    fn le32(b: &[u8], at: usize) -> u32 {
        u32::from_le_bytes(b[at..at + 4].try_into().unwrap())
    }

    /// Checks a GPT header sector: signature, size, and CRC with the field zeroed.
    fn check_header(disk: &[u8], lba: u64) -> [u8; 92] {
        let at = lba as usize * SECTOR;
        let sector = &disk[at..at + SECTOR];
        assert_eq!(&sector[0..8], b"EFI PART");
        assert_eq!(le32(sector, 8), 0x0001_0000);
        assert_eq!(le32(sector, 12), 92);
        let mut h = [0u8; 92];
        h.copy_from_slice(&sector[..92]);
        let stored = le32(&h, 16);
        h[16..20].fill(0);
        assert_eq!(crc32(&h), stored, "header CRC at LBA {lba}");
        h
    }

    /// Builds a small FAT volume with the sibling writer.
    fn sample_volume() -> Vec<u8> {
        crate::build(&[("EFI/BOOT/BOOTX64.EFI", &[0xAB; 1000][..])]).unwrap()
    }

    #[test]
    fn crc32_matches_the_ieee_check_value() {
        assert_eq!(crc32(b"123456789"), 0xCBF4_3926);
        assert_eq!(crc32(&[]), 0);
    }

    #[test]
    fn mixed_endian_guid_stores_first_fields_little_endian() {
        let g = guid(0x0102_0304, 0x0506, 0x0708, [9, 10, 11, 12, 13, 14, 15, 16]);
        assert_eq!(&g[0..4], &[0x04, 0x03, 0x02, 0x01]);
        assert_eq!(&g[4..6], &[0x06, 0x05]);
        assert_eq!(&g[6..8], &[0x08, 0x07]);
        assert_eq!(&g[8..16], &[9, 10, 11, 12, 13, 14, 15, 16]);
        assert_eq!(&ESP_TYPE_GUID[0..4], &[0x28, 0x73, 0x2A, 0xC1]);
    }

    #[test]
    fn protective_mbr_has_one_ee_entry_and_signature() {
        let disk = wrap(&sample_volume(), DISK_GUID, PART_GUID).unwrap();
        let total = (disk.len() / SECTOR) as u64;
        assert_eq!(&disk[510..512], &[0x55, 0xAA]);
        assert_eq!(disk[446 + 4], 0xEE);
        assert_eq!(le32(&disk, 446 + 8), 1, "protective entry starts at LBA 1");
        assert_eq!(u64::from(le32(&disk, 446 + 12)), total - 1);
    }

    #[test]
    fn primary_header_and_array_are_consistent() {
        let volume = sample_volume();
        let disk = wrap(&volume, DISK_GUID, PART_GUID).unwrap();
        let h = check_header(&disk, 1);
        let total = (disk.len() / SECTOR) as u64;
        assert_eq!(le64(&h, 24), 1, "my LBA");
        assert_eq!(le64(&h, 32), total - 1, "alternate points to the backup");
        assert_eq!(le64(&h, 40), FIRST_USABLE_LBA);
        assert_eq!(
            le64(&h, 48),
            ESP_FIRST_LBA + (volume.len() / SECTOR) as u64 - 1
        );
        assert_eq!(&h[56..72], &DISK_GUID);
        assert_eq!(le64(&h, 72), PRIMARY_ENTRIES_LBA);
        assert_eq!(le32(&h, 80), 128);
        assert_eq!(le32(&h, 84), 128);
        let array = &disk[2 * SECTOR..2 * SECTOR + 128 * 128];
        assert_eq!(le32(&h, 88), crc32(array), "entries CRC");
    }

    #[test]
    fn esp_entry_spans_the_volume_and_is_named() {
        let volume = sample_volume();
        let disk = wrap(&volume, DISK_GUID, PART_GUID).unwrap();
        let entry = &disk[2 * SECTOR..2 * SECTOR + 128];
        assert_eq!(&entry[0..16], &ESP_TYPE_GUID);
        assert_eq!(&entry[16..32], &PART_GUID);
        let first = le64(entry, 32);
        let last = le64(entry, 40);
        assert_eq!(first, 2048);
        assert_eq!(last, 2048 + (volume.len() / SECTOR) as u64 - 1);
        let name: Vec<u16> = (0..10)
            .map(|i| u16::from_le_bytes([entry[56 + 2 * i], entry[57 + 2 * i]]))
            .collect();
        assert_eq!(String::from_utf16(&name).unwrap(), "EFI System");
        assert!(
            entry[76..128].iter().all(|&b| b == 0),
            "unused name bytes are zero"
        );
    }

    #[test]
    fn backup_header_points_back_to_the_primary() {
        let disk = wrap(&sample_volume(), DISK_GUID, PART_GUID).unwrap();
        let total = (disk.len() / SECTOR) as u64;
        let last = total - 1;
        let b = check_header(&disk, last);
        assert_eq!(le64(&b, 24), last, "my LBA");
        assert_eq!(le64(&b, 32), 1, "alternate points back to LBA 1");
        assert_eq!(
            le64(&b, 72),
            last - 32,
            "backup array sits before the backup header"
        );
        let primary_array = &disk[2 * SECTOR..2 * SECTOR + 128 * 128];
        let backup_at = (last - 32) as usize * SECTOR;
        assert_eq!(&disk[backup_at..backup_at + 128 * 128], primary_array);
        assert_eq!(le32(&b, 88), crc32(primary_array));
    }

    #[test]
    fn esp_range_holds_the_volume_exactly() {
        let volume = sample_volume();
        let disk = wrap(&volume, DISK_GUID, PART_GUID).unwrap();
        let start = 2048 * SECTOR;
        assert_eq!(&disk[start..start + volume.len()], &volume[..]);
    }

    #[test]
    fn empty_and_misaligned_volumes_are_refused() {
        assert_eq!(wrap(&[], DISK_GUID, PART_GUID), Err(GptError::EmptyVolume));
        assert_eq!(
            wrap(&[0u8; 513], DISK_GUID, PART_GUID),
            Err(GptError::Misaligned)
        );
    }
}
