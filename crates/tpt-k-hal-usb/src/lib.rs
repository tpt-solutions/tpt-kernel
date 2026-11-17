// SPDX-License-Identifier: MIT OR Apache-2.0
//! tpt-k HAL USB: descriptor parser for the standard device, configuration,
//! interface and endpoint descriptors (USB 2.0 chapter 9).
//!
//! The parser reads descriptors from bytes the caller has already fetched with
//! a control transfer. It does not talk to a host controller. Unknown
//! descriptor types, such as HID class descriptors, are returned unparsed so
//! that a class driver can read them.

#![no_std]

#[cfg(test)]
extern crate std;

#[cfg(test)]
mod mutation_tests;

/// Descriptor type codes.
pub mod kind {
    /// Device descriptor.
    pub const DEVICE: u8 = 0x01;
    /// Configuration descriptor.
    pub const CONFIGURATION: u8 = 0x02;
    /// Interface descriptor.
    pub const INTERFACE: u8 = 0x04;
    /// Endpoint descriptor.
    pub const ENDPOINT: u8 = 0x05;
}

/// Why a descriptor could not be parsed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum UsbError {
    /// The input ends inside a descriptor.
    TooShort,
    /// A descriptor's length byte is below 2 or runs past the input.
    BadLength,
    /// The descriptor type is not the one this parser was asked for.
    WrongType,
    /// The configuration's `wTotalLength` disagrees with the bytes given.
    BadTotalLength,
}

fn u8_at(b: &[u8], off: usize) -> Result<u8, UsbError> {
    b.get(off).copied().ok_or(UsbError::TooShort)
}

fn u16_at(b: &[u8], off: usize) -> Result<u16, UsbError> {
    let raw = b.get(off..off + 2).ok_or(UsbError::TooShort)?;
    Ok(u16::from_le_bytes([raw[0], raw[1]]))
}

/// Splits one descriptor off the front of `bytes`: `(type, whole descriptor)`.
fn split_one(bytes: &[u8]) -> Result<(u8, &[u8]), UsbError> {
    let len = u8_at(bytes, 0)? as usize;
    let kind = u8_at(bytes, 1)?;
    if len < 2 || len > bytes.len() {
        return Err(UsbError::BadLength);
    }
    Ok((kind, &bytes[..len]))
}

/// A device descriptor, which is 18 bytes long.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Device {
    /// USB specification release, in binary-coded decimal (0x0200 for 2.0).
    pub usb_version: u16,
    /// Class code, or 0 when the class is per interface.
    pub class: u8,
    /// Subclass code.
    pub subclass: u8,
    /// Protocol code.
    pub protocol: u8,
    /// Maximum packet size for endpoint 0, in bytes.
    pub max_packet_size0: u8,
    /// Vendor ID.
    pub vendor_id: u16,
    /// Product ID.
    pub product_id: u16,
    /// Device release number, in binary-coded decimal.
    pub device_version: u16,
    /// Number of configurations.
    pub num_configurations: u8,
}

impl Device {
    /// Parses the first device descriptor in `bytes`.
    pub fn parse(bytes: &[u8]) -> Result<Self, UsbError> {
        let (k, d) = split_one(bytes)?;
        if k != kind::DEVICE {
            return Err(UsbError::WrongType);
        }
        if d.len() < 18 {
            return Err(UsbError::BadLength);
        }
        Ok(Self {
            usb_version: u16_at(d, 2)?,
            class: d[4],
            subclass: d[5],
            protocol: d[6],
            max_packet_size0: d[7],
            vendor_id: u16_at(d, 8)?,
            product_id: u16_at(d, 10)?,
            device_version: u16_at(d, 12)?,
            num_configurations: d[17],
        })
    }
}

/// A configuration: its header and every descriptor that belongs to it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Configuration<'a> {
    /// Value to pass to SET_CONFIGURATION.
    pub value: u8,
    /// Bit 6 set means self-powered; bit 5 set means remote wakeup.
    pub attributes: u8,
    /// Maximum power draw, in units of 2 mA.
    pub max_power: u8,
    /// Number of interfaces the configuration declares.
    pub num_interfaces: u8,
    /// The descriptors after the configuration header, up to `wTotalLength`.
    body: &'a [u8],
}

impl<'a> Configuration<'a> {
    /// Parses a configuration block: the 9-byte header followed by its
    /// interface and endpoint descriptors. `bytes` must hold at least
    /// `wTotalLength` bytes, and the block is cut to that length.
    pub fn parse(bytes: &'a [u8]) -> Result<Self, UsbError> {
        let (k, head) = split_one(bytes)?;
        if k != kind::CONFIGURATION {
            return Err(UsbError::WrongType);
        }
        if head.len() < 9 {
            return Err(UsbError::BadLength);
        }
        let total = u16_at(head, 2)? as usize;
        if total < 9 || total > bytes.len() {
            return Err(UsbError::BadTotalLength);
        }
        Ok(Self {
            value: head[5],
            attributes: head[7],
            max_power: head[8],
            num_interfaces: head[4],
            body: &bytes[9..total],
        })
    }

