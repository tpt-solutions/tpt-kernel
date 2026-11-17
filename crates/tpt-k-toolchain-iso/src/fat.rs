// SPDX-License-Identifier: MIT OR Apache-2.0
//! FAT32 image writer for the tpt-k boot partition.
//!
//! [`build`] takes `(path, bytes)` pairs and returns a complete FAT32 volume in
//! memory: boot sector, FSInfo, a backup boot sector, two FAT copies, directory
//! clusters and file data. Directories are created as needed. Names must be
//! 8.3 (the same rule `tpt-k-boot-fs` reads back), since long file names are
//! not written.
//!
//! Sizing: the volume holds exactly the clusters that the content needs, and
//! is padded up to the FAT32 minimum of 65,525 clusters. That is about 32 MiB
//! with 512-byte clusters. Clusters are 512 bytes for content up to 256 MiB and
//! 4 KiB above that.

use std::collections::BTreeMap;
use std::fmt;
use std::string::{String, ToString};
use std::vec;
use std::vec::Vec;

use tpt_k_boot_fs::fat32::name_83;

/// Bytes per sector.
const SECTOR: usize = 512;
/// Sectors before the first FAT. The count is a power-of-two-friendly choice
/// and matches common formatters.
const RESERVED_SECTORS: u32 = 32;
/// Number of FAT copies written.
const NUM_FATS: u32 = 2;
/// Sector holding the FSInfo structure.
const FSINFO_SECTOR: u16 = 1;
/// Sector holding the backup boot sector.
const BACKUP_BOOT_SECTOR: u16 = 6;
/// Smallest cluster count that makes a volume FAT32.
const MIN_FAT32_CLUSTERS: u32 = 65_525;
/// First cluster that holds data.
const FIRST_DATA_CLUSTER: u32 = 2;
/// Bytes per directory entry.
const DIR_ENTRY: usize = 32;
/// Attribute: directory.
const ATTR_DIRECTORY: u8 = 0x10;
/// Attribute: archive (an ordinary file).
const ATTR_ARCHIVE: u8 = 0x20;
/// End-of-chain marker for FAT32 entries.
const END_OF_CHAIN: u32 = 0x0FFF_FFFF;
/// Media descriptor for fixed disks.
const MEDIA: u8 = 0xF8;
/// Largest file the FAT32 size field records.
const MAX_FILE_BYTES: u64 = u32::MAX as u64;
/// Content above this size uses 4 KiB clusters.
const LARGE_CONTENT: u64 = 256 << 20;
/// Sectors per cluster for content above [`LARGE_CONTENT`].
const LARGE_SECTORS_PER_CLUSTER: u8 = 8;

/// Why an image could not be built.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ImageError {
    /// The path is empty.
    EmptyPath,
    /// A component is not a valid 8.3 name, or is `.`, `..`, or ends in a dot.
    BadName {
        /// The path as given.
        path: String,
    },
    /// The path names an existing file or directory, or a file would sit where
    /// a directory is needed.
    Conflict {
        /// The path as given.
        path: String,
    },
    /// A directory has more entries than one cluster holds. Directories are
    /// one cluster each, so at 512-byte clusters a directory holds 16 entries.
    DirectoryFull {
        /// The directory's path.
        path: String,
    },
    /// The file is larger than the 4 GiB - 1 byte that FAT32 records.
    FileTooLarge {
        /// The path as given.
        path: String,
    },
    /// The volume would need more sectors than a 32-bit count can hold.
    ImageTooLarge,
}

impl fmt::Display for ImageError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::EmptyPath => write!(f, "empty path"),
            Self::BadName { path } => write!(f, "{path}: not an 8.3 name"),
            Self::Conflict { path } => write!(f, "{path}: conflicts with an existing entry"),
            Self::DirectoryFull { path } => write!(f, "{path}: directory has too many entries"),
            Self::FileTooLarge { path } => write!(f, "{path}: file is larger than FAT32 allows"),
            Self::ImageTooLarge => write!(f, "image is too large for a 32-bit sector count"),
        }
    }
}

impl std::error::Error for ImageError {}

/// Sectors per cluster for a volume that holds `content` bytes of file data.
#[must_use]
pub fn sectors_per_cluster_for(content: u64) -> u8 {
    if content <= LARGE_CONTENT {
        1
    } else {
        LARGE_SECTORS_PER_CLUSTER
    }
}

