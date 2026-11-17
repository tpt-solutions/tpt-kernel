// SPDX-License-Identifier: MIT OR Apache-2.0
//! tpt-k root spawn: an ELF64 load planner for the root server and its children.
//!
//! [`plan`] reads an x86-64 ELF executable and says where each loadable
//! segment goes, what it is made of, and where execution starts. It does not
//! copy bytes or map memory. The caller does that from the plan, so the parser
//! can be tested on plain byte slices and the kernel keeps the mapping code.
//!
//! The plan has a fixed capacity of [`MAX_SEGMENTS`] and uses no heap. The
//! planner refuses any image it cannot check: a writable and executable
//! segment, overlapping segments, a segment outside the input, or an entry
//! point that is not inside an executable segment.

#![no_std]

#[cfg(test)]
extern crate std;

pub mod lifecycle;

#[cfg(test)]
mod fuzz_tests;

/// Page size that segment placement is checked against, in bytes.
pub const PAGE_SIZE: u64 = 4096;
/// Most `PT_LOAD` segments a plan can hold.
pub const MAX_SEGMENTS: usize = 8;

const ELF_MAGIC: [u8; 4] = [0x7F, b'E', b'L', b'F'];
const CLASS_64: u8 = 2;
const DATA_LSB: u8 = 1;
const ET_EXEC: u16 = 2;
const ET_DYN: u16 = 3;
const EM_X86_64: u16 = 62;
const PT_LOAD: u32 = 1;
const PF_X: u32 = 1;
const PF_W: u32 = 2;
const PF_R: u32 = 4;
const EHDR_LEN: usize = 64;
const PHDR_LEN: usize = 56;

/// Why an image was refused.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ElfError {
    /// The input is shorter than the ELF header.
    TooShort,
    /// The first four bytes are not `\x7fELF`.
    BadMagic,
    /// The image is not 64-bit little-endian ELF.
    NotElf64Lsb,
    /// The image is not an executable (`ET_EXEC`) or position-independent (`ET_DYN`) file.
    NotExecutable,
    /// The image is not for x86-64.
    NotX86_64,
    /// The program header table is malformed or runs past the input.
    BadProgramHeaders,
    /// The image has no `PT_LOAD` segment.
    NoLoadSegments,
    /// More `PT_LOAD` segments than [`MAX_SEGMENTS`].
    TooManySegments,
    /// A segment's file bytes run past the input.
    SegmentOutsideImage,
    /// A segment's memory size is smaller than its file size, or zero, or its
    /// addresses wrap.
    BadSegmentSize,
    /// A segment's file offset and virtual address differ modulo the page size.
    Misaligned,
    /// A segment is both writable and executable.
    WritableExecutable,
    /// Two segments share a page.
    Overlap,
    /// The entry point is not inside an executable segment.
    BadEntry,
}

/// Access rights of a loadable segment.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Access {
    /// The segment may be read.
    pub read: bool,
    /// The segment may be written.
    pub write: bool,
    /// The segment may be executed.
    pub execute: bool,
}

/// One loadable segment: where it goes and what comes from the file.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Segment {
    /// Virtual address the segment is loaded at.
    pub vaddr: u64,
    /// Offset of the segment's file bytes in the image.
    pub file_offset: u64,
    /// Number of file bytes to copy.
    pub file_size: u64,
    /// Number of bytes the segment occupies in memory. The rest past
    /// `file_size` is zero-filled.
    pub mem_size: u64,
    /// Access rights.
    pub access: Access,
}

const EMPTY_SEGMENT: Segment = Segment {
    vaddr: 0,
    file_offset: 0,
    file_size: 0,
    mem_size: 0,
    access: Access {
        read: false,
        write: false,
        execute: false,
    },
};

/// A checked load plan.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Plan {
    entry: u64,
    count: usize,
    segments: [Segment; MAX_SEGMENTS],
}

impl Plan {
    /// The virtual address where execution starts.
    #[must_use]
    pub const fn entry(&self) -> u64 {
        self.entry
    }

    /// The loadable segments, in program-header order.
    #[must_use]
    pub fn segments(&self) -> &[Segment] {
        &self.segments[..self.count]
    }
}

fn u16_at(b: &[u8], at: usize) -> Result<u16, ElfError> {
    let raw = b.get(at..at + 2).ok_or(ElfError::TooShort)?;
    Ok(u16::from_le_bytes([raw[0], raw[1]]))
}

fn u32_at(b: &[u8], at: usize) -> Result<u32, ElfError> {
    let raw = b.get(at..at + 4).ok_or(ElfError::TooShort)?;
    Ok(u32::from_le_bytes([raw[0], raw[1], raw[2], raw[3]]))
}