    /// The descriptors that follow the configuration header.
    #[must_use]
    pub fn descriptors(&self) -> Descriptors<'a> {
        Descriptors { rest: self.body }
    }
}

/// A parsed interface descriptor.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Interface {
    /// Interface number.
    pub number: u8,
    /// Alternate setting for `number`.
    pub alternate: u8,
    /// Number of endpoints the interface uses, not counting endpoint 0.
    pub num_endpoints: u8,
    /// Class code.
    pub class: u8,
    /// Subclass code.
    pub subclass: u8,
    /// Protocol code.
    pub protocol: u8,
}

impl Interface {
    /// Parses a 9-byte interface descriptor.
    pub fn parse(bytes: &[u8]) -> Result<Self, UsbError> {
        let (k, d) = split_one(bytes)?;
        if k != kind::INTERFACE {
            return Err(UsbError::WrongType);
        }
        if d.len() < 9 {
            return Err(UsbError::BadLength);
        }
        Ok(Self {
            number: d[2],
            alternate: d[3],
            num_endpoints: d[4],
            class: d[5],
            subclass: d[6],
            protocol: d[7],
        })
    }

    /// True for the HID boot keyboard protocol (class 3, subclass 1, protocol 1).
    #[must_use]
    pub fn is_hid_boot_keyboard(&self) -> bool {
        self.class == 3 && self.subclass == 1 && self.protocol == 1
    }

    /// True for the HID boot mouse protocol (class 3, subclass 1, protocol 2).
    #[must_use]
    pub fn is_hid_boot_mouse(&self) -> bool {
        self.class == 3 && self.subclass == 1 && self.protocol == 2
    }
}

/// Transfer type of an endpoint.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TransferType {
    /// Control transfers (endpoint 0 only, in practice).
    Control,
    /// Isochronous transfers.
    Isochronous,
    /// Bulk transfers.
    Bulk,
    /// Interrupt transfers.
    Interrupt,
}

/// A parsed endpoint descriptor.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Endpoint {
    /// Endpoint number, 0 to 15.
    pub number: u8,
    /// True for an IN endpoint (device to host).
    pub is_in: bool,
    /// Transfer type.
    pub transfer: TransferType,
    /// Maximum packet size in bytes, bits 0-10 of `wMaxPacketSize`.
    pub max_packet_size: u16,
    /// Additional transactions per microframe, from bits 11-12 of
    /// `wMaxPacketSize` (high-speed isochronous and interrupt endpoints).
    pub transactions_per_microframe: u8,
    /// Polling interval. For interrupt endpoints it is in frames (1 ms each at
    /// full speed); for high-speed interrupt endpoints it is `2^(n-1)` microframes.
    pub interval: u8,
}

impl Endpoint {
    /// Parses a 7-byte endpoint descriptor.
    pub fn parse(bytes: &[u8]) -> Result<Self, UsbError> {
        let (k, d) = split_one(bytes)?;
        if k != kind::ENDPOINT {
            return Err(UsbError::WrongType);
        }
        if d.len() < 7 {
            return Err(UsbError::BadLength);
        }
        let attributes = d[3];
        let transfer = match attributes & 0x3 {
            0 => TransferType::Control,
            1 => TransferType::Isochronous,
            2 => TransferType::Bulk,
            _ => TransferType::Interrupt,
        };
        let packet = u16_at(d, 4)?;
        Ok(Self {
            number: d[2] & 0x0F,
            is_in: d[2] & 0x80 != 0,
            transfer,
            max_packet_size: packet & 0x07FF,
            transactions_per_microframe: ((packet >> 11) & 0x3) as u8 + 1,
            interval: d[6],
        })
    }
}

/// One descriptor inside a configuration, typed where this crate knows it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Descriptor<'a> {
    /// An interface descriptor.
    Interface(Interface),
    /// An endpoint descriptor.
    Endpoint(Endpoint),
    /// Any other type, such as a class-specific descriptor. The bytes include
    /// the length and type header.
    Other {
        /// The descriptor type.
        kind: u8,
        /// The whole descriptor.
        bytes: &'a [u8],
    },
}

/// Iterator over the descriptors inside a configuration.
#[derive(Clone, Debug)]
pub struct Descriptors<'a> {
    rest: &'a [u8],
}

impl<'a> Iterator for Descriptors<'a> {
    type Item = Result<Descriptor<'a>, UsbError>;

