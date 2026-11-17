// SPDX-License-Identifier: MIT OR Apache-2.0
//! tpt-k HAL ACPI parser: RSDP, the system description table list, MADT and MCFG.
//!
//! The parser works on byte slices. The caller maps physical addresses to
//! slices (the kernel does this through its memory window), so this crate
//! needs no allocator and no privileged access. Every table is checked for its
//! signature, its length and its checksum before any field is read.
//!
//! The DSDT and SSDT hold AML bytecode. Parsing AML is not done here yet.

#![no_std]

#[cfg(test)]
extern crate std;

#[cfg(test)]
mod fuzz_tests;

/// Length of the common header that starts every system description table.
pub const SDT_HEADER_LEN: usize = 36;

/// Why a table could not be parsed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AcpiError {
    /// The input ends before the structure does.
    TooShort,
    /// The signature is not the one this structure requires.
    BadSignature,
    /// The bytes do not sum to zero modulo 256.
    BadChecksum,
    /// A length field is too small or runs past the input.
    BadLength,
}

/// True when the bytes sum to zero modulo 256, as ACPI checksums require.
#[must_use]
pub fn checksum_ok(bytes: &[u8]) -> bool {
    bytes.iter().fold(0u8, |acc, b| acc.wrapping_add(*b)) == 0
}

fn get<const N: usize>(bytes: &[u8], offset: usize) -> Result<[u8; N], AcpiError> {
    bytes
        .get(offset..offset + N)
        .and_then(|s| s.try_into().ok())
        .ok_or(AcpiError::TooShort)
}

fn u8_at(bytes: &[u8], offset: usize) -> Result<u8, AcpiError> {
    Ok(get::<1>(bytes, offset)?[0])
}

fn u16_at(bytes: &[u8], offset: usize) -> Result<u16, AcpiError> {
    Ok(u16::from_le_bytes(get(bytes, offset)?))
}

fn u32_at(bytes: &[u8], offset: usize) -> Result<u32, AcpiError> {
    Ok(u32::from_le_bytes(get(bytes, offset)?))
}

fn u64_at(bytes: &[u8], offset: usize) -> Result<u64, AcpiError> {
    Ok(u64::from_le_bytes(get(bytes, offset)?))
}

/// The root system description pointer, found by the firmware in the EBDA or
/// the BIOS area and handed to the loader.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Rsdp {
    /// ACPI revision: 0 for ACPI 1.0, 2 or later for the extended table.
    pub revision: u8,
    /// Physical address of the RSDT.
    pub rsdt_address: u32,
    /// Physical address of the XSDT, present from revision 2 when non-zero.
    pub xsdt_address: Option<u64>,
}

impl Rsdp {
    /// Parses and checks the RSDP. `bytes` may be longer than the structure.
    pub fn parse(bytes: &[u8]) -> Result<Self, AcpiError> {
        if bytes.len() < 20 {
            return Err(AcpiError::TooShort);
        }
        if &bytes[0..8] != b"RSD PTR " {
            return Err(AcpiError::BadSignature);
        }
        if !checksum_ok(&bytes[..20]) {
            return Err(AcpiError::BadChecksum);
        }
        let revision = u8_at(bytes, 15)?;
        let rsdt_address = u32_at(bytes, 16)?;
        if revision < 2 {
            return Ok(Self {
                revision,
                rsdt_address,
                xsdt_address: None,
            });
        }
        let length = u32_at(bytes, 20)? as usize;
        if length < 36 || length > bytes.len() {
            return Err(AcpiError::BadLength);
        }
        if !checksum_ok(&bytes[..length]) {
            return Err(AcpiError::BadChecksum);
        }
        let xsdt = u64_at(bytes, 24)?;
        Ok(Self {
            revision,
            rsdt_address,
            xsdt_address: (xsdt != 0).then_some(xsdt),
        })
    }
}

/// A system description table whose length and checksum have been checked.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Sdt<'a> {
    bytes: &'a [u8],
}

