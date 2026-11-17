// SPDX-License-Identifier: MIT OR Apache-2.0
//! Device discovery in AML: walks the DSDT and SSDT bodies and records each
//! `Device` with its `_HID` and `_ADR`.
//!
//! This is not an AML interpreter. Control methods are skipped by their
//! PkgLength and never evaluated. Operation regions, fields and other terms
//! whose contents would need interpreting stop the walk with
//! [`AmlError::Unsupported`], so a malformed or unfamiliar table is refused
//! rather than guessed at. Blocks skipped by length are not inspected.
//!
//! The walk holds no heap data. Paths and devices live in fixed arrays of
//! [`MAX_DEPTH`] name segments and [`MAX_DEVICES`] devices.

use core::fmt;

/// Deepest scope nesting, and the most name segments in one path.
pub const MAX_DEPTH: usize = 8;
/// Most devices one walk can record.
pub const MAX_DEVICES: usize = 32;

/// Why a body could not be walked. Offsets are relative to the start of the
/// body passed to [`discover`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AmlError {
    /// The body ends inside a term or a PkgLength.
    Truncated {
        /// Offset where the bytes ran out.
        offset: usize,
    },
    /// A PkgLength is too small to cover its own bytes, or a block runs past
    /// its parent.
    BadLength {
        /// Offset of the bad length.
        offset: usize,
    },
    /// A name is not a valid NameString, or it climbs above the root.
    BadName {
        /// Offset of the name.
        offset: usize,
    },
    /// A term is outside what this walker understands.
    Unsupported {
        /// Offset of the term.
        offset: usize,
    },
    /// Scopes nest deeper than [`MAX_DEPTH`], or a path has too many segments.
    TooDeep,
    /// More than [`MAX_DEVICES`] devices were found.
    Full,
}

/// An absolute ACPI namespace path, such as `\_SB_.PCI0`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Path {
    segs: [[u8; 4]; MAX_DEPTH],
    len: u8,
}

impl Path {
    /// The namespace root, `\`.
    pub const ROOT: Self = Self {
        segs: [[0; 4]; MAX_DEPTH],
        len: 0,
    };

    /// The name segments, outermost first. Each is four ASCII bytes.
    pub fn segments(&self) -> impl Iterator<Item = &[u8; 4]> + '_ {
        self.segs[..self.len as usize].iter()
    }

    /// Number of segments below the root.
    #[must_use]
    pub fn depth(&self) -> usize {
        self.len as usize
    }

    fn push(&mut self, seg: [u8; 4]) -> Result<(), AmlError> {
        if self.depth() == MAX_DEPTH {
            return Err(AmlError::TooDeep);
        }
        self.segs[self.depth()] = seg;
        self.len += 1;
        Ok(())
    }

    fn pop(&mut self) -> bool {
        if self.len == 0 {
            false
        } else {
            self.len -= 1;
            true
        }
    }
}

impl fmt::Display for Path {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("\\")?;
        for (i, seg) in self.segments().enumerate() {
            if i > 0 {
                f.write_str(".")?;
            }
            // Name segments are validated ASCII when they are parsed.
            f.write_str(core::str::from_utf8(seg).unwrap_or("????"))?;
        }
        Ok(())
    }
}

/// The value of `_HID`: a compressed EISA ID, or a string.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Id<'a> {
    /// A four-byte compressed EISA ID, as stored by a DWordConst.
    Eisa(u32),
    /// A string ID, such as `PNP0A03` written as a string.
    Str(&'a str),
}

/// A device found in the namespace.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Device<'a> {
    /// Where the device sits in the namespace.
    pub path: Path,
    /// The `_HID`, if the device declares one with a DWordConst or a string.
    pub hid: Option<Id<'a>>,
    /// The `_ADR`, if it is a DWordConst.
    pub adr: Option<u32>,
}

/// The devices found by one walk, in the order they appear in the table.
#[derive(Clone, Copy, Debug)]
pub struct Devices<'a> {
    items: [Option<Device<'a>>; MAX_DEVICES],
    len: usize,
}

