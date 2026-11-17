// SPDX-License-Identifier: MIT OR Apache-2.0
//! tpt-k HAL FDT parser: flattened device trees (DTB, devicetree spec v0.4).
//!
//! The parser reads the blob in place. It checks the header, walks the
//! structure block as events, and offers path lookups and `reg` decoding on
//! top of that walk. It allocates nothing.

#![no_std]

#[cfg(test)]
extern crate std;

#[cfg(test)]
mod fuzz_tests;

const MAGIC: u32 = 0xD00D_FEED;
const HEADER_LEN: usize = 40;
const TOKEN_BEGIN_NODE: u32 = 1;
const TOKEN_END_NODE: u32 = 2;
const TOKEN_PROP: u32 = 3;
const TOKEN_NOP: u32 = 4;
const TOKEN_END: u32 = 9;

/// Why a blob or a structure could not be read.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FdtError {
    /// The input ends before the header or a region does.
    TooShort,
    /// The first word is not `0xd00dfeed`.
    BadMagic,
    /// `totalsize` is too small or larger than the input.
    BadLength,
    /// A region offset or size runs outside the blob.
    BadOffset,
    /// The blob is too new for this parser (`last_comp_version` above 17).
    BadVersion,
    /// The structure block holds an unknown token.
    BadToken,
    /// A name is not NUL-terminated or not valid UTF-8.
    BadString,
}

fn be32(bytes: &[u8], offset: usize) -> Result<u32, FdtError> {
    let raw: [u8; 4] = bytes
        .get(offset..offset + 4)
        .and_then(|s| s.try_into().ok())
        .ok_or(FdtError::TooShort)?;
    Ok(u32::from_be_bytes(raw))
}

fn region(blob: &[u8], offset: u32, size: u32) -> Result<&[u8], FdtError> {
    let start = offset as usize;
    let end = start
        .checked_add(size as usize)
        .ok_or(FdtError::BadOffset)?;
    blob.get(start..end).ok_or(FdtError::BadOffset)
}

fn align4(n: usize) -> usize {
    (n + 3) & !3
}

/// A NUL-terminated name at `offset` in `buf`, and the offset just past its NUL.
fn name_at(buf: &[u8], offset: usize) -> Result<(&str, usize), FdtError> {
    let tail = buf.get(offset..).ok_or(FdtError::BadString)?;
    let nul = tail
        .iter()
        .position(|b| *b == 0)
        .ok_or(FdtError::BadString)?;
    let name = core::str::from_utf8(&tail[..nul]).map_err(|_| FdtError::BadString)?;
    Ok((name, offset + nul + 1))
}

/// A parsed flattened device tree.
#[derive(Clone, Copy, Debug)]
pub struct Fdt<'a> {
    structure: &'a [u8],
    strings: &'a [u8],
}

/// One item from the structure block.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Event<'a> {
    /// A node opens. The root node's name is empty.
    BeginNode(&'a str),
    /// The innermost open node closes.
    EndNode,
    /// A property of the innermost open node. `value` is the raw bytes.
    Property {
        /// Property name.
        name: &'a str,
        /// Raw property value, big-endian where a cell type says so.
        value: &'a [u8],
    },
}

impl<'a> Fdt<'a> {
    /// Checks the header and the region offsets. `blob` may be longer than the
    /// tree; only `totalsize` bytes are used.
    pub fn parse(blob: &'a [u8]) -> Result<Self, FdtError> {
        if blob.len() < HEADER_LEN {
            return Err(FdtError::TooShort);
        }
        if be32(blob, 0)? != MAGIC {
            return Err(FdtError::BadMagic);
        }
        let total = be32(blob, 4)? as usize;
        if total < HEADER_LEN || total > blob.len() {
            return Err(FdtError::BadLength);
        }
        let blob = &blob[..total];
        let off_struct = be32(blob, 8)?;
        let off_strings = be32(blob, 12)?;
        let last_comp = be32(blob, 24)?;
        let size_strings = be32(blob, 32)?;
        let size_struct = be32(blob, 36)?;
        if last_comp > 17 {
            return Err(FdtError::BadVersion);
        }
        Ok(Self {
            structure: region(blob, off_struct, size_struct)?,
            strings: region(blob, off_strings, size_strings)?,
        })
    }