impl<'a> Sdt<'a> {
    /// Checks the length field and the checksum. `bytes` may be longer than
    /// the table; only the first `length` bytes are kept.
    pub fn parse(bytes: &'a [u8]) -> Result<Self, AcpiError> {
        if bytes.len() < SDT_HEADER_LEN {
            return Err(AcpiError::TooShort);
        }
        let length = u32_at(bytes, 4)? as usize;
        if length < SDT_HEADER_LEN || length > bytes.len() {
            return Err(AcpiError::BadLength);
        }
        let bytes = &bytes[..length];
        if !checksum_ok(bytes) {
            return Err(AcpiError::BadChecksum);
        }
        Ok(Self { bytes })
    }

    /// The four-byte signature, such as `APIC` or `MCFG`.
    #[must_use]
    pub fn signature(&self) -> [u8; 4] {
        [self.bytes[0], self.bytes[1], self.bytes[2], self.bytes[3]]
    }

    /// The revision field of the header.
    #[must_use]
    pub fn revision(&self) -> u8 {
        self.bytes[8]
    }

    /// The whole table, header included.
    #[must_use]
    pub fn bytes(&self) -> &'a [u8] {
        self.bytes
    }

    /// The table body, after the 36-byte header.
    #[must_use]
    pub fn body(&self) -> &'a [u8] {
        &self.bytes[SDT_HEADER_LEN..]
    }
}

/// Iterator over the physical addresses in the RSDT or XSDT.
#[derive(Clone, Debug)]
pub struct TableAddresses<'a> {
    body: &'a [u8],
    width: usize,
    index: usize,
}

impl Iterator for TableAddresses<'_> {
    type Item = u64;

    fn next(&mut self) -> Option<u64> {
        let start = self.index.checked_mul(self.width)?;
        let end = start.checked_add(self.width)?;
        let raw = self.body.get(start..end)?;
        self.index += 1;
        Some(if self.width == 8 {
            u64::from_le_bytes(raw.try_into().ok()?)
        } else {
            u32::from_le_bytes(raw.try_into().ok()?) as u64
        })
    }
}

/// Lists the tables a root table points to. An `XSDT` holds 64-bit addresses,
/// an `RSDT` 32-bit ones.
pub fn table_addresses<'a>(root: &Sdt<'a>) -> Result<TableAddresses<'a>, AcpiError> {
    let width = match &root.signature() {
        b"XSDT" => 8,
        b"RSDT" => 4,
        _ => return Err(AcpiError::BadSignature),
    };
    Ok(TableAddresses {
        body: root.body(),
        width,
        index: 0,
    })
}

/// A parsed Multiple APIC Description Table.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Madt<'a> {
    sdt: Sdt<'a>,
}

impl<'a> Madt<'a> {
    /// Checks that `bytes` is a MADT (`APIC`) and returns a view of it.
    pub fn parse(bytes: &'a [u8]) -> Result<Self, AcpiError> {
        let sdt = Sdt::parse(bytes)?;
        if &sdt.signature() != b"APIC" {
            return Err(AcpiError::BadSignature);
        }
        if sdt.body().len() < 8 {
            return Err(AcpiError::TooShort);
        }
        Ok(Self { sdt })
    }

    /// The 32-bit local APIC address. A type 5 entry may override it.
    #[must_use]
    pub fn local_apic_address(&self) -> u32 {
        u32::from_le_bytes([
            self.sdt.body()[0],
            self.sdt.body()[1],
            self.sdt.body()[2],
            self.sdt.body()[3],
        ])
    }

    /// The MADT flags. Bit 0 says the system also has a legacy 8259 PIC.
    #[must_use]
    pub fn flags(&self) -> u32 {
        u32::from_le_bytes([
            self.sdt.body()[4],
            self.sdt.body()[5],
            self.sdt.body()[6],
            self.sdt.body()[7],
        ])
    }

    /// The interrupt controller structures after the fixed fields.
    #[must_use]
    pub fn entries(&self) -> MadtEntries<'a> {
        MadtEntries {
            rest: &self.sdt.body()[8..],
            done: false,
        }
    }
}