impl<'a> Devices<'a> {
    fn empty() -> Self {
        Self {
            items: [None; MAX_DEVICES],
            len: 0,
        }
    }

    fn push(&mut self, device: Device<'a>) -> Result<usize, AmlError> {
        if self.len == MAX_DEVICES {
            return Err(AmlError::Full);
        }
        self.items[self.len] = Some(device);
        self.len += 1;
        Ok(self.len - 1)
    }

    /// The devices in table order, outer devices before the devices nested in them.
    pub fn iter(&self) -> impl Iterator<Item = &Device<'a>> + '_ {
        self.items[..self.len].iter().filter_map(Option::as_ref)
    }

    /// Number of devices found.
    #[must_use]
    pub fn count(&self) -> usize {
        self.len
    }
}

/// Walks an AML body (a DSDT or SSDT body, without its header) and records
/// every device in it.
pub fn discover(body: &[u8]) -> Result<Devices<'_>, AmlError> {
    let mut out = Devices::empty();
    walk(body, 0, Path::ROOT, &mut out, None, 0)?;
    Ok(out)
}

/// [`discover`] for a parsed system description table.
pub fn discover_table<'a>(sdt: &crate::Sdt<'a>) -> Result<Devices<'a>, AmlError> {
    discover(sdt.body())
}

// --- byte access with offsets relative to the body --------------------------

fn byte(data: &[u8], at: usize, base: usize) -> Result<u8, AmlError> {
    data.get(at)
        .copied()
        .ok_or(AmlError::Truncated { offset: base + at })
}

fn slice(data: &[u8], start: usize, end: usize, base: usize) -> Result<&[u8], AmlError> {
    if start > end || end > data.len() {
        return Err(AmlError::BadLength {
            offset: base + start,
        });
    }
    Ok(&data[start..end])
}

/// Reads a PkgLength at `at`. Returns the absolute end of the block (the
/// PkgLength counts its own bytes) and the offset where the block's contents start.
fn pkg_length(data: &[u8], at: usize, base: usize) -> Result<(usize, usize), AmlError> {
    let lead = byte(data, at, base)?;
    let extra = (lead >> 6) as usize;
    let mut len = if extra == 0 {
        (lead & 0x3F) as usize
    } else {
        let mut len = (lead & 0x0F) as usize;
        for i in 0..extra {
            len |= (byte(data, at + 1 + i, base)? as usize) << (4 + 8 * i);
        }
        len
    };
    let header = 1 + extra;
    if len < header {
        return Err(AmlError::BadLength { offset: base + at });
    }
    let end = at
        .checked_add(len)
        .ok_or(AmlError::BadLength { offset: base + at })?;
    if end > data.len() {
        return Err(AmlError::Truncated { offset: base + at });
    }
    len = end;
    Ok((len, at + header))
}

/// Reads one NameSeg: a letter or underscore, then letters, digits or underscores.
fn seg_at(data: &[u8], at: usize, base: usize) -> Result<[u8; 4], AmlError> {
    let raw =
        slice(data, at, at + 4, base).map_err(|_| AmlError::Truncated { offset: base + at })?;
    let mut seg = [0u8; 4];
    seg.copy_from_slice(raw);
    let lead_ok = seg[0].is_ascii_uppercase() || seg[0] == b'_';
    let rest_ok = seg[1..]
        .iter()
        .all(|&c| c.is_ascii_uppercase() || c.is_ascii_digit() || c == b'_');
    if lead_ok && rest_ok {
        Ok(seg)
    } else {
        Err(AmlError::BadName { offset: base + at })
    }
}

/// A parsed NameString, before it is resolved against a scope.
#[derive(Clone, Copy, Debug)]
struct Name {
    root: bool,
    parents: usize,
    segs: Path,
    offset: usize,
}

impl Name {
    /// True for a single, relative segment equal to `wanted`.
    fn is_single(&self, wanted: &[u8; 4]) -> bool {
        !self.root && self.parents == 0 && self.segs.depth() == 1 && self.segs.segs[0] == *wanted
    }
}

