// SPDX-License-Identifier: MIT OR Apache-2.0
//! Read-only FAT32 for the loader: boot sector checks, cluster chains, 8.3
//! directory lookup, and whole-file reads into a caller buffer.
//!
//! The driver needs no heap. It reads through [`BlockRead`], one small
//! field at a time, so it never holds a cluster in memory. A path component
//! is matched by its 8.3 short name and by its long file name (LFN entries,
//! ASCII only). A long name counts only when its LFN chain is complete and
//! its checksum matches the short entry it precedes; otherwise the short
//! name is the only match.

use core::fmt;

/// Bytes per sector. The driver refuses any other sector size.
pub const SECTOR_SIZE: usize = 512;
/// Directory entry size in bytes.
const DIR_ENTRY: usize = 32;
/// Attribute bit: the entry is a directory.
const ATTR_DIRECTORY: u8 = 0x10;
/// Attribute bit: volume label.
const ATTR_VOLUME_ID: u8 = 0x08;
/// Attribute value that marks a long-file-name entry.
const ATTR_LFN: u8 = 0x0F;
/// First cluster number that holds data.
const FIRST_DATA_CLUSTER: u32 = 2;
/// Smallest cluster count that makes a volume FAT32 (Microsoft spec).
const MIN_FAT32_CLUSTERS: u32 = 65_525;
/// Longest long file name accepted, in bytes. All names are ASCII.
pub const MAX_LONG_NAME: usize = 255;
/// Most LFN entries in one chain: 255 characters at 13 per entry.
const MAX_LFN_PARTS: u8 = 20;
/// Byte offsets of the 13 UCS-2 characters inside an LFN entry.
const LFN_OFFSETS: [usize; 13] = [1, 3, 5, 7, 9, 14, 16, 18, 20, 22, 24, 28, 30];
/// Flag in an LFN sequence byte that marks the highest-numbered entry.
const LFN_LAST: u8 = 0x40;

/// Reads bytes at an absolute offset within the volume.
pub trait BlockRead {
    /// The device's error type.
    type Error;

    /// Fills `buf` from `offset`. Fails if the range is outside the device.
    fn read_at(&mut self, offset: u64, buf: &mut [u8]) -> Result<(), Self::Error>;
}

/// Why a volume or a path could not be read.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Fat32Error<E> {
    /// The device failed.
    Io(E),
    /// The boot sector has a field outside what this driver accepts. The
    /// string says which check failed.
    BadBootSector(&'static str),
    /// The volume is valid FAT but has fewer clusters than FAT32 requires.
    NotFat32,
    /// A cluster number is out of range, or a chain ends before the file does.
    Corrupt,
    /// A path component does not exist.
    NotFound,
    /// A path component that is not the last names a file.
    NotADirectory,
    /// The final path component is a directory.
    NotAFile,
    /// The name is empty, not ASCII, or longer than [`MAX_LONG_NAME`].
    BadName,
}

impl<E: fmt::Debug> fmt::Display for Fat32Error<E> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(e) => write!(f, "device error: {e:?}"),
            Self::BadBootSector(why) => write!(f, "bad boot sector: {why}"),
            Self::NotFat32 => write!(f, "volume is not FAT32"),
            Self::Corrupt => write!(f, "corrupt cluster chain"),
            Self::NotFound => write!(f, "not found"),
            Self::NotADirectory => write!(f, "not a directory"),
            Self::NotAFile => write!(f, "is a directory"),
            Self::BadName => write!(f, "name is not ASCII or is too long"),
        }
    }
}

/// The fields of the boot sector this driver uses, after checking them.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Bpb {
    /// Sectors per cluster; a power of two from 1 to 128.
    pub sectors_per_cluster: u8,
    /// Sectors before the first FAT.
    pub reserved_sectors: u16,
    /// Number of FAT copies.
    pub num_fats: u8,
    /// Sectors per FAT.
    pub fat_size: u32,
    /// Total sectors on the volume.
    pub total_sectors: u32,
    /// First cluster of the root directory.
    pub root_cluster: u32,
    /// Number of data clusters.
    pub cluster_count: u32,
}