/// One interrupt controller structure from the MADT.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MadtEntry {
    /// Type 0: a processor's local APIC.
    LocalApic {
        /// ACPI processor UID.
        uid: u8,
        /// Local APIC ID.
        apic_id: u8,
        /// Bit 0: the processor is enabled. Bit 1: it can be brought online.
        flags: u32,
    },
    /// Type 1: an I/O APIC.
    IoApic {
        /// I/O APIC ID.
        id: u8,
        /// Physical address of its registers.
        address: u32,
        /// First global system interrupt it handles.
        gsi_base: u32,
    },
    /// Type 2: a legacy IRQ that is wired to a different global interrupt.
    InterruptOverride {
        /// Bus the source is on (0 means ISA).
        bus: u8,
        /// IRQ source on that bus.
        source: u8,
        /// Global system interrupt it is routed to.
        gsi: u32,
        /// MPS INTI flags: polarity in bits 0-1, trigger mode in bits 2-3.
        flags: u16,
    },
    /// Type 4: the local APIC's NMI input.
    LocalApicNmi {
        /// ACPI processor UID, or 0xFF for all processors.
        uid: u8,
        /// MPS INTI flags.
        flags: u16,
        /// Local APIC LINT pin (0 or 1).
        lint: u8,
    },
    /// Type 5: a 64-bit local APIC address that replaces the 32-bit one.
    LocalApicAddressOverride {
        /// Physical address of the local APIC registers.
        address: u64,
    },
    /// Type 9: a local x2APIC.
    LocalX2Apic {
        /// x2APIC ID, 32 bits wide.
        x2apic_id: u32,
        /// Bit 0: enabled. Bit 1: can be brought online.
        flags: u32,
        /// ACPI processor UID.
        uid: u32,
    },
    /// Any other structure type. Its length is still honoured.
    Unknown {
        /// The structure type byte.
        kind: u8,
    },
}

/// Iterator over the MADT interrupt controller structures.
///
/// A structure whose length is below 2, or runs past the table, yields one
/// `Err(BadLength)` and then ends, since the rest cannot be located.
#[derive(Clone, Debug)]
pub struct MadtEntries<'a> {
    rest: &'a [u8],
    done: bool,
}

impl Iterator for MadtEntries<'_> {
    type Item = Result<MadtEntry, AcpiError>;

    fn next(&mut self) -> Option<Self::Item> {
        if self.done || self.rest.is_empty() {
            return None;
        }
        let parsed = parse_madt_entry(self.rest);
        match parsed {
            Ok((entry, len)) => {
                self.rest = &self.rest[len..];
                Some(Ok(entry))
            }
            Err(e) => {
                self.done = true;
                Some(Err(e))
            }
        }
    }
}

fn parse_madt_entry(rest: &[u8]) -> Result<(MadtEntry, usize), AcpiError> {
    let kind = u8_at(rest, 0)?;
    let len = u8_at(rest, 1)? as usize;
    if len < 2 || len > rest.len() {
        return Err(AcpiError::BadLength);
    }
    let body = &rest[..len];
    let need = |n: usize| {
        if len < n {
            Err(AcpiError::BadLength)
        } else {
            Ok(())
        }
    };
    let entry = match kind {
        0 => {
            need(8)?;
            MadtEntry::LocalApic {
                uid: u8_at(body, 2)?,
                apic_id: u8_at(body, 3)?,
                flags: u32_at(body, 4)?,
            }
        }
        1 => {
            need(12)?;
            MadtEntry::IoApic {
                id: u8_at(body, 2)?,
                address: u32_at(body, 4)?,
                gsi_base: u32_at(body, 8)?,
            }
        }
        2 => {
            need(10)?;
            MadtEntry::InterruptOverride {
                bus: u8_at(body, 2)?,
                source: u8_at(body, 3)?,
                gsi: u32_at(body, 4)?,
                flags: u16_at(body, 8)?,
            }
        }
        4 => {
            need(6)?;
            MadtEntry::LocalApicNmi {
                uid: u8_at(body, 2)?,
                flags: u16_at(body, 3)?,
                lint: u8_at(body, 5)?,
            }
        }
        5 => {
            need(12)?;
            MadtEntry::LocalApicAddressOverride {
                address: u64_at(body, 4)?,
            }
        }
        9 => {
            need(16)?;
            MadtEntry::LocalX2Apic {
                x2apic_id: u32_at(body, 4)?,
                flags: u32_at(body, 8)?,
                uid: u32_at(body, 12)?,
            }
        }
        _ => MadtEntry::Unknown { kind },
    };
    Ok((entry, len))
}