/// Reads a NameString at `at` and returns it with the offset just past it.
fn name_at(data: &[u8], at: usize, base: usize) -> Result<(Name, usize), AmlError> {
    let mut i = at;
    let mut root = false;
    let mut parents = 0usize;
    if byte(data, i, base)? == b'\\' {
        root = true;
        i += 1;
    } else {
        while byte(data, i, base)? == b'^' {
            parents += 1;
            i += 1;
            if parents > MAX_DEPTH {
                return Err(AmlError::BadName { offset: base + at });
            }
        }
    }
    let mut segs = Path::ROOT;
    match byte(data, i, base)? {
        0x00 => i += 1,
        0x2E => {
            i += 1;
            for _ in 0..2 {
                segs.push(seg_at(data, i, base)?)?;
                i += 4;
            }
        }
        0x2F => {
            let count = byte(data, i + 1, base)? as usize;
            i += 2;
            for _ in 0..count {
                segs.push(seg_at(data, i, base)?)?;
                i += 4;
            }
        }
        _ => {
            segs.push(seg_at(data, i, base)?)?;
            i += 4;
        }
    }
    Ok((
        Name {
            root,
            parents,
            segs,
            offset: base + at,
        },
        i,
    ))
}

/// Resolves a name against the current scope.
fn resolve(scope: Path, name: &Name) -> Result<Path, AmlError> {
    let mut path = if name.root { Path::ROOT } else { scope };
    for _ in 0..name.parents {
        if !path.pop() {
            return Err(AmlError::BadName {
                offset: name.offset,
            });
        }
    }
    for seg in name.segs.segments() {
        path.push(*seg)?;
    }
    Ok(path)
}

/// The value of a data object in a Name.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Value<'a> {
    Dword(u32),
    Str(&'a str),
    Other,
}

/// Reads one data object at `at`. Only the constant forms a Name can hold are
/// understood. Anything else is unsupported.
fn data_object(data: &[u8], at: usize, base: usize) -> Result<(Value<'_>, usize), AmlError> {
    let unsupported = AmlError::Unsupported { offset: base + at };
    match byte(data, at, base)? {
        // ZeroOp, OneOp, OnesOp.
        0x00 | 0x01 | 0xFF => Ok((Value::Other, at + 1)),
        // BytePrefix.
        0x0A => {
            byte(data, at + 1, base)?;
            Ok((Value::Other, at + 2))
        }
        // WordPrefix.
        0x0B => {
            slice(data, at + 1, at + 3, base)?;
            Ok((Value::Other, at + 3))
        }
        // DWordPrefix.
        0x0C => {
            let raw = slice(data, at + 1, at + 5, base)?;
            let mut v = [0u8; 4];
            v.copy_from_slice(raw);
            Ok((Value::Dword(u32::from_le_bytes(v)), at + 5))
        }
        // QWordPrefix.
        0x0E => {
            slice(data, at + 1, at + 9, base)?;
            Ok((Value::Other, at + 9))
        }
        // StringPrefix: a NUL-terminated ASCII string.
        0x0D => {
            let tail = data
                .get(at + 1..)
                .ok_or(AmlError::Truncated { offset: base + at })?;
            let nul = tail
                .iter()
                .position(|&b| b == 0)
                .ok_or(AmlError::Truncated { offset: base + at })?;
            let text = core::str::from_utf8(&tail[..nul]).map_err(|_| unsupported)?;
            Ok((Value::Str(text), at + 1 + nul + 1))
        }
        _ => Err(unsupported),
    }
}