impl Bpb {
    /// Parses and checks a 512-byte boot sector.
    pub fn parse(sector: &[u8; SECTOR_SIZE]) -> Result<Self, &'static str> {
        if sector[510] != 0x55 || sector[511] != 0xAA {
            return Err("missing 0x55AA signature");
        }
        let u16_at = |o: usize| u16::from_le_bytes([sector[o], sector[o + 1]]);
        let u32_at =
            |o: usize| u32::from_le_bytes([sector[o], sector[o + 1], sector[o + 2], sector[o + 3]]);
        if u16_at(11) as usize != SECTOR_SIZE {
            return Err("bytes per sector is not 512");
        }
        let spc = sector[13];
        if !spc.is_power_of_two() || spc > 128 {
            return Err("sectors per cluster is not a power of two up to 128");
        }
        let reserved = u16_at(14);
        if reserved == 0 {
            return Err("no reserved sectors");
        }
        let num_fats = sector[16];
        if num_fats == 0 {
            return Err("no FAT copies");
        }
        if u16_at(17) != 0 || u16_at(22) != 0 {
            return Err("FAT16 fields are not zero");
        }
        let total16 = u16_at(19) as u32;
        let total32 = u32_at(32);
        let total_sectors = if total32 != 0 { total32 } else { total16 };
        if total_sectors == 0 {
            return Err("no total sector count");
        }
        let fat_size = u32_at(36);
        if fat_size == 0 {
            return Err("FAT size is zero");
        }
        let root_cluster = u32_at(44);
        let data_start = reserved as u64 + num_fats as u64 * fat_size as u64;
        let data_sectors = (total_sectors as u64)
            .checked_sub(data_start)
            .ok_or("FAT does not fit on the volume")?;
        let cluster_count = (data_sectors / spc as u64) as u32;
        if cluster_count < MIN_FAT32_CLUSTERS {
            return Err("too few clusters for FAT32");
        }
        if root_cluster < FIRST_DATA_CLUSTER || root_cluster - FIRST_DATA_CLUSTER >= cluster_count {
            return Err("root cluster is out of range");
        }
        // One 4-byte FAT entry per cluster, plus the two reserved entries.
        if (fat_size as u64) * (SECTOR_SIZE as u64 / 4) < cluster_count as u64 + 2 {
            return Err("FAT is too small for the cluster count");
        }
        Ok(Self {
            sectors_per_cluster: spc,
            reserved_sectors: reserved,
            num_fats,
            fat_size,
            total_sectors,
            root_cluster,
            cluster_count,
        })
    }

    /// Bytes per cluster.
    #[must_use]
    pub fn cluster_bytes(&self) -> u32 {
        self.sectors_per_cluster as u32 * SECTOR_SIZE as u32
    }

    /// Byte offset of the first FAT.
    #[must_use]
    pub fn fat_offset(&self) -> u64 {
        self.reserved_sectors as u64 * SECTOR_SIZE as u64
    }

    /// Byte offset of data cluster 2.
    #[must_use]
    pub fn data_offset(&self) -> u64 {
        (self.reserved_sectors as u64 + self.num_fats as u64 * self.fat_size as u64)
            * SECTOR_SIZE as u64
    }
}

/// What a path names: a file or a directory, and where its data starts.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Entry {
    /// True for a directory.
    pub is_dir: bool,
    /// First cluster of the data, or 0 for an empty file.
    pub first_cluster: u32,
    /// File size in bytes. Zero for a directory.
    pub size: u32,
}

/// A mounted FAT32 volume.
#[derive(Debug)]
pub struct Fat32<D> {
    device: D,
    bpb: Bpb,
}

impl<D: BlockRead> Fat32<D> {
    /// Reads and checks the boot sector, and returns the mounted volume.
    pub fn mount(mut device: D) -> Result<Self, Fat32Error<D::Error>> {
        let mut sector = [0u8; SECTOR_SIZE];
        device.read_at(0, &mut sector).map_err(Fat32Error::Io)?;
        let bpb = match Bpb::parse(&sector) {
            Ok(b) => b,
            Err("too few clusters for FAT32") => return Err(Fat32Error::NotFat32),
            Err(why) => return Err(Fat32Error::BadBootSector(why)),
        };
        Ok(Self { device, bpb })
    }

    /// The checked boot sector fields.
    #[must_use]
    pub fn bpb(&self) -> &Bpb {
        &self.bpb
    }

    /// Returns the device, for example to reuse it after reading.
    pub fn into_device(self) -> D {
        self.device
    }

    /// Looks up `path` (components separated by `/`, leading slash optional)
    /// and describes what it names. A component matches its 8.3 name or its
    /// long name, ignoring ASCII case. The empty path is the root directory.
    pub fn lookup(&mut self, path: &str) -> Result<Entry, Fat32Error<D::Error>> {
        let mut current = Entry {
            is_dir: true,
            first_cluster: self.bpb.root_cluster,
            size: 0,
        };
        for component in path.split('/').filter(|c| !c.is_empty() && *c != ".") {
            if !current.is_dir {
                return Err(Fat32Error::NotADirectory);
            }
            let want = Want::parse::<D::Error>(component)?;
            current = self
                .find_in_dir(current.first_cluster, &want)?
                .ok_or(Fat32Error::NotFound)?;
        }
        Ok(current)
    }