    /// Every event in tree order, from the root's `BeginNode` to its `EndNode`.
    #[must_use]
    pub fn events(&self) -> Events<'a> {
        Events {
            structure: self.structure,
            strings: self.strings,
            pos: 0,
            done: false,
        }
    }

    /// The value of `name` on the node at `path`, such as `/` or
    /// `/soc/uart@9000000`. A path component without `@` matches a node name
    /// with any unit address, so `/soc/uart` finds `uart@9000000`.
    #[must_use]
    pub fn property(&self, path: &str, name: &str) -> Option<&'a [u8]> {
        let target_depth = path.split('/').filter(|c| !c.is_empty()).count();
        let mut open = 0usize;
        // Nodes 0..matched are on the path, counting the root as node 0.
        let mut matched = 0usize;
        for event in self.events() {
            match event.ok()? {
                Event::BeginNode(node) => {
                    let index = open;
                    open += 1;
                    if index == 0 {
                        matched = 1;
                    } else if matched == index {
                        let component = path.split('/').filter(|c| !c.is_empty()).nth(index - 1);
                        if component.is_some_and(|c| name_matches(node, c)) {
                            matched = index + 1;
                        }
                    }
                }
                Event::EndNode => {
                    open = open.saturating_sub(1);
                    if matched > open {
                        matched = open;
                    }
                }
                Event::Property { name: prop, value } => {
                    if open == target_depth + 1 && matched == target_depth + 1 && prop == name {
                        return Some(value);
                    }
                }
            }
        }
        None
    }

    /// The `#address-cells` and `#size-cells` of the root, with the devicetree
    /// defaults of 2 and 1 when they are absent.
    #[must_use]
    pub fn root_cells(&self) -> (usize, usize) {
        let read = |name| {
            self.property("/", name)
                .and_then(|v| v.get(0..4))
                .map(|v| u32::from_be_bytes([v[0], v[1], v[2], v[3]]) as usize)
        };
        (
            read("#address-cells").unwrap_or(2),
            read("#size-cells").unwrap_or(1),
        )
    }

    /// Calls `f(base, size)` for each `reg` range of every top-level node whose
    /// name is `memory` or starts with `memory@`. The cells come from the root.
    pub fn memory_regions<F: FnMut(u64, u64)>(&self, mut f: F) {
        let (addr_cells, size_cells) = self.root_cells();
        let mut open = 0usize;
        let mut in_memory = false;
        for event in self.events() {
            let Ok(event) = event else { return };
            match event {
                Event::BeginNode(node) => {
                    open += 1;
                    if open == 2 {
                        in_memory = name_matches(node, "memory");
                    }
                }
                Event::EndNode => {
                    open = open.saturating_sub(1);
                    if open < 2 {
                        in_memory = false;
                    }
                }
                Event::Property { name, value } => {
                    if open == 2 && in_memory && name == "reg" {
                        for (base, size) in RegIter::new(value, addr_cells, size_cells) {
                            f(base, size);
                        }
                    }
                }
            }
        }
    }
}

/// True if a node name matches a path component. A component without `@`
/// matches any unit address.
fn name_matches(node: &str, component: &str) -> bool {
    if node == component {
        return true;
    }
    if component.contains('@') {
        return false;
    }
    node.split('@').next() == Some(component)
}

/// Iterator over the events of the structure block.
#[derive(Clone, Debug)]
pub struct Events<'a> {
    structure: &'a [u8],
    strings: &'a [u8],
    pos: usize,
    done: bool,
}

impl<'a> Iterator for Events<'a> {
    type Item = Result<Event<'a>, FdtError>;

    fn next(&mut self) -> Option<Self::Item> {
        if self.done {
            return None;
        }
        loop {
            let token = match be32(self.structure, self.pos) {
                Ok(t) => t,
                Err(e) => {
                    self.done = true;
                    return Some(Err(e));
                }
            };
            self.pos += 4;
            let result = match token {
                TOKEN_BEGIN_NODE => match name_at(self.structure, self.pos) {
                    Ok((name, next)) => {
                        self.pos = align4(next);
                        Ok(Event::BeginNode(name))
                    }
                    Err(e) => Err(e),
                },
                TOKEN_END_NODE => Ok(Event::EndNode),
                TOKEN_PROP => self.property(),
                TOKEN_NOP => continue,
                TOKEN_END => {
                    self.done = true;
                    return None;
                }
                _ => Err(FdtError::BadToken),
            };
            if result.is_err() {
                self.done = true;
            }
            return Some(result);
        }
    }
}