/// Walks one term list. `owner` is the index of the device whose body this is,
/// if any; `_HID` and `_ADR` names are recorded against it.
fn walk<'a>(
    data: &'a [u8],
    base: usize,
    scope: Path,
    out: &mut Devices<'a>,
    owner: Option<usize>,
    depth: usize,
) -> Result<(), AmlError> {
    if depth > MAX_DEPTH {
        return Err(AmlError::TooDeep);
    }
    let mut pos = 0usize;
    while pos < data.len() {
        match byte(data, pos, base)? {
            // ScopeOp: changes the scope for its body.
            0x10 => {
                let (end, body) = pkg_length(data, pos + 1, base)?;
                let (name, next) = name_at(data, body, base)?;
                let inner = resolve(scope, &name)?;
                walk(
                    slice(data, next, end, base)?,
                    base + next,
                    inner,
                    out,
                    None,
                    depth + 1,
                )?;
                pos = end;
            }
            // ExtOpPrefix.
            0x5B => match byte(data, pos + 1, base)? {
                // DeviceOp: records the device, then walks its body.
                0x82 => {
                    let (end, body) = pkg_length(data, pos + 2, base)?;
                    let (name, next) = name_at(data, body, base)?;
                    let path = resolve(scope, &name)?;
                    let index = out.push(Device {
                        path,
                        hid: None,
                        adr: None,
                    })?;
                    walk(
                        slice(data, next, end, base)?,
                        base + next,
                        path,
                        out,
                        Some(index),
                        depth + 1,
                    )?;
                    pos = end;
                }
                // Field, Processor, PowerResource, ThermalZone, IndexField and
                // BankField: skipped by length, their contents not read.
                0x81 | 0x83 | 0x84 | 0x85 | 0x86 | 0x87 => {
                    let (end, _) = pkg_length(data, pos + 2, base)?;
                    pos = end;
                }
                _ => return Err(AmlError::Unsupported { offset: base + pos }),
            },
            // MethodOp: skipped by length, never evaluated.
            0x14 => {
                let (end, _) = pkg_length(data, pos + 1, base)?;
                pos = end;
            }
            // NameOp: records _HID and _ADR for the enclosing device; any
            // other name's value is read only to step past it.
            0x08 => {
                let (name, next) = name_at(data, pos + 1, base)?;
                let (value, after) = data_object(data, next, base)?;
                if let Some(index) = owner {
                    if let Some(device) = out.items[index].as_mut() {
                        if name.is_single(b"_HID") {
                            device.hid = match value {
                                Value::Dword(v) => Some(Id::Eisa(v)),
                                Value::Str(s) => Some(Id::Str(s)),
                                Value::Other => device.hid,
                            };
                        } else if name.is_single(b"_ADR") {
                            if let Value::Dword(v) = value {
                                device.adr = Some(v);
                            }
                        }
                    }
                }
                pos = after;
            }
            _ => return Err(AmlError::Unsupported { offset: base + pos }),
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::string::ToString;
    use std::vec::Vec;

    /// Wraps `body` in a PkgLength that counts its own bytes.
    fn pkg(body: &[u8]) -> Vec<u8> {
        let mut out = Vec::new();
        if body.len() + 1 < 64 {
            out.push((body.len() + 1) as u8);
        } else {
            let total = body.len() + 2;
            assert!(total < 4096, "test body too large for two PkgLength bytes");
            out.push(0x40 | (total & 0x0F) as u8);
            out.push((total >> 4) as u8);
        }
        out.extend_from_slice(body);
        out
    }

    fn cat(parts: &[&[u8]]) -> Vec<u8> {
        parts.concat()
    }

    /// `_SB_` as a root-relative name: `\_SB_`.
    const SB: &[u8] = b"\\_SB_";

    fn name_hid_str(text: &[u8]) -> Vec<u8> {
        let mut v = std::vec![0x08];
        v.extend_from_slice(b"_HID");
        v.push(0x0D);
        v.extend_from_slice(text);
        v.push(0);
        v
    }

    fn name_adr(value: u32) -> Vec<u8> {
        let mut v = std::vec![0x08];
        v.extend_from_slice(b"_ADR");
        v.push(0x0C);
        v.extend_from_slice(&value.to_le_bytes());
        v
    }

    /// Scope(\_SB_) { Device(PCI0) { _HID "PNP0A03"; _ADR 0x00020000 } }
    fn pci_tree() -> Vec<u8> {
        let device_body = cat(&[b"PCI0", &name_hid_str(b"PNP0A03"), &name_adr(0x0002_0000)]);
        let device = cat(&[&[0x5B, 0x82], &pkg(&device_body)]);
        let scope_body = cat(&[SB, &device]);
        cat(&[&[0x10], &pkg(&scope_body)])
    }

    #[test]
    fn scope_and_device_with_hid_string_and_adr() {
        let aml = pci_tree();
        let devices = discover(&aml).unwrap();
        assert_eq!(devices.count(), 1);
        let d = devices.iter().next().unwrap();
        assert_eq!(d.path.to_string(), "\\_SB_.PCI0");
        assert_eq!(d.hid, Some(Id::Str("PNP0A03")));
        assert_eq!(d.adr, Some(0x0002_0000));
    }

    #[test]
    fn hid_as_dword_is_recorded_as_eisa() {
        let mut hid = std::vec![0x08];
        hid.extend_from_slice(b"_HID");
        hid.push(0x0C);
        hid.extend_from_slice(&0x0A03_41D0u32.to_le_bytes());
        let device = cat(&[&[0x5B, 0x82], &pkg(&cat(&[b"KBD0", &hid]))]);
        let devices = discover(&device).unwrap();
        let d = devices.iter().next().unwrap();
        assert_eq!(d.hid, Some(Id::Eisa(0x0A03_41D0)));
        assert_eq!(d.path.to_string(), "\\KBD0");
    }

    #[test]
    fn nested_device_is_found_after_its_parent() {
        let inner = cat(&[
            &[0x5B, 0x82],
            &pkg(&cat(&[b"ISA0", &name_hid_str(b"PNP0A00")])),
        ]);
        let outer_body = cat(&[b"PCI0", &name_adr(0), &inner]);
        let outer = cat(&[&[0x5B, 0x82], &pkg(&outer_body)]);
        let aml = cat(&[&[0x10], &pkg(&cat(&[SB, &outer]))]);
        let devices = discover(&aml).unwrap();
        let paths: Vec<std::string::String> = devices.iter().map(|d| d.path.to_string()).collect();
        assert_eq!(paths, ["\\_SB_.PCI0", "\\_SB_.PCI0.ISA0"]);
        // The _HID of the nested device belongs to it, not to its parent.
        let outer_dev = devices.iter().next().unwrap();
        assert_eq!(outer_dev.hid, None);
        assert_eq!(outer_dev.adr, Some(0));
        let inner_dev = devices.iter().nth(1).unwrap();
        assert_eq!(inner_dev.hid, Some(Id::Str("PNP0A00")));
    }

    #[test]
    fn method_body_is_skipped_without_interpretation() {
        // The method body holds an OperationRegion, which the walker does not
        // understand. Skipping by length means that does not matter.
        let method_body = cat(&[b"MTHD", &[0x00], &[0x5B, 0x80, 0xFF, 0xFF, 0xFF]]);
        let method = cat(&[&[0x14], &pkg(&method_body)]);
        let device_body = cat(&[b"PCI0", &method, &name_hid_str(b"PNP0A03")]);
        let device = cat(&[&[0x5B, 0x82], &pkg(&device_body)]);
        let devices = discover(&device).unwrap();
        assert_eq!(devices.iter().next().unwrap().hid, Some(Id::Str("PNP0A03")));
    }

    #[test]
    fn truncated_pkglength_is_refused() {
        // Two extra bytes announced, one present.
        let aml = [0x10, 0x80, 0x01];
        assert!(matches!(discover(&aml), Err(AmlError::Truncated { .. })));
        // A length that runs past the body.
        let aml = [0x10, 0x3F, 0x00];
        assert!(matches!(discover(&aml), Err(AmlError::Truncated { .. })));
    }

    #[test]
    fn unknown_term_inside_a_device_is_unsupported() {
        // OperationRegion has term arguments this walker does not read.
        let op_region = [0x5B, 0x80, b'O', b'P', b'R', b'0', 0x00];
        let device_body = cat(&[b"EC00", &op_region]);
        let device = cat(&[&[0x5B, 0x82], &pkg(&device_body)]);
        assert!(matches!(
            discover(&device),
            Err(AmlError::Unsupported { .. })
        ));
    }

    #[test]
    fn unrelated_names_are_stepped_over() {
        // Name(XYZ0, DWord) and Name(XYZ1, "text") are read and not recorded.
        let mut other = std::vec![0x08];
        other.extend_from_slice(b"XYZ0");
        other.push(0x0C);
        other.extend_from_slice(&[1, 2, 3, 4]);
        let mut text = std::vec![0x08];
        text.extend_from_slice(b"XYZ1");
        text.push(0x0D);
        text.extend_from_slice(b"hello\0");
        let device_body = cat(&[b"DEV0", &other, &text, &name_adr(7)]);
        let device = cat(&[&[0x5B, 0x82], &pkg(&device_body)]);
        let devices = discover(&device).unwrap();
        let d = devices.iter().next().unwrap();
        assert_eq!(d.adr, Some(7));
        assert_eq!(d.hid, None);
    }

    #[test]
    fn bad_name_segment_is_refused() {
        // A NameSeg must start with a letter or underscore.
        let device = cat(&[&[0x5B, 0x82], &pkg(b"1ABC")]);
        assert!(matches!(discover(&device), Err(AmlError::BadName { .. })));
    }

    #[test]
    fn too_many_devices_reports_full() {
        let mut body = Vec::new();
        for i in 0..=MAX_DEVICES {
            let name = [b'D', b'E', b'V', b'A' + (i % 26) as u8];
            body.extend_from_slice(&[0x5B, 0x82]);
            body.extend_from_slice(&pkg(&name));
        }
        assert_eq!(discover(&body).unwrap_err(), AmlError::Full);
    }

    #[test]
    fn nesting_beyond_the_depth_limit_is_refused() {
        // Each device nests one level deeper than the last.
        let mut inner: Vec<u8> = Vec::new();
        for _ in 0..=MAX_DEPTH {
            let body = cat(&[b"NEST", &inner]);
            inner = cat(&[&[0x5B, 0x82], &pkg(&body)]);
        }
        assert_eq!(discover(&inner).unwrap_err(), AmlError::TooDeep);
    }

    #[test]
    fn parent_prefix_climbs_out_of_the_scope() {
        // Inside \_SB_.PCI0, Scope(^BUS0) resolves to the sibling \_SB_.BUS0.
        let device = cat(&[&[0x5B, 0x82], &pkg(&cat(&[b"USB0", &name_adr(1)]))]);
        let inner_scope = cat(&[&[0x10], &pkg(&cat(&[b"^BUS0", &device]))]);
        let pci = cat(&[&[0x5B, 0x82], &pkg(&cat(&[b"PCI0", &inner_scope]))]);
        let aml = cat(&[&[0x10], &pkg(&cat(&[SB, &pci]))]);
        let devices = discover(&aml).unwrap();
        let paths: Vec<std::string::String> = devices.iter().map(|d| d.path.to_string()).collect();
        assert_eq!(paths, ["\\_SB_.PCI0", "\\_SB_.BUS0.USB0"]);
    }

    #[test]
    fn random_bodies_never_panic() {
        // Deterministic xorshift64 input: random bodies and mutations of a
        // valid table. Every result must be Ok or an AmlError.
        let mut state: u64 = 0x9E37_79B9_7F4A_7C15;
        let mut next = move || {
            state ^= state << 13;
            state ^= state >> 7;
            state ^= state << 17;
            state
        };
        let valid = pci_tree();
        for i in 0..1500usize {
            let body: Vec<u8> = if i % 2 == 0 {
                let len = (next() % 256) as usize;
                (0..len).map(|_| next() as u8).collect()
            } else {
                let mut v = valid.clone();
                let flips = 1 + (next() % 4) as usize;
                for _ in 0..flips {
                    let at = (next() as usize) % v.len();
                    v[at] = next() as u8;
                }
                let cut = (next() as usize) % (v.len() + 1);
                v.truncate(cut);
                v
            };
            let _ = discover(&body);
        }
        // The unmodified table is still found.
        assert_eq!(discover(&valid).unwrap().count(), 1);
    }
}