    /// Reads the file at `path` into `out`, up to `out.len()` bytes or the
    /// file size, whichever is smaller. Returns the number of bytes written.
    pub fn read_file(&mut self, path: &str, out: &mut [u8]) -> Result<usize, Fat32Error<D::Error>> {
        let entry = self.lookup(path)?;
        if entry.is_dir {
            return Err(Fat32Error::NotAFile);
        }
        let want = (entry.size as usize).min(out.len());
        let mut done = 0usize;
        let mut cluster = entry.first_cluster;
        let cluster_bytes = self.bpb.cluster_bytes() as usize;
        while done < want {
            if !self.valid_cluster(cluster) {
                return Err(Fat32Error::Corrupt);
            }
            let take = (want - done).min(cluster_bytes);
            let at = self.cluster_offset(cluster);
            self.device
                .read_at(at, &mut out[done..done + take])
                .map_err(Fat32Error::Io)?;
            done += take;
            if done < want {
                cluster = self.next_cluster(cluster)?.ok_or(Fat32Error::Corrupt)?;
            }
        }
        Ok(done)
    }

    fn valid_cluster(&self, cluster: u32) -> bool {
        cluster >= FIRST_DATA_CLUSTER && cluster - FIRST_DATA_CLUSTER < self.bpb.cluster_count
    }

    fn cluster_offset(&self, cluster: u32) -> u64 {
        self.bpb.data_offset()
            + (cluster - FIRST_DATA_CLUSTER) as u64 * self.bpb.cluster_bytes() as u64
    }

    /// The cluster after `cluster` in its chain, or `None` at end of chain.
    fn next_cluster(&mut self, cluster: u32) -> Result<Option<u32>, Fat32Error<D::Error>> {
        let mut raw = [0u8; 4];
        let at = self.bpb.fat_offset() + cluster as u64 * 4;
        self.device.read_at(at, &mut raw).map_err(Fat32Error::Io)?;
        let next = u32::from_le_bytes(raw) & 0x0FFF_FFFF;
        if next >= 0x0FFF_FFF8 {
            return Ok(None);
        }
        if !self.valid_cluster(next) {
            return Err(Fat32Error::Corrupt);
        }
        Ok(Some(next))
    }

    /// Searches a directory chain for `want`, by short name or by long name.
    fn find_in_dir(
        &mut self,
        dir_cluster: u32,
        want: &Want<'_>,
    ) -> Result<Option<Entry>, Fat32Error<D::Error>> {
        let entries_per_cluster = self.bpb.cluster_bytes() as usize / DIR_ENTRY;
        let mut cluster = dir_cluster;
        // A chain cannot be longer than the cluster count; more hops mean a loop.
        let mut hops = 0u32;
        // LFN entries may cross clusters, so the chain lives across the loop.
        let mut chain = LfnChain::new();
        loop {
            if !self.valid_cluster(cluster) {
                return Err(Fat32Error::Corrupt);
            }
            let base = self.cluster_offset(cluster);
            for i in 0..entries_per_cluster {
                let mut e = [0u8; DIR_ENTRY];
                self.device
                    .read_at(base + (i * DIR_ENTRY) as u64, &mut e)
                    .map_err(Fat32Error::Io)?;
                match e[0] {
                    0x00 => return Ok(None),
                    0xE5 => {
                        chain.reset();
                        continue;
                    }
                    _ => {}
                }
                if e[11] == ATTR_LFN {
                    chain.push(&e);
                    continue;
                }
                let mut short = [0u8; 11];
                short.copy_from_slice(&e[0..11]);
                let long_hit = chain.names(&short, want.long);
                chain.reset();
                if e[11] & ATTR_VOLUME_ID != 0 {
                    continue;
                }
                let short_hit = want
                    .short
                    .is_some_and(|n| short[..].eq_ignore_ascii_case(&n[..]));
                if long_hit || short_hit {
                    let hi = u16::from_le_bytes([e[20], e[21]]) as u32;
                    let lo = u16::from_le_bytes([e[26], e[27]]) as u32;
                    return Ok(Some(Entry {
                        is_dir: e[11] & ATTR_DIRECTORY != 0,
                        first_cluster: (hi << 16) | lo,
                        size: u32::from_le_bytes([e[28], e[29], e[30], e[31]]),
                    }));
                }
            }
            hops += 1;
            if hops > self.bpb.cluster_count {
                return Err(Fat32Error::Corrupt);
            }
            match self.next_cluster(cluster)? {
                Some(next) => cluster = next,
                None => return Ok(None),
            }
        }
    }
}

