// SPDX-License-Identifier: MIT OR Apache-2.0
//! TCP header parse and build (RFC 793), without options. There is no state
//! machine here; a caller decides what each segment means.

use crate::checksum::pseudo_header;
use crate::ipv4::PROTO_TCP;
use crate::{be16, be32, put16, put32, put_bytes, NetError};

/// Length of a TCP header without options.
pub const HEADER_LEN: usize = 20;

/// FIN flag.
pub const FIN: u8 = 0x01;
/// SYN flag.
pub const SYN: u8 = 0x02;
/// RST flag.
pub const RST: u8 = 0x04;
/// PSH flag.
pub const PSH: u8 = 0x08;
/// ACK flag.
pub const ACK: u8 = 0x10;

/// The fields of a TCP header, without options.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Header {
    /// Source port.
    pub src_port: u16,
    /// Destination port.
    pub dst_port: u16,
    /// Sequence number.
    pub seq: u32,
    /// Acknowledgment number (meaningful when ACK is set).
    pub ack: u32,
    /// Flag bits, from the constants in this module.
    pub flags: u8,
    /// Receive window.
    pub window: u16,
}

/// Parses a TCP segment into its header and payload. The checksum covers the
/// pseudo-header, so the IPv4 addresses are needed.
pub fn parse(
    src_ip: [u8; 4],
    dst_ip: [u8; 4],
    segment: &[u8],
) -> Result<(Header, &[u8]), NetError> {
    if segment.len() < HEADER_LEN {
        return Err(NetError::Truncated);
    }
    let data_offset = usize::from(segment[12] >> 4) * 4;
    if data_offset < HEADER_LEN {
        return Err(NetError::BadHeaderLength);
    }
    if data_offset > segment.len() {
        return Err(NetError::Truncated);
    }
    let length = u16::try_from(segment.len()).map_err(|_| NetError::BadLength)?;
    let mut sum = pseudo_header(src_ip, dst_ip, PROTO_TCP, length);
    sum.add(segment);
    if sum.finish() != 0 {
        return Err(NetError::BadChecksum);
    }
    let header = Header {
        src_port: be16(segment, 0)?,
        dst_port: be16(segment, 2)?,
        seq: be32(segment, 4)?,
        ack: be32(segment, 8)?,
        flags: segment[13],
        window: be16(segment, 14)?,
    };
    Ok((header, &segment[data_offset..]))
}

/// Writes a TCP segment with no options, followed by `payload`, into `out`.
/// Returns the segment length.
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
    put32(region, 4, header.seq)?;
    put32(region, 8, header.ack)?;
    region[12] = 5 << 4;
    region[13] = header.flags;
    put16(region, 14, header.window)?;
    put16(region, 16, 0)?;
    put16(region, 18, 0)?;
    put_bytes(region, HEADER_LEN, payload)?;
    let mut sum = pseudo_header(src_ip, dst_ip, PROTO_TCP, length);
    sum.add(region);
    put16(region, 16, sum.finish())?;
    Ok(total)
}

#[cfg(test)]
mod tests {
    use super::*;

    const SRC: [u8; 4] = [10, 0, 0, 1];
    const DST: [u8; 4] = [10, 0, 0, 2];

    #[test]
    fn syn_round_trip() {
        let syn = Header {
            src_port: 40000,
            dst_port: 80,
            seq: 0x1000_0000,
            ack: 0,
            flags: SYN,
            window: 65535,
        };
        let mut buf = [0u8; 32];
        let n = write(&mut buf, SRC, DST, &syn, &[]).unwrap();
        assert_eq!(n, HEADER_LEN);
        assert_eq!(buf[12], 0x50);
        let (parsed, payload) = parse(SRC, DST, &buf[..n]).unwrap();
        assert_eq!(parsed, syn);
        assert!(payload.is_empty());
    }

    #[test]
    fn syn_ack_with_payload_and_checksum_failure() {
        let synack = Header {
            src_port: 80,
            dst_port: 40000,
            seq: 77,
            ack: 0x1000_0001,
            flags: SYN | ACK,
            window: 1024,
        };
        let mut buf = [0u8; 40];
        let n = write(&mut buf, DST, SRC, &synack, b"hi").unwrap();
        let (parsed, payload) = parse(DST, SRC, &buf[..n]).unwrap();
        assert_eq!(parsed.flags, SYN | ACK);
        assert_eq!(parsed.ack, 0x1000_0001);
        assert_eq!(payload, b"hi");
        buf[4] ^= 0x01;
        assert_eq!(parse(DST, SRC, &buf[..n]), Err(NetError::BadChecksum));
    }
}