    fn next(&mut self) -> Option<Self::Item> {
        if self.rest.is_empty() {
            return None;
        }
        let (k, whole) = match split_one(self.rest) {
            Ok(v) => v,
            Err(e) => {
                self.rest = &[];
                return Some(Err(e));
            }
        };
        self.rest = &self.rest[whole.len()..];
        let item = match k {
            kind::INTERFACE => Interface::parse(whole).map(Descriptor::Interface),
            kind::ENDPOINT => Endpoint::parse(whole).map(Descriptor::Endpoint),
            _ => Ok(Descriptor::Other {
                kind: k,
                bytes: whole,
            }),
        };
        Some(item)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::vec::Vec;

    const DEVICE_BYTES: [u8; 18] = [
        18, 1, 0x00, 0x02, 0, 0, 0, 64, 0x6B, 0x1D, 0x02, 0x00, 0x00, 0x06, 1, 2, 3, 1,
    ];

    fn configuration(interfaces: &[u8]) -> Vec<u8> {
        let total = 9 + interfaces.len();
        let mut c = Vec::new();
        c.extend_from_slice(&[9, 2, total as u8, 0, 1, 1, 0, 0xA0, 50]);
        c.extend_from_slice(interfaces);
        c
    }

    #[test]
    fn device_descriptor_decodes() {
        let d = Device::parse(&DEVICE_BYTES).unwrap();
        assert_eq!(d.usb_version, 0x0200);
        assert_eq!(d.max_packet_size0, 64);
        assert_eq!((d.vendor_id, d.product_id), (0x1D6B, 0x0002));
        assert_eq!(d.device_version, 0x0600);
        assert_eq!(d.num_configurations, 1);
        assert_eq!(Device::parse(&DEVICE_BYTES[..10]), Err(UsbError::BadLength));
        let mut wrong = DEVICE_BYTES;
        wrong[1] = kind::CONFIGURATION;
        assert_eq!(Device::parse(&wrong), Err(UsbError::WrongType));
    }

    #[test]
    fn keyboard_configuration_yields_interface_endpoint_and_hid() {
        let mut ifc = Vec::new();
        // Boot keyboard interface with one interrupt IN endpoint.
        ifc.extend_from_slice(&[9, 4, 0, 0, 1, 3, 1, 1, 0]);
        // HID class descriptor: unparsed.
        ifc.extend_from_slice(&[9, 0x21, 0x11, 0x01, 0, 1, 0x22, 63, 0]);
        // Interrupt IN endpoint 1, 8 bytes, 10 ms.
        ifc.extend_from_slice(&[7, 5, 0x81, 0x03, 8, 0, 10]);
        let cfg = configuration(&ifc);
        let c = Configuration::parse(&cfg).unwrap();
        assert_eq!((c.value, c.num_interfaces, c.max_power), (1, 1, 50));
        assert_eq!(c.attributes, 0xA0);

        let items: Vec<Descriptor> = c.descriptors().map(Result::unwrap).collect();
        assert_eq!(items.len(), 3);
        let Descriptor::Interface(i) = items[0] else {
            panic!("expected interface");
        };
        assert!(i.is_hid_boot_keyboard());
        assert!(!i.is_hid_boot_mouse());
        assert_eq!(i.num_endpoints, 1);
        assert!(matches!(items[1], Descriptor::Other { kind: 0x21, .. }));
        let Descriptor::Endpoint(e) = items[2] else {
            panic!("expected endpoint");
        };
        assert_eq!(e.number, 1);
        assert!(e.is_in);
        assert_eq!(e.transfer, TransferType::Interrupt);
        assert_eq!(e.max_packet_size, 8);
        assert_eq!(e.transactions_per_microframe, 1);
        assert_eq!(e.interval, 10);
    }

    #[test]
    fn endpoint_packet_size_splits_transactions() {
        // High-speed isochronous OUT endpoint 2, 1024 bytes with 3 transactions.
        let raw = [7, 5, 0x02, 0x01, 0x00, 0x14, 1];
        let e = Endpoint::parse(&raw).unwrap();
        assert!(!e.is_in);
        assert_eq!(e.number, 2);
        assert_eq!(e.transfer, TransferType::Isochronous);
        assert_eq!(e.max_packet_size, 1024);
        assert_eq!(e.transactions_per_microframe, 3);
    }

    #[test]
    fn total_length_must_fit_the_input() {
        let mut cfg = configuration(&[]);
        cfg[2] = 40; // claims 40 bytes, only 9 are present
        assert_eq!(Configuration::parse(&cfg), Err(UsbError::BadTotalLength));
    }

    #[test]
    fn zero_length_descriptor_is_an_error() {
        let cfg = configuration(&[0, 4, 0]);
        let c = Configuration::parse(&cfg).unwrap();
        assert_eq!(c.descriptors().next(), Some(Err(UsbError::BadLength)));
    }

    #[test]
    fn mouse_is_recognised() {
        let i = Interface {
            number: 0,
            alternate: 0,
            num_endpoints: 1,
            class: 3,
            subclass: 1,
            protocol: 2,
        };
        assert!(i.is_hid_boot_mouse());
        assert!(!i.is_hid_boot_keyboard());
    }
}
