// SPDX-License-Identifier: MIT OR Apache-2.0
//! ELF64 parsing for the kernel image: little-endian x86_64 executables with
//! `PT_LOAD` segments. Every offset is checked against the input length before
//! it is read.

const ELF_MAGIC: [u8; 4] = [0x7f, b'E', b'L', b'F'];
const CLASS_64: u8 = 2;
const DATA_LSB: u8 = 1;
const ET_EXEC: u16 = 2;
const EM_X86_64: u16 = 62;
const PT_LOAD: u32 = 1;
const EHDR_SIZE: usize = 64;
const PHDR_SIZE: usize = 56;

/// Why an image was refused.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ElfError {
    /// The input is shorter than the fields being read.
    TooShort,
    /// The first four bytes are not `\x7fELF`.
    BadMagic,
    /// Not a 64-bit image.
    NotElf64,
    /// Not little-endian.
    NotLittleEndian,
    /// Not an `ET_EXEC` file. Relocatable, shared and core files are refused.
    NotExecutable,
    /// Not built for x86_64.
    WrongMachine,
    /// A program header or segment lies outside the input, or is malformed.
    BadSegment,
    /// The entry point is not inside a loadable segment.
    EntryOutsideLoad,
}

/// One loadable segment: `file_size` bytes at `file_offset` are copied to
/// physical address `paddr`, and the rest of `mem_size` is zeroed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Segment {
    /// Offset of the segment's data in the file.
    pub file_offset: u64,
    /// Number of bytes to copy from the file.
    pub file_size: u64,
    /// Physical load address.
    pub paddr: u64,
    /// Bytes occupied in memory, at least `file_size`.
    pub mem_size: u64,
}

/// A validated ELF64 image borrowed from the caller's buffer.
#[derive(Debug, Clone, Copy)]
pub struct Elf<'a> {
    data: &'a [u8],
    entry: u64,
    phoff: usize,
    phnum: usize,
}

impl<'a> Elf<'a> {
    /// Checks the header and every `PT_LOAD` segment, then returns the image.
    ///
    /// # Errors
    ///
    /// Returns an [`ElfError`] if the image is not a well-formed x86_64 ELF64
    /// executable whose entry point lies in a loadable segment.
    pub fn parse(data: &'a [u8]) -> Result<Self, ElfError> {
        if data.len() < EHDR_SIZE {
            return Err(ElfError::TooShort);
        }
        if data[..4] != ELF_MAGIC {
            return Err(ElfError::BadMagic);
        }
        if data[4] != CLASS_64 {
            return Err(ElfError::NotElf64);
        }
        if data[5] != DATA_LSB {
            return Err(ElfError::NotLittleEndian);
        }
        if u16_at(data, 16)? != ET_EXEC {
            return Err(ElfError::NotExecutable);
        }
        if u16_at(data, 18)? != EM_X86_64 {
            return Err(ElfError::WrongMachine);
        }
        let entry = u64_at(data, 24)?;
        let phoff = usize::try_from(u64_at(data, 32)?).map_err(|_| ElfError::BadSegment)?;
        let phentsize = usize::from(u16_at(data, 54)?);
        let phnum = usize::from(u16_at(data, 56)?);
        if phentsize != PHDR_SIZE {
            return Err(ElfError::BadSegment);
        }
        let table_end = phnum
            .checked_mul(PHDR_SIZE)
            .and_then(|len| len.checked_add(phoff))
            .ok_or(ElfError::BadSegment)?;
        if table_end > data.len() {
            return Err(ElfError::BadSegment);
        }

        let image = Self {
            data,
            entry,
            phoff,
            phnum,
        };
        let mut entry_covered = false;
        for segment in image.load_segments() {
            let segment = segment?;
            if entry >= segment.paddr && entry - segment.paddr < segment.mem_size {
                entry_covered = true;
            }
        }
        if !entry_covered {
            return Err(ElfError::EntryOutsideLoad);
        }
        Ok(image)
    }

    /// The entry point. For these images it is also a physical address.
    #[must_use]
    pub fn entry(&self) -> u64 {
        self.entry
    }

    /// The `PT_LOAD` segments in program-header order. Each one has been
    /// checked to lie within the input.
    pub fn load_segments(&self) -> impl Iterator<Item = Result<Segment, ElfError>> + 'a {
        let data = self.data;
        let phoff = self.phoff;
        (0..self.phnum).filter_map(move |index| {
            let base = phoff + index * PHDR_SIZE;
            match u32_at(data, base) {
                Ok(PT_LOAD) => Some(read_segment(data, base)),
                Ok(_) => None,
                Err(error) => Some(Err(error)),
            }
        })
    }
}

fn read_segment(data: &[u8], base: usize) -> Result<Segment, ElfError> {
    let file_offset = u64_at(data, base + 8)?;
    let paddr = u64_at(data, base + 24)?;
    let file_size = u64_at(data, base + 32)?;
    let mem_size = u64_at(data, base + 40)?;
    if file_size > mem_size {
        return Err(ElfError::BadSegment);
    }
    let end = file_offset
        .checked_add(file_size)
        .ok_or(ElfError::BadSegment)?;
    if end > data.len() as u64 {
        return Err(ElfError::BadSegment);
    }
    Ok(Segment {
        file_offset,
        file_size,
        paddr,
        mem_size,
    })
}