/// One enhanced-configuration (ECAM) window from the MCFG table.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct McfgEntry {
    /// Physical base address of the window for `start_bus`.
    pub base: u64,
    /// PCI segment group number.
    pub segment: u16,
    /// First bus the window covers.
    pub start_bus: u8,
    /// Last bus the window covers.
    pub end_bus: u8,
}

/// A parsed PCI Express memory-mapped configuration table.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Mcfg<'a> {
    sdt: Sdt<'a>,
}

impl<'a> Mcfg<'a> {
    /// Checks that `bytes` is an MCFG table and returns a view of it.
    pub fn parse(bytes: &'a [u8]) -> Result<Self, AcpiError> {
        let sdt = Sdt::parse(bytes)?;
        if &sdt.signature() != b"MCFG" {
            return Err(AcpiError::BadSignature);
        }
        if sdt.body().len() < 8 {
            return Err(AcpiError::TooShort);
        }
        Ok(Self { sdt })
    }

    /// The ECAM windows. A table whose entries are not a whole number of
    /// 16-byte records is refused by the iterator's last item.
    #[must_use]
    pub fn entries(&self) -> McfgEntries<'a> {
        McfgEntries {
            rest: &self.sdt.body()[8..],
        }
    }
}

/// Iterator over the MCFG windows.
#[derive(Clone, Debug)]
pub struct McfgEntries<'a> {
    rest: &'a [u8],
}

