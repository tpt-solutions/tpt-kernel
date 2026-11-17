// SPDX-License-Identifier: MIT OR Apache-2.0
//! Ethernet II frame header.

use crate::{array, be16, put16, put_bytes, NetError};

/// Length of the Ethernet II header.
pub const HEADER_LEN: usize = 14;
/// EtherType for IPv4.
pub const ETHERTYPE_IPV4: u16 = 0x0800;
/// EtherType for ARP.
pub const ETHERTYPE_ARP: u16 = 0x0806;

/// An Ethernet II header.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Header {
    /// Destination MAC address.
    pub dst: [u8; 6],
    /// Source MAC address.
    pub src: [u8; 6],
    /// EtherType of the payload.
    pub ethertype: u16,
}

/// Splits a frame into its header and payload.
pub fn parse(frame: &[u8]) -> Result<(Header, &[u8]), NetError> {
    let header = Header {
        dst: array(frame, 0)?,
        src: array(frame, 6)?,
        ethertype: be16(frame, 12)?,
    };
    Ok((header, &frame[HEADER_LEN..]))
}

/// Writes a header into the start of `out` and returns its length.
pub fn write(out: &mut [u8], header: &Header) -> Result<usize, NetError> {
    put_bytes(out, 0, &header.dst)?;
    put_bytes(out, 6, &header.src)?;
    put16(out, 12, header.ethertype)?;
    Ok(HEADER_LEN)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn header_round_trip() {
        let h = Header {
            dst: [0xff; 6],
            src: [0x02, 0, 0, 0, 0, 1],
            ethertype: ETHERTYPE_ARP,
        };
        let mut frame = [0u8; 20];
        assert_eq!(write(&mut frame, &h), Ok(HEADER_LEN));
        let (parsed, payload) = parse(&frame).unwrap();
        assert_eq!(parsed, h);
        assert_eq!(payload.len(), 6);
        assert_eq!(parse(&frame[..13]), Err(NetError::Truncated));
    }
}