/// Converts one path component to the 11-byte space-padded 8.3 form.
///
/// Names are upper-cased. `..` is accepted as the parent entry name. Anything
/// that does not fit 8.3 is refused here; [`Fat32::lookup`] still tries such
/// a name against long names.
pub fn name_83<E>(component: &str) -> Result<[u8; 11], Fat32Error<E>> {
    let mut out = [b' '; 11];
    if component == ".." {
        out[0] = b'.';
        out[1] = b'.';
        return Ok(out);
    }
    let (base, ext) = match component.rfind('.') {
        Some(dot) if dot > 0 => (&component[..dot], &component[dot + 1..]),
        _ => (component, ""),
    };
    if base.is_empty() || base.len() > 8 || ext.len() > 3 {
        return Err(Fat32Error::BadName);
    }
    for (dst, src) in out[..8].iter_mut().zip(base.bytes()) {
        *dst = valid_83_char(src)?;
    }
    for (dst, src) in out[8..].iter_mut().zip(ext.bytes()) {
        *dst = valid_83_char(src)?;
    }
    Ok(out)
}

fn valid_83_char<E>(b: u8) -> Result<u8, Fat32Error<E>> {
    const EXTRA: &[u8] = b"!#$%&'()-@^_`{}~";
    if b.is_ascii_alphanumeric() || EXTRA.contains(&b) {
        Ok(b.to_ascii_uppercase())
    } else {
        Err(Fat32Error::BadName)
    }
}

/// A path component as the directory search sees it: its 8.3 form, when it
/// has one, and the text to match against long names.
#[derive(Debug, Clone, Copy)]
struct Want<'a> {
    short: Option<[u8; 11]>,
    long: &'a str,
}

impl<'a> Want<'a> {
    fn parse<E>(component: &'a str) -> Result<Self, Fat32Error<E>> {
        if component.is_empty() || component.len() > MAX_LONG_NAME || !component.is_ascii() {
            return Err(Fat32Error::BadName);
        }
        Ok(Self {
            short: name_83::<E>(component).ok(),
            long: component,
        })
    }
}

/// The checksum of a short name, as LFN entries record it (the rotate-and-add
/// sum from the FAT specification).
fn short_checksum(name: &[u8; 11]) -> u8 {
    name.iter().fold(0u8, |sum, &c| {
        let rotated: u8 = if sum & 1 != 0 { 0x80 } else { 0 };
        rotated.wrapping_add(sum >> 1).wrapping_add(c)
    })
}

/// The LFN entries seen since the last short entry, held in a fixed buffer.
///
/// Entries arrive highest sequence number first, with the last-entry flag on
/// the first. The chain is complete when sequence 1 has been read.
struct LfnChain {
    /// True while the entries so far form a chain that began with the flag.
    active: bool,
    /// Number of entries the chain has.
    count: u8,
    /// Sequence number the next entry must carry. Zero once complete.
    next: u8,
    /// Short-name checksum recorded on every entry of the chain.
    checksum: u8,
    /// The 13 UCS-2 characters of each entry, in name order.
    units: [u16; MAX_LFN_PARTS as usize * 13],
}

impl LfnChain {
    fn new() -> Self {
        Self {
            active: false,
            count: 0,
            next: 0,
            checksum: 0,
            units: [0; MAX_LFN_PARTS as usize * 13],
        }
    }

    fn reset(&mut self) {
        self.active = false;
        self.count = 0;
        self.next = 0;
    }

    /// Adds one LFN entry. Anything out of order ends the chain.
    fn push(&mut self, e: &[u8; DIR_ENTRY]) {
        let seq = e[0] & 0x1F;
        if e[0] & LFN_LAST != 0 {
            self.reset();
            if seq == 0 || seq > MAX_LFN_PARTS {
                return;
            }
            self.active = true;
            self.count = seq;
            self.next = seq;
            self.checksum = e[13];
        }
        if !self.active || seq == 0 || seq != self.next || e[13] != self.checksum {
            self.reset();
            return;
        }
        let base = (seq as usize - 1) * 13;
        for (i, off) in LFN_OFFSETS.iter().enumerate() {
            self.units[base + i] = u16::from_le_bytes([e[*off], e[*off + 1]]);
        }
        self.next -= 1;
    }