impl Iterator for McfgEntries<'_> {
    type Item = Result<McfgEntry, AcpiError>;

    fn next(&mut self) -> Option<Self::Item> {
        if self.rest.is_empty() {
            return None;
        }
        if self.rest.len() < 16 {
            self.rest = &[];
            return Some(Err(AcpiError::BadLength));
        }
        let r = self.rest;
        let entry = McfgEntry {
            base: match u64_at(r, 0) {
                Ok(v) => v,
                Err(e) => return Some(Err(e)),
            },
            segment: match u16_at(r, 8) {
                Ok(v) => v,
                Err(e) => return Some(Err(e)),
            },
            start_bus: r[10],
            end_bus: r[11],
        };
        self.rest = &r[16..];
        Some(Ok(entry))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::vec::Vec;

    /// Builds a table with `signature`, the given body, and a valid checksum.
    fn table(signature: &[u8; 4], body: &[u8]) -> Vec<u8> {
        let mut t = Vec::new();
        t.extend_from_slice(signature);
        let len = (SDT_HEADER_LEN + body.len()) as u32;
        t.extend_from_slice(&len.to_le_bytes());
        t.push(1); // revision
        t.push(0); // checksum, fixed below
        t.extend_from_slice(b"TPTK  ");
        t.extend_from_slice(b"TESTTBL ");
        t.extend_from_slice(&1u32.to_le_bytes());
        t.extend_from_slice(b"TPT ");
        t.extend_from_slice(&1u32.to_le_bytes());
        assert_eq!(t.len(), SDT_HEADER_LEN);
        t.extend_from_slice(body);
        fix_checksum(&mut t, 9);
        t
    }

    /// Sets the byte at `index` so the whole buffer sums to zero.
    fn fix_checksum(t: &mut [u8], index: usize) {
        t[index] = 0;
        let sum = t.iter().fold(0u8, |a, b| a.wrapping_add(*b));
        t[index] = 0u8.wrapping_sub(sum);
    }

    fn rsdp_v2(xsdt: u64) -> Vec<u8> {
        let mut r = Vec::new();
        r.extend_from_slice(b"RSD PTR ");
        r.push(0); // checksum, fixed below
        r.extend_from_slice(b"TPTK  ");
        r.push(2);
        r.extend_from_slice(&0x1000u32.to_le_bytes());
        r.extend_from_slice(&36u32.to_le_bytes());
        r.extend_from_slice(&xsdt.to_le_bytes());
        r.push(0); // extended checksum
        r.extend_from_slice(&[0; 3]);
        let v1: u8 = r[..20].iter().fold(0u8, |a, b| a.wrapping_add(*b));
        r[8] = 0u8.wrapping_sub(v1);
        let all: u8 = r.iter().fold(0u8, |a, b| a.wrapping_add(*b));
        r[32] = 0u8.wrapping_sub(all);
        r
    }

    fn madt_body(entries: &[u8]) -> Vec<u8> {
        let mut b = Vec::new();
        b.extend_from_slice(&0xFEE0_0000u32.to_le_bytes());
        b.extend_from_slice(&1u32.to_le_bytes());
        b.extend_from_slice(entries);
        b
    }

    #[test]
    fn rsdp_v2_yields_xsdt() {
        let r = rsdp_v2(0x1234_5000);
        let parsed = Rsdp::parse(&r).unwrap();
        assert_eq!(parsed.revision, 2);
        assert_eq!(parsed.rsdt_address, 0x1000);
        assert_eq!(parsed.xsdt_address, Some(0x1234_5000));
    }

    #[test]
    fn rsdp_rejects_bad_checksum_and_signature() {
        let mut r = rsdp_v2(0x1000);
        r[10] ^= 1;
        assert_eq!(Rsdp::parse(&r), Err(AcpiError::BadChecksum));
        let mut r = rsdp_v2(0x1000);
        r[0] = b'X';
        assert_eq!(Rsdp::parse(&r), Err(AcpiError::BadSignature));
        assert_eq!(Rsdp::parse(&r[..10]), Err(AcpiError::TooShort));
    }

    #[test]
    fn sdt_checks_length_and_checksum() {
        let t = table(b"TEST", &[1, 2, 3]);
        let s = Sdt::parse(&t).unwrap();
        assert_eq!(s.signature(), *b"TEST");
        assert_eq!(s.body(), &[1, 2, 3]);
        let mut bad = t.clone();
        bad[38] ^= 0xFF;
        assert_eq!(Sdt::parse(&bad), Err(AcpiError::BadChecksum));
        assert_eq!(Sdt::parse(&t[..30]), Err(AcpiError::TooShort));
        let mut long = t.clone();
        long[4] = 0xFF;
        assert_eq!(Sdt::parse(&long), Err(AcpiError::BadLength));
    }

    #[test]
    fn xsdt_lists_64_bit_addresses() {
        let mut body = Vec::new();
        body.extend_from_slice(&0x1_0000_0000u64.to_le_bytes());
        body.extend_from_slice(&0xFEE0_1000u64.to_le_bytes());
        let t = table(b"XSDT", &body);
        let root = Sdt::parse(&t).unwrap();
        let addrs: Vec<u64> = table_addresses(&root).unwrap().collect();
        assert_eq!(addrs, [0x1_0000_0000, 0xFEE0_1000]);
    }

    #[test]
    fn rsdt_lists_32_bit_addresses() {
        let mut body = Vec::new();
        body.extend_from_slice(&0x7000u32.to_le_bytes());
        body.extend_from_slice(&0x8000u32.to_le_bytes());
        let t = table(b"RSDT", &body);
        let root = Sdt::parse(&t).unwrap();
        let addrs: Vec<u64> = table_addresses(&root).unwrap().collect();
        assert_eq!(addrs, [0x7000, 0x8000]);
    }

    #[test]
    fn madt_entries_decode() {
        let mut e = Vec::new();
        // Local APIC: uid 0, id 0, enabled.
        e.extend_from_slice(&[0, 8, 0, 0, 1, 0, 0, 0]);
        // I/O APIC: id 2, address 0xFEC00000, GSI base 0.
        e.extend_from_slice(&[1, 12, 2, 0]);
        e.extend_from_slice(&0xFEC0_0000u32.to_le_bytes());
        e.extend_from_slice(&0u32.to_le_bytes());
        // Override: ISA IRQ 0 -> GSI 2, active high, edge.
        e.extend_from_slice(&[2, 10, 0, 0]);
        e.extend_from_slice(&2u32.to_le_bytes());
        e.extend_from_slice(&0u16.to_le_bytes());
        // Local APIC NMI: all processors, LINT1.
        e.extend_from_slice(&[4, 6, 0xFF, 0, 0, 1]);
        // Unknown type 0x7F with length 4.
        e.extend_from_slice(&[0x7F, 4, 0, 0]);
        let t = table(b"APIC", &madt_body(&e));
        let madt = Madt::parse(&t).unwrap();
        assert_eq!(madt.local_apic_address(), 0xFEE0_0000);
        assert_eq!(madt.flags(), 1);
        let got: Vec<MadtEntry> = madt.entries().map(Result::unwrap).collect();
        assert_eq!(
            got,
            [
                MadtEntry::LocalApic {
                    uid: 0,
                    apic_id: 0,
                    flags: 1
                },
                MadtEntry::IoApic {
                    id: 2,
                    address: 0xFEC0_0000,
                    gsi_base: 0
                },
                MadtEntry::InterruptOverride {
                    bus: 0,
                    source: 0,
                    gsi: 2,
                    flags: 0
                },
                MadtEntry::LocalApicNmi {
                    uid: 0xFF,
                    flags: 0,
                    lint: 1
                },
                MadtEntry::Unknown { kind: 0x7F },
            ]
        );
    }

    #[test]
    fn madt_x2apic_and_override_decode() {
        let mut e = Vec::new();
        e.extend_from_slice(&[9, 16, 0, 0]);
        e.extend_from_slice(&0x20u32.to_le_bytes());
        e.extend_from_slice(&1u32.to_le_bytes());
        e.extend_from_slice(&3u32.to_le_bytes());
        e.extend_from_slice(&[5, 12, 0, 0]);
        e.extend_from_slice(&0x1_0000_0000u64.to_le_bytes());
        let t = table(b"APIC", &madt_body(&e));
        let got: Vec<MadtEntry> = Madt::parse(&t)
            .unwrap()
            .entries()
            .map(Result::unwrap)
            .collect();
        assert_eq!(
            got,
            [
                MadtEntry::LocalX2Apic {
                    x2apic_id: 0x20,
                    flags: 1,
                    uid: 3
                },
                MadtEntry::LocalApicAddressOverride {
                    address: 0x1_0000_0000
                },
            ]
        );
    }

    #[test]
    fn madt_stops_on_bad_length() {
        // Length 1 is below the 2-byte minimum, so nothing after it can be found.
        let t = table(b"APIC", &madt_body(&[0, 1, 0, 0, 0, 0, 0, 0]));
        let mut it = Madt::parse(&t).unwrap().entries();
        assert_eq!(it.next(), Some(Err(AcpiError::BadLength)));
        assert_eq!(it.next(), None);
    }

    #[test]
    fn madt_rejects_wrong_signature_and_short_known_entry() {
        let t = table(b"MCFG", &[0; 8]);
        assert_eq!(Madt::parse(&t), Err(AcpiError::BadSignature));
        // A local APIC entry (type 0) that claims only 4 bytes.
        let t = table(b"APIC", &madt_body(&[0, 4, 0, 0]));
        let mut it = Madt::parse(&t).unwrap().entries();
        assert_eq!(it.next(), Some(Err(AcpiError::BadLength)));
    }

    #[test]
    fn mcfg_entries_decode() {
        let mut body = Vec::new();
        body.extend_from_slice(&[0; 8]);
        body.extend_from_slice(&0xE000_0000u64.to_le_bytes());
        body.extend_from_slice(&0u16.to_le_bytes());
        body.push(0);
        body.push(0xFF);
        body.extend_from_slice(&[0; 4]);
        let t = table(b"MCFG", &body);
        let mcfg = Mcfg::parse(&t).unwrap();
        let got: Vec<McfgEntry> = mcfg.entries().map(Result::unwrap).collect();
        assert_eq!(
            got,
            [McfgEntry {
                base: 0xE000_0000,
                segment: 0,
                start_bus: 0,
                end_bus: 0xFF
            }]
        );
    }

    #[test]
    fn mcfg_partial_record_is_an_error() {
        let mut body = Vec::new();
        body.extend_from_slice(&[0; 8]);
        body.extend_from_slice(&[0; 10]);
        let t = table(b"MCFG", &body);
        let mut it = Mcfg::parse(&t).unwrap().entries();
        assert_eq!(it.next(), Some(Err(AcpiError::BadLength)));
        assert_eq!(it.next(), None);
    }
}