/// A child of a directory: another directory, or a file with its data.
#[derive(Debug)]
enum Child<'a> {
    Dir(usize),
    File { data: &'a [u8], first: u32 },
}

/// A directory in the arena. Index 0 is the root.
#[derive(Debug)]
struct Dir<'a> {
    /// Index of the parent directory. Unused for the root.
    parent: usize,
    /// Children keyed by 8.3 name, in name order.
    children: BTreeMap<[u8; 11], Child<'a>>,
    /// Path used in error messages.
    path: String,
    /// First cluster of the directory, assigned during layout.
    cluster: u32,
}

/// Converts one path component to its 8.3 form, refusing `.`, `..` and a
/// trailing dot, which `name_83` would otherwise accept.
fn short_name(component: &str, path: &str) -> Result<[u8; 11], ImageError> {
    let bad = || ImageError::BadName {
        path: path.to_string(),
    };
    if component == "." || component == ".." || component.ends_with('.') {
        return Err(bad());
    }
    name_83::<()>(component).map_err(|_| bad())
}

/// Adds one file to the arena, creating its parent directories.
fn insert<'a>(dirs: &mut Vec<Dir<'a>>, path: &str, data: &'a [u8]) -> Result<(), ImageError> {
    let conflict = || ImageError::Conflict {
        path: path.to_string(),
    };
    let comps: Vec<&str> = path.split('/').filter(|c| !c.is_empty()).collect();
    let Some((last, parents)) = comps.split_last() else {
        return Err(ImageError::EmptyPath);
    };

    let mut cur = 0usize;
    let mut prefix = String::new();
    for comp in parents {
        let name = short_name(comp, path)?;
        if !prefix.is_empty() {
            prefix.push('/');
        }
        prefix.push_str(comp);
        cur = match dirs[cur].children.get(&name) {
            Some(Child::Dir(i)) => *i,
            Some(Child::File { .. }) => return Err(conflict()),
            None => {
                let idx = dirs.len();
                dirs.push(Dir {
                    parent: cur,
                    children: BTreeMap::new(),
                    path: prefix.clone(),
                    cluster: 0,
                });
                dirs[cur].children.insert(name, Child::Dir(idx));
                idx
            }
        };
    }

    let name = short_name(last, path)?;
    if dirs[cur].children.contains_key(&name) {
        return Err(conflict());
    }
    dirs[cur]
        .children
        .insert(name, Child::File { data, first: 0 });
    Ok(())
}

/// One 32-byte short directory entry.
fn dir_entry(name: &[u8; 11], attr: u8, cluster: u32, size: u32) -> [u8; DIR_ENTRY] {
    let mut e = [0u8; DIR_ENTRY];
    e[0..11].copy_from_slice(name);
    e[11] = attr;
    e[20..22].copy_from_slice(&((cluster >> 16) as u16).to_le_bytes());
    e[26..28].copy_from_slice(&(cluster as u16).to_le_bytes());
    e[28..32].copy_from_slice(&size.to_le_bytes());
    e
}

/// The boot sector for a volume of `total` sectors.
fn boot_sector(total: u32, spc: u8, fat_sectors: u32, root_cluster: u32) -> [u8; SECTOR] {
    let mut b = [0u8; SECTOR];
    b[0..3].copy_from_slice(&[0xEB, 0x58, 0x90]);
    b[3..11].copy_from_slice(b"TPTK    ");
    b[11..13].copy_from_slice(&(SECTOR as u16).to_le_bytes());
    b[13] = spc;
    b[14..16].copy_from_slice(&(RESERVED_SECTORS as u16).to_le_bytes());
    b[16] = NUM_FATS as u8;
    // Root entry count and 16-bit FAT and total fields stay zero for FAT32.
    b[21] = MEDIA;
    b[24..26].copy_from_slice(&63u16.to_le_bytes());
    b[26..28].copy_from_slice(&255u16.to_le_bytes());
    b[32..36].copy_from_slice(&total.to_le_bytes());
    b[36..40].copy_from_slice(&fat_sectors.to_le_bytes());
    b[44..48].copy_from_slice(&root_cluster.to_le_bytes());
    b[48..50].copy_from_slice(&FSINFO_SECTOR.to_le_bytes());
    b[50..52].copy_from_slice(&BACKUP_BOOT_SECTOR.to_le_bytes());
    b[64] = 0x80;
    b[66] = 0x29;
    b[67..71].copy_from_slice(&0x5450_544Bu32.to_le_bytes());
    b[71..82].copy_from_slice(b"NO NAME    ");
    b[82..90].copy_from_slice(b"FAT32   ");
    b[510] = 0x55;
    b[511] = 0xAA;
    b
}