    /// True if the chain is complete and belongs to the short entry `short`,
    /// and it spells `long` ignoring ASCII case. A chain with a non-ASCII
    /// character, a misplaced terminator, or a wrong checksum never matches.
    fn names(&self, short: &[u8; 11], long: &str) -> bool {
        if !self.active || self.next != 0 || self.checksum != short_checksum(short) {
            return false;
        }
        let total = self.count as usize * 13;
        let raw = &self.units[..total];
        let len = raw.iter().position(|&u| u == 0).unwrap_or(total);
        if len == 0 || len != long.len() {
            return false;
        }
        // After the terminator, the rest of the chain is 0xFFFF padding only.
        if len < total && !raw[len + 1..].iter().all(|&u| u == 0xFFFF) {
            return false;
        }
        raw[..len]
            .iter()
            .zip(long.bytes())
            .all(|(&u, b)| u != 0 && u <= 0x7F && (u as u8).eq_ignore_ascii_case(&b))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::vec;
    use std::vec::Vec;

    const CLUSTERS: u32 = 65_540;
    const RSVD: u32 = 32;
    const FATSZ: u32 = 513;
    const DATA_SECTOR: u32 = RSVD + 2 * FATSZ;
    const TOTAL: u32 = DATA_SECTOR + CLUSTERS;
    const EOC: u32 = 0x0FFF_FFFF;

    struct MemDisk(Vec<u8>);

    impl BlockRead for MemDisk {
        type Error = ();

        fn read_at(&mut self, offset: u64, buf: &mut [u8]) -> Result<(), ()> {
            let start = usize::try_from(offset).map_err(|_| ())?;
            let end = start.checked_add(buf.len()).ok_or(())?;
            buf.copy_from_slice(self.0.get(start..end).ok_or(())?);
            Ok(())
        }
    }

    fn cluster_at(c: u32) -> usize {
        (DATA_SECTOR as usize + (c as usize - 2)) * SECTOR_SIZE
    }

    fn set_fat(d: &mut [u8], c: u32, v: u32) {
        let at = RSVD as usize * SECTOR_SIZE + c as usize * 4;
        d[at..at + 4].copy_from_slice(&v.to_le_bytes());
    }

    fn put(d: &mut [u8], c: u32, offset: usize, bytes: &[u8]) {
        let at = cluster_at(c) + offset;
        d[at..at + bytes.len()].copy_from_slice(bytes);
    }

    /// A 32-byte short directory entry.
    fn dirent(name: &[u8; 11], attr: u8, cluster: u32, size: u32) -> [u8; 32] {
        let mut e = [0u8; 32];
        e[0..11].copy_from_slice(name);
        e[11] = attr;
        e[20..22].copy_from_slice(&((cluster >> 16) as u16).to_le_bytes());
        e[26..28].copy_from_slice(&(cluster as u16).to_le_bytes());
        e[28..32].copy_from_slice(&size.to_le_bytes());
        e
    }

    /// A long-file-name entry. Its contents do not matter to the driver.
    fn lfn_entry() -> [u8; 32] {
        let mut e = [0x41u8; 32];
        e[11] = ATTR_LFN;
        e
    }

    fn pattern(n: usize) -> Vec<u8> {
        (0..n).map(|i| (i * 7 % 251) as u8).collect()
    }

    fn write_dir(d: &mut [u8], c: u32, entries: &[[u8; 32]]) {
        for (i, e) in entries.iter().enumerate() {
            put(d, c, i * 32, e);
        }
    }

    fn image() -> Vec<u8> {
        let mut d = vec![0u8; TOTAL as usize * SECTOR_SIZE];
        // Boot sector.
        let b = &mut d[..SECTOR_SIZE];
        b[0..3].copy_from_slice(&[0xEB, 0x58, 0x90]);
        b[3..11].copy_from_slice(b"MSWIN4.1");
        b[11..13].copy_from_slice(&512u16.to_le_bytes());
        b[13] = 1;
        b[14..16].copy_from_slice(&(RSVD as u16).to_le_bytes());
        b[16] = 2;
        b[21] = 0xF8;
        b[32..36].copy_from_slice(&TOTAL.to_le_bytes());
        b[36..40].copy_from_slice(&FATSZ.to_le_bytes());
        b[44..48].copy_from_slice(&2u32.to_le_bytes());
        b[48..50].copy_from_slice(&1u16.to_le_bytes());
        b[510] = 0x55;
        b[511] = 0xAA;

        set_fat(&mut d, 0, 0x0FFF_FFF8);
        set_fat(&mut d, 1, EOC);
        for c in [2, 5, 6, 7, 12] {
            set_fat(&mut d, c, EOC);
        }
        set_fat(&mut d, 3, 4); // HELLO.TXT: 3 -> 4 -> 5
        set_fat(&mut d, 4, 5);
        set_fat(&mut d, 13, 13); // LOOPDIR: a chain that points at itself

        // Root directory: an LFN entry before a short name, a deleted entry,
        // an empty file, a short file, and a subdirectory.
        let pat = pattern(1300);
        let mut gone = dirent(b"GONE    TXT", 0x20, 9, 100);
        gone[0] = 0xE5;
        write_dir(
            &mut d,
            2,
            &[
                lfn_entry(),
                dirent(b"HELLO   TXT", 0x20, 3, 1300),
                dirent(b"SUB        ", ATTR_DIRECTORY, 6, 0),
                gone,
                dirent(b"EMPTY   TXT", 0x20, 0, 0),
                dirent(b"SHORT   BIN", 0x20, 12, 1000),
                dirent(b"LOOPDIR    ", ATTR_DIRECTORY, 13, 0),
            ],
        );
        put(&mut d, 3, 0, &pat[0..512]);
        put(&mut d, 4, 0, &pat[512..1024]);
        put(&mut d, 5, 0, &pat[1024..1300]);

        // SUB: ".", "..", and NOTE.TXT.
        write_dir(
            &mut d,
            6,
            &[
                dirent(b".          ", ATTR_DIRECTORY, 6, 0),
                dirent(b"..         ", ATTR_DIRECTORY, 2, 0),
                dirent(b"NOTE    TXT", 0x20, 7, 11),
            ],
        );
        put(&mut d, 7, 0, b"sub file ok");

        // LOOPDIR: sixteen non-matching entries, so the scan continues to the chain.
        let filler: Vec<[u8; 32]> = (0..16)
            .map(|_| dirent(b"ZZZ        ", 0x20, 0, 0))
            .collect();
        write_dir(&mut d, 13, &filler);
        d
    }

    fn mount() -> Fat32<MemDisk> {
        Fat32::mount(MemDisk(image())).unwrap()
    }

    #[test]
    fn mount_reads_the_geometry() {
        let fs = mount();
        let bpb = fs.bpb();
        assert_eq!(bpb.root_cluster, 2);
        assert_eq!(bpb.cluster_count, CLUSTERS);
        assert_eq!(bpb.cluster_bytes(), 512);
        assert_eq!(bpb.data_offset(), DATA_SECTOR as u64 * 512);
    }

    #[test]
    fn reads_a_file_spanning_three_clusters() {
        let mut fs = mount();
        let mut out = vec![0u8; 2000];
        let n = fs.read_file("HELLO.TXT", &mut out).unwrap();
        assert_eq!(n, 1300);
        assert_eq!(&out[..n], &pattern(1300)[..]);
    }

    #[test]
    fn short_buffer_gets_a_prefix() {
        let mut fs = mount();
        let mut out = vec![0u8; 700];
        let n = fs.read_file("/hello.txt", &mut out).unwrap();
        assert_eq!(n, 700);
        assert_eq!(&out[..], &pattern(1300)[..700]);
    }

    #[test]
    fn lookup_ignores_case_and_leading_slash() {
        let mut fs = mount();
        let entry = fs.lookup("/Hello.Txt").unwrap();
        assert!(!entry.is_dir);
        assert_eq!(entry.size, 1300);
        assert_eq!(entry.first_cluster, 3);
    }

    #[test]
    fn walks_into_a_subdirectory_and_back_out() {
        let mut fs = mount();
        let mut out = [0u8; 32];
        let n = fs.read_file("sub/note.txt", &mut out).unwrap();
        assert_eq!(&out[..n], b"sub file ok");
        let n = fs.read_file("SUB/../HELLO.TXT", &mut [0u8; 8]).unwrap();
        assert_eq!(n, 8);
        assert!(fs.lookup("SUB").unwrap().is_dir);
    }

    #[test]
    fn empty_file_reads_zero_bytes() {
        let mut fs = mount();
        assert_eq!(fs.read_file("EMPTY.TXT", &mut [0u8; 4]).unwrap(), 0);
    }

    #[test]
    fn missing_and_deleted_names_are_not_found() {
        let mut fs = mount();
        assert_eq!(fs.lookup("NOPE.TXT"), Err(Fat32Error::NotFound));
        // GONE.TXT has a 0xE5 first byte, so it is not visible.
        assert_eq!(fs.lookup("GONE.TXT"), Err(Fat32Error::NotFound));
    }

    #[test]
    fn long_name_without_a_matching_chain_is_not_found() {
        let mut fs = mount();
        // The LFN entry in this image is not a valid chain for HELLO.TXT,
        // so the long name matches nothing.
        assert_eq!(fs.lookup("hello text file.txt"), Err(Fat32Error::NotFound));
    }

    #[test]
    fn file_in_the_middle_of_a_path_is_not_a_directory() {
        let mut fs = mount();
        assert_eq!(fs.lookup("HELLO.TXT/X"), Err(Fat32Error::NotADirectory));
    }

    #[test]
    fn directory_is_not_a_file() {
        let mut fs = mount();
        assert_eq!(
            fs.read_file("SUB", &mut [0u8; 4]),
            Err(Fat32Error::NotAFile)
        );
    }

    #[test]
    fn chain_that_ends_before_the_size_is_corrupt() {
        let mut fs = mount();
        // SHORT.BIN declares 1000 bytes but its chain is one cluster long.
        let mut out = vec![0u8; 1000];
        assert_eq!(
            fs.read_file("SHORT.BIN", &mut out),
            Err(Fat32Error::Corrupt)
        );
    }

    #[test]
    fn directory_chain_loop_is_detected() {
        let mut fs = mount();
        assert_eq!(fs.lookup("LOOPDIR/X.TXT"), Err(Fat32Error::Corrupt));
    }

    #[test]
    fn short_names_are_converted_to_83_form() {
        assert_eq!(name_83::<()>("readme").unwrap(), *b"README     ");
        assert_eq!(name_83::<()>("a.bin").unwrap(), *b"A       BIN");
        assert_eq!(name_83::<()>("..").unwrap(), *b"..         ");
        assert_eq!(name_83::<()>("toolongname.txt"), Err(Fat32Error::BadName));
        assert_eq!(name_83::<()>("a.tar.gz"), Err(Fat32Error::BadName));
        assert_eq!(name_83::<()>("a b"), Err(Fat32Error::BadName));
        assert_eq!(name_83::<()>(".hidden"), Err(Fat32Error::BadName));
    }

    fn refuse(edit: &dyn Fn(&mut [u8])) -> Fat32Error<()> {
        let mut d = image();
        edit(&mut d);
        match Fat32::mount(MemDisk(d)) {
            Err(e) => e,
            Ok(_) => panic!("corrupt boot sector was accepted"),
        }
    }

    #[test]
    fn corrupted_boot_sectors_are_refused() {
        assert!(matches!(
            refuse(&|d| d[510] = 0),
            Fat32Error::BadBootSector(_)
        ));
        assert!(matches!(
            refuse(&|d| d[11..13].copy_from_slice(&4096u16.to_le_bytes())),
            Fat32Error::BadBootSector(_)
        ));
        assert!(matches!(
            refuse(&|d| d[13] = 3),
            Fat32Error::BadBootSector(_)
        ));
        assert!(matches!(
            refuse(&|d| d[44..48].copy_from_slice(&0u32.to_le_bytes())),
            Fat32Error::BadBootSector(_)
        ));
        assert!(matches!(
            refuse(&|d| d[36..40].copy_from_slice(&0u32.to_le_bytes())),
            Fat32Error::BadBootSector(_)
        ));
        // A volume too small to be FAT32 is reported as such.
        assert_eq!(
            refuse(&|d| d[32..36].copy_from_slice(&20_000u32.to_le_bytes())),
            Fat32Error::NotFat32
        );
    }

    /// Builds the LFN entries for `name`, for the short entry `short`, in
    /// the order a directory stores them: highest sequence number first.
    fn lfn_entries(name: &str, short: &[u8; 11]) -> Vec<[u8; 32]> {
        let mut units: Vec<u16> = name.bytes().map(u16::from).collect();
        if !units.len().is_multiple_of(13) {
            units.push(0x0000);
        }
        while !units.len().is_multiple_of(13) {
            units.push(0xFFFF);
        }
        let parts = units.len() / 13;
        let sum = short_checksum(short);
        let mut out = Vec::new();
        for k in (1..=parts).rev() {
            let mut e = [0u8; 32];
            e[0] = k as u8 | if k == parts { LFN_LAST } else { 0 };
            for (i, off) in LFN_OFFSETS.iter().enumerate() {
                let u = units[(k - 1) * 13 + i].to_le_bytes();
                e[*off] = u[0];
                e[*off + 1] = u[1];
            }
            e[11] = ATTR_LFN;
            e[13] = sum;
            out.push(e);
        }
        out
    }

    /// A volume whose root directory spills into a second cluster, holding
    /// several long names. One chain crosses the cluster boundary.
    fn lfn_image() -> Vec<u8> {
        let mut d = image();
        // Root: cluster 2 continues in cluster 19.
        set_fat(&mut d, 2, 19);
        set_fat(&mut d, 19, EOC);

        let mut root: Vec<[u8; 32]> = Vec::new();

        // A two-part name's file: kernel-image.elf, cluster 14.
        root.extend(lfn_entries("kernel-image.elf", b"KERNEL~1ELF"));
        root.push(dirent(b"KERNEL~1ELF", 0x20, 14, 6));
        set_fat(&mut d, 14, EOC);
        put(&mut d, 14, 0, b"kernel");

        // Checksum on every entry is wrong, so only the short name matches.
        let mut bad = lfn_entries("bad-name-file.bin", b"BADNAM~1BIN");
        for e in &mut bad {
            e[13] ^= 0xFF;
        }
        root.extend(bad);
        root.push(dirent(b"BADNAM~1BIN", 0x20, 15, 3));
        set_fat(&mut d, 15, EOC);
        put(&mut d, 15, 0, b"bad");

        // A three-part name, cluster 17. Its short entry is the sixteenth root
        // slot overall, so the chain's last entry is in the first root cluster
        // and its short entry in the second.
        root.extend(lfn_entries("three-part-long-name-file.dat", b"THREEP~1DAT"));
        root.push(dirent(b"THREEP~1DAT", 0x20, 17, 5));
        set_fat(&mut d, 17, EOC);
        put(&mut d, 17, 0, b"three");

        // Exactly 13 characters: no terminator and no padding.
        root.extend(lfn_entries("exactly13char", b"EXACTL~1   "));
        root.push(dirent(b"EXACTL~1   ", 0x20, 18, 4));
        set_fat(&mut d, 18, EOC);
        put(&mut d, 18, 0, b"exac");

        // An orphan chain with no short entry after it; the directory ends.
        root.extend(lfn_entries("orphan-entry.txt", b"ORPHAN~1TXT"));

        // The image's own seven entries occupy slots 0 to 6 of cluster 2.
        for (i, e) in root.iter().enumerate() {
            let slot = i + 7;
            let (cluster, index) = if slot < 16 {
                (2, slot)
            } else {
                (19, slot - 16)
            };
            put(&mut d, cluster, index * 32, e);
        }
        d
    }

    fn mount_lfn() -> Fat32<MemDisk> {
        Fat32::mount(MemDisk(lfn_image())).unwrap()
    }

    #[test]
    fn finds_a_file_by_its_long_name_and_reads_it() {
        let mut fs = mount_lfn();
        let mut out = [0u8; 16];
        let n = fs.read_file("kernel-image.elf", &mut out).unwrap();
        assert_eq!(&out[..n], b"kernel");
    }

    #[test]
    fn long_name_lookup_ignores_case() {
        let mut fs = mount_lfn();
        let entry = fs.lookup("KERNEL-Image.ELF").unwrap();
        assert_eq!(entry.first_cluster, 14);
        assert_eq!(entry.size, 6);
    }

    #[test]
    fn short_alias_still_finds_the_same_file() {
        let mut fs = mount_lfn();
        assert_eq!(fs.lookup("KERNEL~1.ELF").unwrap().first_cluster, 14);
    }

    #[test]
    fn three_part_long_name_is_found() {
        let mut fs = mount_lfn();
        let entry = fs.lookup("three-part-long-name-file.dat").unwrap();
        assert_eq!(entry.first_cluster, 17);
        assert_eq!(entry.size, 5);
    }

    #[test]
    fn long_name_of_exactly_thirteen_characters_is_found() {
        let mut fs = mount_lfn();
        assert_eq!(fs.lookup("exactly13char").unwrap().first_cluster, 18);
    }

    #[test]
    fn a_bad_checksum_on_the_lfn_chain_falls_back_to_the_short_name() {
        let mut fs = mount_lfn();
        assert_eq!(fs.lookup("bad-name-file.bin"), Err(Fat32Error::NotFound));
        assert_eq!(fs.lookup("BADNAM~1.BIN").unwrap().first_cluster, 15);
    }

    #[test]
    fn an_orphan_lfn_chain_with_no_short_entry_is_ignored() {
        let mut fs = mount_lfn();
        assert_eq!(fs.lookup("orphan-entry.txt"), Err(Fat32Error::NotFound));
    }

    #[test]
    fn a_prefix_of_a_long_name_does_not_match() {
        let mut fs = mount_lfn();
        assert_eq!(fs.lookup("kernel-image.el"), Err(Fat32Error::NotFound));
        assert_eq!(fs.lookup("kernel-image.elff"), Err(Fat32Error::NotFound));
    }

    #[test]
    fn non_ascii_and_overlong_names_are_refused() {
        let mut fs = mount_lfn();
        assert_eq!(fs.lookup("caf\u{e9}.txt"), Err(Fat32Error::BadName));
        let long = [b'a'; 256];
        let long = core::str::from_utf8(&long).unwrap();
        assert_eq!(fs.lookup(long), Err(Fat32Error::BadName));
    }

    #[test]
    fn a_long_name_chain_with_a_non_ascii_character_does_not_match() {
        let mut d = lfn_image();
        // Root slot 7 is the first entry of the kernel-image.elf chain. Change
        // its first character to U+00E9.
        let at = cluster_at(2) + 7 * 32;
        d[at + 1] = 0xE9;
        d[at + 2] = 0x00;
        let mut fs = Fat32::mount(MemDisk(d)).unwrap();
        assert_eq!(fs.lookup("kernel-image.elf"), Err(Fat32Error::NotFound));
        assert_eq!(fs.lookup("KERNEL~1.ELF").unwrap().first_cluster, 14);
    }

    #[test]
    fn device_failure_is_reported() {
        let r = Fat32::mount(MemDisk(vec![0u8; 100]));
        assert!(matches!(r, Err(Fat32Error::Io(()))));
    }
}