fn u64_at(b: &[u8], at: usize) -> Result<u64, ElfError> {
    let raw = b.get(at..at + 8).ok_or(ElfError::TooShort)?;
    let mut v = [0u8; 8];
    v.copy_from_slice(raw);
    Ok(u64::from_le_bytes(v))
}

fn page_down(a: u64) -> u64 {
    a & !(PAGE_SIZE - 1)
}

fn page_up(a: u64) -> Option<u64> {
    a.checked_add(PAGE_SIZE - 1).map(page_down)
}

/// Checks an ELF64 x86-64 image and returns its load plan.
pub fn plan(image: &[u8]) -> Result<Plan, ElfError> {
    if image.len() < EHDR_LEN {
        return Err(ElfError::TooShort);
    }
    if image[0..4] != ELF_MAGIC {
        return Err(ElfError::BadMagic);
    }
    if image[4] != CLASS_64 || image[5] != DATA_LSB {
        return Err(ElfError::NotElf64Lsb);
    }
    let e_type = u16_at(image, 16)?;
    if e_type != ET_EXEC && e_type != ET_DYN {
        return Err(ElfError::NotExecutable);
    }
    if u16_at(image, 18)? != EM_X86_64 {
        return Err(ElfError::NotX86_64);
    }
    let entry = u64_at(image, 24)?;
    let phoff = u64_at(image, 32)?;
    let phentsize = u16_at(image, 54)? as usize;
    let phnum = u16_at(image, 56)? as usize;
    if phentsize != PHDR_LEN || phnum == 0 {
        return Err(ElfError::BadProgramHeaders);
    }
    let table_end = phoff
        .checked_add((phnum * PHDR_LEN) as u64)
        .ok_or(ElfError::BadProgramHeaders)?;
    if table_end > image.len() as u64 {
        return Err(ElfError::BadProgramHeaders);
    }

    let mut out = Plan {
        entry,
        count: 0,
        segments: [EMPTY_SEGMENT; MAX_SEGMENTS],
    };
    for i in 0..phnum {
        let at = phoff as usize + i * PHDR_LEN;
        if u32_at(image, at)? != PT_LOAD {
            continue;
        }
        let flags = u32_at(image, at + 4)?;
        let segment = Segment {
            file_offset: u64_at(image, at + 8)?,
            vaddr: u64_at(image, at + 16)?,
            file_size: u64_at(image, at + 32)?,
            mem_size: u64_at(image, at + 40)?,
            access: Access {
                read: flags & PF_R != 0,
                write: flags & PF_W != 0,
                execute: flags & PF_X != 0,
            },
        };
        check_segment(image, &segment)?;
        if out.count == MAX_SEGMENTS {
            return Err(ElfError::TooManySegments);
        }
        out.segments[out.count] = segment;
        out.count += 1;
    }

    if out.count == 0 {
        return Err(ElfError::NoLoadSegments);
    }
    let segs = out.segments();
    for (i, a) in segs.iter().enumerate() {
        for b in &segs[i + 1..] {
            if pages_overlap(a, b)? {
                return Err(ElfError::Overlap);
            }
        }
    }
    let entry_ok = segs
        .iter()
        .any(|s| s.access.execute && entry >= s.vaddr && entry < s.vaddr + s.mem_size);
    if !entry_ok {
        return Err(ElfError::BadEntry);
    }
    Ok(out)
}

fn check_segment(image: &[u8], s: &Segment) -> Result<(), ElfError> {
    if s.mem_size == 0 || s.mem_size < s.file_size {
        return Err(ElfError::BadSegmentSize);
    }
    s.vaddr
        .checked_add(s.mem_size)
        .ok_or(ElfError::BadSegmentSize)?;
    let file_end = s
        .file_offset
        .checked_add(s.file_size)
        .ok_or(ElfError::SegmentOutsideImage)?;
    if file_end > image.len() as u64 {
        return Err(ElfError::SegmentOutsideImage);
    }
    if s.file_offset % PAGE_SIZE != s.vaddr % PAGE_SIZE {
        return Err(ElfError::Misaligned);
    }
    if s.access.write && s.access.execute {
        return Err(ElfError::WritableExecutable);
    }
    Ok(())
}