impl<'a> Events<'a> {
    fn property(&mut self) -> Result<Event<'a>, FdtError> {
        let len = be32(self.structure, self.pos)? as usize;
        let name_off = be32(self.structure, self.pos + 4)? as usize;
        let start = self.pos + 8;
        let value = self
            .structure
            .get(start..start + len)
            .ok_or(FdtError::BadOffset)?;
        self.pos = align4(start + len);
        let (name, _) = name_at(self.strings, name_off)?;
        Ok(Event::Property { name, value })
    }
}

/// Iterator over `(base, size)` pairs in a `reg` property.
#[derive(Clone, Debug)]
pub struct RegIter<'a> {
    rest: &'a [u8],
    addr_cells: usize,
    size_cells: usize,
}

impl<'a> RegIter<'a> {
    /// Decodes `value` as a list of `(address, size)` pairs. A cell count of
    /// zero means the field is absent and reads as zero. Cell counts above two
    /// are not supported, and the iterator stops at the first such pair.
    #[must_use]
    pub fn new(value: &'a [u8], addr_cells: usize, size_cells: usize) -> Self {
        Self {
            rest: value,
            addr_cells,
            size_cells,
        }
    }
}

fn read_cells(bytes: &[u8], cells: usize) -> Option<(u64, &[u8])> {
    if cells > 2 {
        return None;
    }
    let width = cells * 4;
    let (head, tail) = (bytes.get(..width)?, bytes.get(width..)?);
    let word = |at: usize| u32::from_be_bytes([head[at], head[at + 1], head[at + 2], head[at + 3]]);
    let value = match cells {
        0 => 0,
        1 => word(0) as u64,
        _ => ((word(0) as u64) << 32) | word(4) as u64,
    };
    Some((value, tail))
}

