// SPDX-License-Identifier: MIT OR Apache-2.0
//! IPv4 headers (RFC 791). Options are not built, and are skipped on parse.

use crate::checksum::checksum;
use crate::{array, be16, put16, put_bytes, NetError};

/// Length of an IPv4 header without options.
pub const MIN_HEADER_LEN: usize = 20;
/// Protocol number for ICMP.
pub const PROTO_ICMP: u8 = 1;
/// Protocol number for TCP.
pub const PROTO_TCP: u8 = 6;
/// Protocol number for UDP.
pub const PROTO_UDP: u8 = 17;

/// The fields of an IPv4 header.
///
/// `header_len` and `total_len` are set by [`parse`]. [`write`] computes them
/// from the payload length, so their values in the struct are ignored there.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Header {
    /// Time to live.
    pub ttl: u8,
    /// Protocol of the payload.
    pub protocol: u8,
    /// Source address.
    pub src: [u8; 4],
    /// Destination address.
    pub dst: [u8; 4],
    /// Identification, for fragment reassembly.
    pub identification: u16,
    /// The don't-fragment flag.
    pub dont_fragment: bool,
    /// The more-fragments flag.
    pub more_fragments: bool,
    /// Fragment offset in units of 8 bytes.
    pub fragment_offset: u16,
    /// Header length in bytes (20 or more).
    pub header_len: usize,
    /// Total datagram length in bytes, header included.
    pub total_len: usize,
}

impl Header {
    /// True if this datagram is a fragment (more fragments follow, or it is
    /// not the first piece).
    #[must_use]
    pub fn is_fragment(&self) -> bool {
        self.more_fragments || self.fragment_offset != 0
    }
}

/// Parses a datagram into its header and payload. The header checksum must
/// verify, and the payload is cut to the total length in the header.
pub fn parse(bytes: &[u8]) -> Result<(Header, &[u8]), NetError> {
    if bytes.len() < MIN_HEADER_LEN {
        return Err(NetError::Truncated);
    }
    if bytes[0] >> 4 != 4 {
        return Err(NetError::BadVersion);
    }
    let header_len = usize::from(bytes[0] & 0x0F) * 4;
    if header_len < MIN_HEADER_LEN {
        return Err(NetError::BadHeaderLength);
    }
    if bytes.len() < header_len {
        return Err(NetError::Truncated);
    }
    let total_len = usize::from(be16(bytes, 2)?);
    if total_len < header_len || total_len > bytes.len() {
        return Err(NetError::BadLength);
    }
    if checksum(&bytes[..header_len]) != 0 {
        return Err(NetError::BadChecksum);
    }
    let flags = be16(bytes, 6)?;
    let header = Header {
        ttl: bytes[8],
        protocol: bytes[9],
        src: array(bytes, 12)?,
        dst: array(bytes, 16)?,
        identification: be16(bytes, 4)?,
        dont_fragment: flags & 0x4000 != 0,
        more_fragments: flags & 0x2000 != 0,
        fragment_offset: flags & 0x1FFF,
        header_len,
        total_len,
    };
    Ok((header, &bytes[header_len..total_len]))
}

/// Writes a 20-byte header for a payload of `payload_len` bytes into `out` and
/// returns 20. The payload is the caller's to place after the header.
pub fn write(out: &mut [u8], header: &Header, payload_len: usize) -> Result<usize, NetError> {
    let total = u16::try_from(MIN_HEADER_LEN + payload_len).map_err(|_| NetError::BadLength)?;
    let region = out
        .get_mut(..MIN_HEADER_LEN)
        .ok_or(NetError::BufferTooSmall)?;
    region[0] = 0x45;
    region[1] = 0;
    put16(region, 2, total)?;
    put16(region, 4, header.identification)?;
    let mut flags = header.fragment_offset & 0x1FFF;
    if header.dont_fragment {
        flags |= 0x4000;
    }
    if header.more_fragments {
        flags |= 0x2000;
    }
    put16(region, 6, flags)?;
    region[8] = header.ttl;
    region[9] = header.protocol;
    put16(region, 10, 0)?;
    put_bytes(region, 12, &header.src)?;
    put_bytes(region, 16, &header.dst)?;
    let ck = checksum(region);
    put16(region, 10, ck)?;
    Ok(MIN_HEADER_LEN)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> Header {
        Header {
            ttl: 64,
            protocol: PROTO_UDP,
            src: [192, 168, 1, 10],
            dst: [192, 168, 1, 1],
            identification: 0x1c46,
            dont_fragment: true,
            more_fragments: false,
            fragment_offset: 0,
            header_len: 0,
            total_len: 0,
        }
    }

    #[test]
    fn build_then_parse_round_trips_and_verifies() {
        let mut buf = [0u8; 64];
        let payload = [9u8; 12];
        write(&mut buf, &sample(), payload.len()).unwrap();
        buf[MIN_HEADER_LEN..MIN_HEADER_LEN + 12].copy_from_slice(&payload);
        let (h, body) = parse(&buf[..MIN_HEADER_LEN + 12]).unwrap();
        assert_eq!(h.ttl, 64);
        assert_eq!(h.protocol, PROTO_UDP);
        assert_eq!(h.src, [192, 168, 1, 10]);
        assert_eq!(h.dst, [192, 168, 1, 1]);
        assert!(h.dont_fragment);
        assert!(!h.is_fragment());
        assert_eq!(h.header_len, 20);
        assert_eq!(h.total_len, 32);
        assert_eq!(body, &payload);
    }

    #[test]
    fn corrupted_header_is_refused() {
        let mut buf = [0u8; 20];
        write(&mut buf, &sample(), 0).unwrap();
        buf[8] ^= 0x01; // change TTL without fixing the checksum
        assert_eq!(parse(&buf), Err(NetError::BadChecksum));
        let mut buf = [0u8; 20];
        write(&mut buf, &sample(), 0).unwrap();
        buf[0] = 0x65;
        assert_eq!(parse(&buf), Err(NetError::BadVersion));
    }

    #[test]
    fn fragment_flags_decode() {
        let mut h = sample();
        h.dont_fragment = false;
        h.more_fragments = true;
        h.fragment_offset = 185;
        let mut buf = [0u8; 20];
        write(&mut buf, &h, 0).unwrap();
        let (parsed, _) = parse(&buf).unwrap();
        assert!(parsed.more_fragments);
        assert_eq!(parsed.fragment_offset, 185);
        assert!(parsed.is_fragment());
    }
}
