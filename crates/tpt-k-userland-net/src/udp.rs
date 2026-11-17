// SPDX-License-Identifier: MIT OR Apache-2.0
//! UDP headers (RFC 768). The checksum covers the IPv4 pseudo-header.

use crate::checksum::pseudo_header;
use crate::ipv4::PROTO_UDP;
use crate::{be16, put16, put_bytes, NetError};

/// Length of the UDP header.
pub const HEADER_LEN: usize = 8;

/// The fields of a UDP header. The length is implied by the payload.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Header {
    /// Source port.
    pub src_port: u16,
    /// Destination port.
    pub dst_port: u16,
}

/// Parses a UDP datagram. A checksum of zero means the sender did not compute
/// one, and is accepted. Otherwise it must verify against the pseudo-header.
pub fn parse(
    src_ip: [u8; 4],
    dst_ip: [u8; 4],
    segment: &[u8],
) -> Result<(Header, &[u8]), NetError> {
    if segment.len() < HEADER_LEN {
        return Err(NetError::Truncated);
    }
    let length = usize::from(be16(segment, 4)?);
    if length < HEADER_LEN || length > segment.len() {
        return Err(NetError::BadLength);
    }
    let segment = &segment[..length];
    if be16(segment, 6)? != 0 {
        let mut sum = pseudo_header(src_ip, dst_ip, PROTO_UDP, length as u16);
        sum.add(segment);
        if sum.finish() != 0 {
            return Err(NetError::BadChecksum);
        }
    }
    let header = Header {
        src_port: be16(segment, 0)?,
        dst_port: be16(segment, 2)?,
    };
    Ok((header, &segment[HEADER_LEN..]))
}

/// Writes a UDP datagram (header and payload) into `out` and returns its length.
/// The checksum is computed over the pseudo-header; a result of zero is sent as
/// 0xFFFF, since zero means "not computed" in IPv4.
pub fn write(
    out: &mut [u8],
    src_ip: [u8; 4],
    dst_ip: [u8; 4],
    header: &Header,
    payload: &[u8],
) -> Result<usize, NetError> {
    let total = HEADER_LEN + payload.len();
    let length = u16::try_from(total).map_err(|_| NetError::BadLength)?;
    let region = out.get_mut(..total).ok_or(NetError::BufferTooSmall)?;
    put16(region, 0, header.src_port)?;
    put16(region, 2, header.dst_port)?;
    put16(region, 4, length)?;
    put16(region, 6, 0)?;
    put_bytes(region, HEADER_LEN, payload)?;
    let mut sum = pseudo_header(src_ip, dst_ip, PROTO_UDP, length);
    sum.add(region);
    let mut ck = sum.finish();
    if ck == 0 {
        ck = 0xFFFF;
    }
    put16(region, 6, ck)?;
    Ok(total)
}

#[cfg(test)]
mod tests {
    use super::*;

    const SRC: [u8; 4] = [10, 0, 0, 1];
    const DST: [u8; 4] = [10, 0, 0, 2];

    #[test]
    fn checksum_matches_hand_computed_value() {
        // Pseudo-header sum 0x1420, UDP header sum 0x0513 (length 12), payload
        // "abcd" sums to 0xc4c6. Total 0xddf9, so the checksum is !0xddf9 = 0x2206.
        let mut buf = [0u8; 16];
        let h = Header {
            src_port: 1234,
            dst_port: 53,
        };
        let n = write(&mut buf, SRC, DST, &h, b"abcd").unwrap();
        assert_eq!(n, 12);
        assert_eq!(&buf[6..8], &[0x22, 0x06]);
        let (parsed, payload) = parse(SRC, DST, &buf[..n]).unwrap();
        assert_eq!(parsed, h);
        assert_eq!(payload, b"abcd");
    }

    #[test]
    fn wrong_pseudo_header_or_payload_is_refused() {
        let mut buf = [0u8; 16];
        let h = Header {
            src_port: 1234,
            dst_port: 53,
        };
        let n = write(&mut buf, SRC, DST, &h, b"abcd").unwrap();
        assert_eq!(
            parse([10, 0, 0, 9], DST, &buf[..n]),
            Err(NetError::BadChecksum)
        );
        buf[9] ^= 1;
        assert_eq!(parse(SRC, DST, &buf[..n]), Err(NetError::BadChecksum));
    }

    #[test]
    fn zero_checksum_is_accepted_as_not_computed() {
        let mut buf = [0u8; 12];
        buf[4..6].copy_from_slice(&12u16.to_be_bytes());
        buf[0..2].copy_from_slice(&5u16.to_be_bytes());
        buf[2..4].copy_from_slice(&6u16.to_be_bytes());
        assert!(parse(SRC, DST, &buf).is_ok());
    }
}