impl Iterator for RegIter<'_> {
    type Item = (u64, u64);

    fn next(&mut self) -> Option<(u64, u64)> {
        let (base, rest) = read_cells(self.rest, self.addr_cells)?;
        let (size, rest) = read_cells(rest, self.size_cells)?;
        self.rest = rest;
        Some((base, size))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::vec::Vec;

    /// Builds a DTB with the structure and strings blocks in order.
    struct Builder {
        structure: Vec<u8>,
        strings: Vec<u8>,
    }

    impl Builder {
        fn new() -> Self {
            Self {
                structure: Vec::new(),
                strings: Vec::new(),
            }
        }

        fn token(&mut self, t: u32) {
            self.structure.extend_from_slice(&t.to_be_bytes());
        }

        fn begin(&mut self, name: &str) {
            self.token(TOKEN_BEGIN_NODE);
            self.structure.extend_from_slice(name.as_bytes());
            self.structure.push(0);
            while !self.structure.len().is_multiple_of(4) {
                self.structure.push(0);
            }
        }

        fn end(&mut self) {
            self.token(TOKEN_END_NODE);
        }

        fn prop(&mut self, name: &str, value: &[u8]) {
            let off = match self
                .strings
                .windows(name.len() + 1)
                .position(|w| w[..name.len()] == *name.as_bytes() && w[name.len()] == 0)
            {
                Some(o) => o,
                None => {
                    let o = self.strings.len();
                    self.strings.extend_from_slice(name.as_bytes());
                    self.strings.push(0);
                    o
                }
            };
            self.token(TOKEN_PROP);
            self.token(value.len() as u32);
            self.token(off as u32);
            self.structure.extend_from_slice(value);
            while !self.structure.len().is_multiple_of(4) {
                self.structure.push(0);
            }
        }

        fn finish(mut self) -> Vec<u8> {
            self.token(TOKEN_END);
            let off_struct = HEADER_LEN + 16; // one empty reservation entry
            let off_strings = off_struct + self.structure.len();
            let total = off_strings + self.strings.len();
            let mut blob = Vec::new();
            let header = [
                MAGIC,
                total as u32,
                off_struct as u32,
                off_strings as u32,
                HEADER_LEN as u32,
                17,
                16,
                0,
                self.strings.len() as u32,
                self.structure.len() as u32,
            ];
            for w in header {
                blob.extend_from_slice(&w.to_be_bytes());
            }
            blob.extend_from_slice(&[0; 16]); // empty memory reservation block
            blob.extend_from_slice(&self.structure);
            blob.extend_from_slice(&self.strings);
            blob
        }
    }

    fn cells(values: &[u32]) -> Vec<u8> {
        values.iter().flat_map(|v| v.to_be_bytes()).collect()
    }

    fn sample() -> Vec<u8> {
        let mut b = Builder::new();
        b.begin("");
        b.prop("#address-cells", &cells(&[2]));
        b.prop("#size-cells", &cells(&[1]));
        b.begin("memory@80000000");
        b.prop("device_type", b"memory\0");
        b.prop("reg", &cells(&[0, 0x8000_0000, 0x4000_0000]));
        b.end();
        b.begin("soc");
        b.prop("#address-cells", &cells(&[1]));
        b.begin("uart@9000000");
        b.prop("compatible", b"arm,pl011\0");
        b.end();
        b.end();
        b.end();
        b.finish()
    }

    #[test]
    fn parse_rejects_bad_headers() {
        let blob = sample();
        assert!(Fdt::parse(&blob).is_ok());
        let mut bad = blob.clone();
        bad[0] = 0;
        assert_eq!(Fdt::parse(&bad).unwrap_err(), FdtError::BadMagic);
        assert_eq!(Fdt::parse(&blob[..20]).unwrap_err(), FdtError::TooShort);
        let mut long = blob.clone();
        long[7] = 0xFF;
        assert_eq!(Fdt::parse(&long).unwrap_err(), FdtError::BadLength);
        let mut newer = blob.clone();
        newer[24..28].copy_from_slice(&18u32.to_be_bytes());
        assert_eq!(Fdt::parse(&newer).unwrap_err(), FdtError::BadVersion);
    }

    #[test]
    fn events_walk_the_tree_in_order() {
        let blob = sample();
        let fdt = Fdt::parse(&blob).unwrap();
        let mut begins = Vec::new();
        let mut depth = 0i32;
        let mut max = 0;
        for ev in fdt.events() {
            match ev.unwrap() {
                Event::BeginNode(n) => {
                    begins.push(n);
                    depth += 1;
                    max = max.max(depth);
                }
                Event::EndNode => depth -= 1,
                Event::Property { .. } => {}
            }
        }
        assert_eq!(begins, ["", "memory@80000000", "soc", "uart@9000000"]);
        assert_eq!(depth, 0);
        assert_eq!(max, 3);
    }

    #[test]
    fn property_lookup_by_path() {
        let blob = sample();
        let fdt = Fdt::parse(&blob).unwrap();
        assert_eq!(
            fdt.property("/soc/uart@9000000", "compatible"),
            Some(&b"arm,pl011\0"[..])
        );
        // A path component without a unit address matches any unit address.
        assert_eq!(
            fdt.property("/soc/uart", "compatible"),
            Some(&b"arm,pl011\0"[..])
        );
        assert_eq!(
            fdt.property("/memory", "device_type"),
            Some(&b"memory\0"[..])
        );
        // Properties of a node belong to that node only.
        assert_eq!(fdt.property("/soc", "compatible"), None);
        assert_eq!(fdt.property("/nope", "compatible"), None);
        assert_eq!(fdt.property("/", "#size-cells"), Some(&cells(&[1])[..]));
    }

    #[test]
    fn root_cells_and_memory_regions() {
        let blob = sample();
        let fdt = Fdt::parse(&blob).unwrap();
        assert_eq!(fdt.root_cells(), (2, 1));
        let mut regions = Vec::new();
        fdt.memory_regions(|b, s| regions.push((b, s)));
        assert_eq!(regions, [(0x8000_0000, 0x4000_0000)]);
    }

    #[test]
    fn reg_iter_decodes_pairs_with_cell_counts() {
        let value = cells(&[0x1, 0x2, 0x3, 0x4, 0x5, 0x6]);
        let got: Vec<(u64, u64)> = RegIter::new(&value, 2, 2).collect();
        assert_eq!(got, [(0x1_0000_0002, 0x3_0000_0004)]);
        // With one address cell and one size cell, the same bytes give three pairs.
        let got: Vec<(u64, u64)> = RegIter::new(&value, 1, 1).collect();
        assert_eq!(got, [(1, 2), (3, 4), (5, 6)]);
        // A zero-cell size reads as zero and does not consume bytes.
        let got: Vec<(u64, u64)> = RegIter::new(&cells(&[0x10]), 1, 0).collect();
        assert_eq!(got, [(0x10, 0)]);
    }

    #[test]
    fn unknown_token_is_an_error() {
        let mut blob = sample();
        // The first structure token after the header is BEGIN_NODE; corrupt it.
        let off = HEADER_LEN + 16;
        blob[off..off + 4].copy_from_slice(&0x77u32.to_be_bytes());
        let fdt = Fdt::parse(&blob).unwrap();
        let first_err = fdt.events().find_map(Result::err);
        assert_eq!(first_err, Some(FdtError::BadToken));
    }
}