/// The FSInfo sector, with the free count and next-free hint filled in.
fn fsinfo(free: u32, next_free: u32) -> [u8; SECTOR] {
    let mut s = [0u8; SECTOR];
    s[0..4].copy_from_slice(&0x4161_5252u32.to_le_bytes());
    s[484..488].copy_from_slice(&0x6141_7272u32.to_le_bytes());
    s[488..492].copy_from_slice(&free.to_le_bytes());
    s[492..496].copy_from_slice(&next_free.to_le_bytes());
    s[508..512].copy_from_slice(&0xAA55_0000u32.to_le_bytes());
    s
}

/// Builds a FAT32 volume holding `files`.
///
/// Each entry is a path (components separated by `/`, a leading slash is
/// optional) and the file's contents. Parent directories are created as needed.
/// The result is a whole volume, starting with the boot sector, with no
/// partition table around it.
pub fn build(files: &[(&str, &[u8])]) -> Result<Vec<u8>, ImageError> {
    let mut dirs = vec![Dir {
        parent: 0,
        children: BTreeMap::new(),
        path: String::from("/"),
        cluster: 0,
    }];
    let mut content = 0u64;
    for (path, data) in files {
        if data.len() as u64 > MAX_FILE_BYTES {
            return Err(ImageError::FileTooLarge {
                path: path.to_string(),
            });
        }
        content += data.len() as u64;
        insert(&mut dirs, path, data)?;
    }

    let spc = sectors_per_cluster_for(content);
    let cluster_bytes = spc as usize * SECTOR;
    let entries_per_cluster = cluster_bytes / DIR_ENTRY;
    for (i, d) in dirs.iter().enumerate() {
        // A subdirectory also holds "." and "..".
        let needed = d.children.len() + if i == 0 { 0 } else { 2 };
        if needed > entries_per_cluster {
            return Err(ImageError::DirectoryFull {
                path: d.path.clone(),
            });
        }
    }

    // Layout: each directory takes one cluster, then each file its chain.
    let mut next = FIRST_DATA_CLUSTER;
    for d in dirs.iter_mut() {
        d.cluster = next;
        next += 1;
    }
    for d in dirs.iter_mut() {
        for child in d.children.values_mut() {
            if let Child::File { data, first } = child {
                let n = data.len().div_ceil(cluster_bytes) as u32;
                if n > 0 {
                    *first = next;
                    next += n;
                }
            }
        }
    }
    let used = next - FIRST_DATA_CLUSTER;

    // Pad to the FAT32 minimum, then size the FAT to cover every cluster.
    let clusters = used.max(MIN_FAT32_CLUSTERS);
    let fat_sectors = (clusters + 2).div_ceil(128);
    let total = u64::from(RESERVED_SECTORS)
        + u64::from(NUM_FATS) * u64::from(fat_sectors)
        + u64::from(clusters) * u64::from(spc);
    let total = u32::try_from(total).map_err(|_| ImageError::ImageTooLarge)?;

    let data_start = RESERVED_SECTORS + NUM_FATS * fat_sectors;
    let cluster_at =
        |c: u32| (data_start as usize) * SECTOR + (c - FIRST_DATA_CLUSTER) as usize * cluster_bytes;
    let mut img = vec![0u8; total as usize * SECTOR];

    let boot = boot_sector(total, spc, fat_sectors, dirs[0].cluster);
    let free = clusters - used;
    let info = fsinfo(free, next);
    img[..SECTOR].copy_from_slice(&boot);
    img[SECTOR..2 * SECTOR].copy_from_slice(&info);
    let backup = BACKUP_BOOT_SECTOR as usize * SECTOR;
    img[backup..backup + SECTOR].copy_from_slice(&boot);
    img[backup + SECTOR..backup + 2 * SECTOR].copy_from_slice(&info);

    // FAT: reserved entries, a chain per directory and file, and free elsewhere.
    let mut fat = vec![0u32; fat_sectors as usize * (SECTOR / 4)];
    fat[0] = 0x0FFF_FF00 | u32::from(MEDIA);
    fat[1] = END_OF_CHAIN;
    for d in &dirs {
        fat[d.cluster as usize] = END_OF_CHAIN;
    }
    for d in &dirs {
        for child in d.children.values() {
            if let Child::File { data, first } = child {
                let n = data.len().div_ceil(cluster_bytes) as u32;
                for k in 0..n {
                    let c = (first + k) as usize;
                    fat[c] = if k + 1 < n {
                        c as u32 + 1
                    } else {
                        END_OF_CHAIN
                    };
                }
            }
        }
    }
    let fat_bytes: Vec<u8> = fat.iter().flat_map(|e| e.to_le_bytes()).collect();
    for copy in 0..NUM_FATS {
        let at = (RESERVED_SECTORS + copy * fat_sectors) as usize * SECTOR;
        img[at..at + fat_bytes.len()].copy_from_slice(&fat_bytes);
    }

    // Directories, then file data.
    for (i, d) in dirs.iter().enumerate() {
        let mut at = cluster_at(d.cluster);
        if i != 0 {
            let parent_cluster = if d.parent == 0 {
                0
            } else {
                dirs[d.parent].cluster
            };
            img[at..at + DIR_ENTRY].copy_from_slice(&dir_entry(
                b".          ",
                ATTR_DIRECTORY,
                d.cluster,
                0,
            ));
            at += DIR_ENTRY;
            img[at..at + DIR_ENTRY].copy_from_slice(&dir_entry(
                b"..         ",
                ATTR_DIRECTORY,
                parent_cluster,
                0,
            ));
            at += DIR_ENTRY;
        }
        for (name, child) in &d.children {
            let entry = match child {
                Child::Dir(j) => dir_entry(name, ATTR_DIRECTORY, dirs[*j].cluster, 0),
                Child::File { data, first } => {
                    dir_entry(name, ATTR_ARCHIVE, *first, data.len() as u32)
                }
            };
            img[at..at + DIR_ENTRY].copy_from_slice(&entry);
            at += DIR_ENTRY;
        }
    }
    for d in &dirs {
        for child in d.children.values() {
            if let Child::File { data, first } = child {
                if !data.is_empty() {
                    let at = cluster_at(*first);
                    img[at..at + data.len()].copy_from_slice(data);
                }
            }
        }
    }

    Ok(img)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tpt_k_boot_fs::fat32::{BlockRead, Fat32, Fat32Error};

    /// An in-memory block device over a finished image.
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

    fn pattern(n: usize) -> Vec<u8> {
        (0..n).map(|i| (i * 7 % 251) as u8).collect()
    }

    fn sample() -> (Vec<u8>, Vec<u8>) {
        let big = pattern(300_000);
        let files: Vec<(&str, &[u8])> = vec![
            ("EFI/BOOT/BOOTX64.EFI", &big),
            ("TPTK/KERNEL.ELF", b"kernel bytes"),
            ("TPTK/ROOT.ELF", b""),
            ("README.TXT", b"hello"),
        ];
        (build(&files).unwrap(), big)
    }

    #[test]
    fn round_trip_through_the_boot_fs_reader() {
        let (img, big) = sample();
        let mut fs = Fat32::mount(MemDisk(img)).unwrap();
        assert_eq!(fs.bpb().root_cluster, 2);
        assert!(fs.bpb().cluster_count >= MIN_FAT32_CLUSTERS);

        let mut out = vec![0u8; big.len()];
        let n = fs.read_file("EFI/BOOT/BOOTX64.EFI", &mut out).unwrap();
        assert_eq!(n, big.len());
        assert_eq!(out, big);

        let mut out = [0u8; 64];
        let n = fs.read_file("tptk/kernel.elf", &mut out).unwrap();
        assert_eq!(&out[..n], b"kernel bytes");

        let n = fs.read_file("TPTK/ROOT.ELF", &mut out).unwrap();
        assert_eq!(n, 0);

        let n = fs.read_file("/README.TXT", &mut out).unwrap();
        assert_eq!(&out[..n], b"hello");
    }

    #[test]
    fn missing_file_is_not_found() {
        let (img, _) = sample();
        let mut fs = Fat32::mount(MemDisk(img)).unwrap();
        assert_eq!(fs.lookup("TPTK/MISSING.ELF"), Err(Fat32Error::NotFound));
        assert!(fs.lookup("EFI/BOOT").unwrap().is_dir);
    }

    #[test]
    fn backup_boot_sector_and_fat_copies_match() {
        let (img, _) = sample();
        let boot = &img[..SECTOR];
        assert_eq!(&img[6 * SECTOR..7 * SECTOR], boot);
        let fat_sectors = u32::from_le_bytes([boot[36], boot[37], boot[38], boot[39]]) as usize;
        let fat1 = 32 * SECTOR;
        let fat2 = (32 + fat_sectors) * SECTOR;
        assert_eq!(
            img[fat1..fat1 + fat_sectors * SECTOR],
            img[fat2..fat2 + fat_sectors * SECTOR]
        );
    }

    #[test]
    fn small_content_is_padded_to_the_fat32_minimum() {
        let img = build(&[("A.TXT", b"x")]).unwrap();
        // 32 reserved + 2 * 512 FAT sectors + 65,525 one-sector clusters.
        assert_eq!(img.len(), (32 + 2 * 512 + 65_525) * SECTOR);
        let mut fs = Fat32::mount(MemDisk(img)).unwrap();
        assert_eq!(fs.bpb().cluster_count, MIN_FAT32_CLUSTERS);
        let mut out = [0u8; 4];
        assert_eq!(fs.read_file("A.TXT", &mut out).unwrap(), 1);
    }

    #[test]
    fn names_are_checked_as_8_3() {
        for bad in [
            "toolongname.txt",
            "a..b",
            "a b.txt",
            "ok.",
            "..",
            "x/./y",
            "a/.hidden",
        ] {
            assert!(
                matches!(build(&[(bad, b"")]), Err(ImageError::BadName { .. })),
                "{bad} was accepted"
            );
        }
        assert_eq!(build(&[("", b"")]).unwrap_err(), ImageError::EmptyPath);
    }

    #[test]
    fn conflicts_are_refused() {
        let twice = build(&[("A.TXT", b"1"), ("A.TXT", b"2")]);
        assert!(matches!(twice, Err(ImageError::Conflict { .. })));
        // A file where a directory is needed, in either order.
        let file_then_dir = build(&[("A", b"1"), ("A/B.TXT", b"2")]);
        assert!(matches!(file_then_dir, Err(ImageError::Conflict { .. })));
        let dir_then_file = build(&[("A/B.TXT", b"2"), ("A", b"1")]);
        assert!(matches!(dir_then_file, Err(ImageError::Conflict { .. })));
    }

    #[test]
    fn a_full_root_directory_is_refused() {
        let names: Vec<String> = (0..17).map(|i| format!("F{i:03}.TXT")).collect();
        let files: Vec<(&str, &[u8])> = names.iter().map(|n| (n.as_str(), &b"x"[..])).collect();
        assert!(matches!(
            build(&files),
            Err(ImageError::DirectoryFull { path }) if path == "/"
        ));
        // Sixteen fit exactly.
        assert!(build(&files[..16]).is_ok());
    }

    #[test]
    fn cluster_size_follows_content_size() {
        assert_eq!(sectors_per_cluster_for(0), 1);
        assert_eq!(sectors_per_cluster_for(LARGE_CONTENT), 1);
        assert_eq!(
            sectors_per_cluster_for(LARGE_CONTENT + 1),
            LARGE_SECTORS_PER_CLUSTER
        );
    }

    #[test]
    fn files_in_one_directory_share_it() {
        let img = build(&[("D/ONE.TXT", b"1"), ("D/TWO.TXT", b"2")]).unwrap();
        let mut fs = Fat32::mount(MemDisk(img)).unwrap();
        let d = fs.lookup("D").unwrap();
        assert!(d.is_dir);
        let mut out = [0u8; 2];
        assert_eq!(fs.read_file("D/TWO.TXT", &mut out).unwrap(), 1);
        assert_eq!(out[0], b'2');
    }
}