fn bytes_at(data: &[u8], offset: usize, len: usize) -> Result<&[u8], ElfError> {
    let end = offset.checked_add(len).ok_or(ElfError::TooShort)?;
    data.get(offset..end).ok_or(ElfError::TooShort)
}

fn u16_at(data: &[u8], offset: usize) -> Result<u16, ElfError> {
    let mut buf = [0u8; 2];
    buf.copy_from_slice(bytes_at(data, offset, 2)?);
    Ok(u16::from_le_bytes(buf))
}

fn u32_at(data: &[u8], offset: usize) -> Result<u32, ElfError> {
    let mut buf = [0u8; 4];
    buf.copy_from_slice(bytes_at(data, offset, 4)?);
    Ok(u32::from_le_bytes(buf))
}

fn u64_at(data: &[u8], offset: usize) -> Result<u64, ElfError> {
    let mut buf = [0u8; 8];
    buf.copy_from_slice(bytes_at(data, offset, 8)?);
    Ok(u64::from_le_bytes(buf))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A 64-byte ELF header, one 56-byte `PT_LOAD` header at offset 64, and
    /// 16 bytes of code at file offset 120.
    fn image(entry: u64, file_size: u64, mem_size: u64, paddr: u64) -> [u8; 136] {
        let mut b = [0u8; 136];
        b[..4].copy_from_slice(&ELF_MAGIC);
        b[4] = CLASS_64;
        b[5] = DATA_LSB;
        b[6] = 1;
        b[16..18].copy_from_slice(&ET_EXEC.to_le_bytes());
        b[18..20].copy_from_slice(&EM_X86_64.to_le_bytes());
        b[24..32].copy_from_slice(&entry.to_le_bytes());
        b[32..40].copy_from_slice(&64u64.to_le_bytes());
        b[54..56].copy_from_slice(&(PHDR_SIZE as u16).to_le_bytes());
        b[56..58].copy_from_slice(&1u16.to_le_bytes());
        let ph = 64;
        b[ph..ph + 4].copy_from_slice(&PT_LOAD.to_le_bytes());
        b[ph + 8..ph + 16].copy_from_slice(&120u64.to_le_bytes());
        b[ph + 16..ph + 24].copy_from_slice(&paddr.to_le_bytes());
        b[ph + 24..ph + 32].copy_from_slice(&paddr.to_le_bytes());
        b[ph + 32..ph + 40].copy_from_slice(&file_size.to_le_bytes());
        b[ph + 40..ph + 48].copy_from_slice(&mem_size.to_le_bytes());
        b
    }

    #[test]
    fn parses_a_valid_image() {
        let bytes = image(0x10_0000, 16, 32, 0x10_0000);
        let elf = Elf::parse(&bytes).unwrap();
        assert_eq!(elf.entry(), 0x10_0000);
        let first = elf.load_segments().next().unwrap().unwrap();
        assert_eq!(first.file_offset, 120);
        assert_eq!(first.file_size, 16);
        assert_eq!(first.mem_size, 32);
        assert_eq!(first.paddr, 0x10_0000);
    }

    #[test]
    fn rejects_bad_magic_and_class() {
        let mut bytes = image(0x10_0000, 16, 16, 0x10_0000);
        bytes[0] = 0;
        assert_eq!(Elf::parse(&bytes).unwrap_err(), ElfError::BadMagic);
        let mut bytes = image(0x10_0000, 16, 16, 0x10_0000);
        bytes[4] = 1;
        assert_eq!(Elf::parse(&bytes).unwrap_err(), ElfError::NotElf64);
    }

    #[test]
    fn rejects_wrong_type_and_machine() {
        let mut bytes = image(0x10_0000, 16, 16, 0x10_0000);
        bytes[16] = 3; // ET_DYN
        assert_eq!(Elf::parse(&bytes).unwrap_err(), ElfError::NotExecutable);
        let mut bytes = image(0x10_0000, 16, 16, 0x10_0000);
        bytes[18] = 0xb7; // AArch64
        assert_eq!(Elf::parse(&bytes).unwrap_err(), ElfError::WrongMachine);
    }

    #[test]
    fn rejects_segment_outside_the_file() {
        let bytes = image(0x10_0000, 64, 64, 0x10_0000);
        assert_eq!(Elf::parse(&bytes).unwrap_err(), ElfError::BadSegment);
    }

    #[test]
    fn rejects_file_size_larger_than_memory_size() {
        let bytes = image(0x10_0000, 16, 8, 0x10_0000);
        assert_eq!(Elf::parse(&bytes).unwrap_err(), ElfError::BadSegment);
    }

    #[test]
    fn rejects_entry_outside_loadable_memory() {
        let bytes = image(0x20_0000, 16, 16, 0x10_0000);
        assert_eq!(Elf::parse(&bytes).unwrap_err(), ElfError::EntryOutsideLoad);
    }

    #[test]
    fn rejects_truncated_input() {
        let bytes = image(0x10_0000, 16, 16, 0x10_0000);
        assert_eq!(Elf::parse(&bytes[..40]).unwrap_err(), ElfError::TooShort);
        assert_eq!(Elf::parse(&bytes[..100]).unwrap_err(), ElfError::BadSegment);
    }
}