/// True if two segments map any page in common. Segments are compared by the
/// pages they cover, since the MMU maps whole pages.
fn pages_overlap(a: &Segment, b: &Segment) -> Result<bool, ElfError> {
    let a_start = page_down(a.vaddr);
    let a_end = page_up(a.vaddr + a.mem_size).ok_or(ElfError::BadSegmentSize)?;
    let b_start = page_down(b.vaddr);
    let b_end = page_up(b.vaddr + b.mem_size).ok_or(ElfError::BadSegmentSize)?;
    Ok(a_start < b_end && b_start < a_end)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::vec::Vec;

    /// One program header to put in a test image.
    #[derive(Clone, Copy)]
    struct Ph {
        kind: u32,
        flags: u32,
        offset: u64,
        vaddr: u64,
        filesz: u64,
        memsz: u64,
    }

    fn load(flags: u32, offset: u64, vaddr: u64, filesz: u64, memsz: u64) -> Ph {
        Ph {
            kind: PT_LOAD,
            flags,
            offset,
            vaddr,
            filesz,
            memsz,
        }
    }

    /// Builds an image: header, program headers, then `body` at file offset
    /// 0x1000 (so page-aligned segments can point at it).
    fn image(e_type: u16, machine: u16, entry: u64, phs: &[Ph], body_len: usize) -> Vec<u8> {
        let mut b = std::vec![0u8; 0x1000 + body_len];
        b[0..4].copy_from_slice(&ELF_MAGIC);
        b[4] = CLASS_64;
        b[5] = DATA_LSB;
        b[6] = 1;
        b[16..18].copy_from_slice(&e_type.to_le_bytes());
        b[18..20].copy_from_slice(&machine.to_le_bytes());
        b[24..32].copy_from_slice(&entry.to_le_bytes());
        b[32..40].copy_from_slice(&(EHDR_LEN as u64).to_le_bytes());
        b[52..54].copy_from_slice(&(EHDR_LEN as u16).to_le_bytes());
        b[54..56].copy_from_slice(&(PHDR_LEN as u16).to_le_bytes());
        b[56..58].copy_from_slice(&(phs.len() as u16).to_le_bytes());
        for (i, p) in phs.iter().enumerate() {
            let at = EHDR_LEN + i * PHDR_LEN;
            b[at..at + 4].copy_from_slice(&p.kind.to_le_bytes());
            b[at + 4..at + 8].copy_from_slice(&p.flags.to_le_bytes());
            b[at + 8..at + 16].copy_from_slice(&p.offset.to_le_bytes());
            b[at + 16..at + 24].copy_from_slice(&p.vaddr.to_le_bytes());
            b[at + 24..at + 32].copy_from_slice(&p.vaddr.to_le_bytes());
            b[at + 32..at + 40].copy_from_slice(&p.filesz.to_le_bytes());
            b[at + 40..at + 48].copy_from_slice(&p.memsz.to_le_bytes());
            b[at + 48..at + 56].copy_from_slice(&PAGE_SIZE.to_le_bytes());
        }
        b
    }

    const RX: u32 = PF_R | PF_X;
    const RW: u32 = PF_R | PF_W;

    fn good_image(phs: &[Ph]) -> Vec<u8> {
        image(ET_EXEC, EM_X86_64, 0x40_1000, phs, 0x3000)
    }

    #[test]
    fn plans_text_and_data_segments() {
        let img = good_image(&[
            load(RX, 0x1000, 0x40_1000, 0x800, 0x800),
            load(RW, 0x2000, 0x40_2000, 0x100, 0x1000),
        ]);
        let p = plan(&img).unwrap();
        assert_eq!(p.entry(), 0x40_1000);
        let segs = p.segments();
        assert_eq!(segs.len(), 2);
        assert_eq!(
            segs[0],
            Segment {
                vaddr: 0x40_1000,
                file_offset: 0x1000,
                file_size: 0x800,
                mem_size: 0x800,
                access: Access {
                    read: true,
                    write: false,
                    execute: true
                },
            }
        );
        assert!(segs[1].access.write && !segs[1].access.execute);
        assert_eq!(segs[1].mem_size, 0x1000);
    }

    #[test]
    fn accepts_position_independent_images() {
        let img = image(
            ET_DYN,
            EM_X86_64,
            0x1000,
            &[load(RX, 0x1000, 0x1000, 0x100, 0x100)],
            0x100,
        );
        assert!(plan(&img).is_ok());
    }

    #[test]
    fn ignores_non_load_headers() {
        let text = load(RX, 0x1000, 0x40_1000, 0x100, 0x100);
        let note = Ph { kind: 4, ..text };
        let img = good_image(&[note, text]);
        assert_eq!(plan(&img).unwrap().segments().len(), 1);
    }

    #[test]
    fn rejects_bad_identity() {
        let mut img = good_image(&[load(RX, 0x1000, 0x40_1000, 0x100, 0x100)]);
        img[0] = 0;
        assert_eq!(plan(&img), Err(ElfError::BadMagic));
        let mut img = good_image(&[load(RX, 0x1000, 0x40_1000, 0x100, 0x100)]);
        img[4] = 1;
        assert_eq!(plan(&img), Err(ElfError::NotElf64Lsb));
        let mut img = good_image(&[load(RX, 0x1000, 0x40_1000, 0x100, 0x100)]);
        img[5] = 2;
        assert_eq!(plan(&img), Err(ElfError::NotElf64Lsb));
        assert_eq!(plan(&img[..40]), Err(ElfError::TooShort));
    }

    #[test]
    fn rejects_wrong_type_and_machine() {
        let phs = [load(RX, 0x1000, 0x40_1000, 0x100, 0x100)];
        let rel = image(1, EM_X86_64, 0x40_1000, &phs, 0x100);
        assert_eq!(plan(&rel), Err(ElfError::NotExecutable));
        let arm = image(ET_EXEC, 183, 0x40_1000, &phs, 0x100);
        assert_eq!(plan(&arm), Err(ElfError::NotX86_64));
    }

    #[test]
    fn rejects_program_table_past_the_input() {
        let phs = [load(RX, 0x1000, 0x40_1000, 0x100, 0x100)];
        let mut img = good_image(&phs);
        img[32..40].copy_from_slice(&0xFFFF_FFFFu64.to_le_bytes());
        assert_eq!(plan(&img), Err(ElfError::BadProgramHeaders));
        let mut img = good_image(&phs);
        img[56..58].copy_from_slice(&0u16.to_le_bytes());
        assert_eq!(plan(&img), Err(ElfError::BadProgramHeaders));
    }

    #[test]
    fn rejects_segment_bytes_past_the_input() {
        let img = good_image(&[load(RX, 0x1000, 0x40_1000, 0x10_0000, 0x10_0000)]);
        assert_eq!(plan(&img), Err(ElfError::SegmentOutsideImage));
    }

    #[test]
    fn rejects_memsz_below_filesz_and_zero_size() {
        let img = good_image(&[load(RX, 0x1000, 0x40_1000, 0x200, 0x100)]);
        assert_eq!(plan(&img), Err(ElfError::BadSegmentSize));
        let img = good_image(&[load(RX, 0x1000, 0x40_1000, 0, 0)]);
        assert_eq!(plan(&img), Err(ElfError::BadSegmentSize));
    }

    #[test]
    fn rejects_writable_executable_segment() {
        let img = good_image(&[load(RX | PF_W, 0x1000, 0x40_1000, 0x100, 0x100)]);
        assert_eq!(plan(&img), Err(ElfError::WritableExecutable));
    }

    #[test]
    fn rejects_misaligned_offset() {
        // File offset 0x1010 but address 0x401000: not the same page offset.
        let img = good_image(&[load(RX, 0x1010, 0x40_1000, 0x100, 0x100)]);
        assert_eq!(plan(&img), Err(ElfError::Misaligned));
    }

    #[test]
    fn rejects_segments_sharing_a_page() {
        let img = good_image(&[
            load(RX, 0x1000, 0x40_1000, 0x100, 0x100),
            load(RW, 0x2800, 0x40_1800, 0x100, 0x100),
        ]);
        assert_eq!(plan(&img), Err(ElfError::Overlap));
    }

    #[test]
    fn rejects_more_than_max_segments() {
        let mut phs = Vec::new();
        for i in 0..(MAX_SEGMENTS as u64 + 1) {
            phs.push(load(RW, 0x1000, 0x40_0000 + i * 0x1000, 0x10, 0x10));
        }
        let img = good_image(&phs);
        assert_eq!(plan(&img), Err(ElfError::TooManySegments));
    }

    #[test]
    fn rejects_entry_outside_executable_segment() {
        let img = image(
            ET_EXEC,
            EM_X86_64,
            0x40_2000,
            &[load(RX, 0x1000, 0x40_1000, 0x100, 0x100)],
            0x100,
        );
        assert_eq!(plan(&img), Err(ElfError::BadEntry));
        // Entry inside a writable-only segment is not executable either.
        let img = image(
            ET_EXEC,
            EM_X86_64,
            0x40_2000,
            &[load(RW, 0x2000, 0x40_2000, 0x100, 0x100)],
            0x3000,
        );
        assert_eq!(plan(&img), Err(ElfError::BadEntry));
    }

    #[test]
    fn no_load_segments_is_an_error() {
        let note = Ph {
            kind: 4,
            flags: 0,
            offset: 0x1000,
            vaddr: 0,
            filesz: 0,
            memsz: 0,
        };
        let img = good_image(&[note]);
        assert_eq!(plan(&img), Err(ElfError::NoLoadSegments));
    }
}
