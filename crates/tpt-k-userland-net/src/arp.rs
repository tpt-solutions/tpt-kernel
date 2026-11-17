// SPDX-License-Identifier: MIT OR Apache-2.0
//! ARP packets for Ethernet and IPv4 (RFC 826).

use crate::{array, be16, put16, put_bytes, NetError};

/// Length of an Ethernet/IPv4 ARP packet.
pub const PACKET_LEN: usize = 28;

/// The ARP operation.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Operation {
    /// Who has the target IP?
    Request,
    /// The target IP is at this MAC.
    Reply,
}

/// An Ethernet/IPv4 ARP packet.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Packet {
    /// Request or reply.
    pub operation: Operation,
    /// Sender hardware address.
    pub sender_mac: [u8; 6],
    /// Sender protocol address.
    pub sender_ip: [u8; 4],
    /// Target hardware address (zero in a request).
    pub target_mac: [u8; 6],
    /// Target protocol address.
    pub target_ip: [u8; 4],
}

/// Parses an ARP packet. Only Ethernet hardware and IPv4 protocol are accepted.
pub fn parse(bytes: &[u8]) -> Result<Packet, NetError> {
    if bytes.len() < PACKET_LEN {
        return Err(NetError::Truncated);
    }
    let htype = be16(bytes, 0)?;
    let ptype = be16(bytes, 2)?;
    if htype != 1 || ptype != 0x0800 || bytes[4] != 6 || bytes[5] != 4 {
        return Err(NetError::Unsupported);
    }
    let operation = match be16(bytes, 6)? {
        1 => Operation::Request,
        2 => Operation::Reply,
        _ => return Err(NetError::Unsupported),
    };
    Ok(Packet {
        operation,
        sender_mac: array(bytes, 8)?,
        sender_ip: array(bytes, 14)?,
        target_mac: array(bytes, 18)?,
        target_ip: array(bytes, 24)?,
    })
}

/// Writes an ARP packet into `out` and returns its length.
pub fn write(out: &mut [u8], packet: &Packet) -> Result<usize, NetError> {
    put16(out, 0, 1)?;
    put16(out, 2, 0x0800)?;
    put_bytes(out, 4, &[6, 4])?;
    let op = match packet.operation {
        Operation::Request => 1,
        Operation::Reply => 2,
    };
    put16(out, 6, op)?;
    put_bytes(out, 8, &packet.sender_mac)?;
    put_bytes(out, 14, &packet.sender_ip)?;
    put_bytes(out, 18, &packet.target_mac)?;
    put_bytes(out, 24, &packet.target_ip)?;
    Ok(PACKET_LEN)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn request_round_trip() {
        let req = Packet {
            operation: Operation::Request,
            sender_mac: [0x02, 0, 0, 0, 0, 1],
            sender_ip: [10, 0, 0, 1],
            target_mac: [0; 6],
            target_ip: [10, 0, 0, 2],
        };
        let mut buf = [0u8; PACKET_LEN];
        assert_eq!(write(&mut buf, &req), Ok(PACKET_LEN));
        assert_eq!(&buf[..8], &[0, 1, 8, 0, 6, 4, 0, 1]);
        assert_eq!(parse(&buf), Ok(req));
    }

    #[test]
    fn reply_and_unsupported_types() {
        let mut buf = [0u8; PACKET_LEN];
        let reply = Packet {
            operation: Operation::Reply,
            sender_mac: [1; 6],
            sender_ip: [10, 0, 0, 2],
            target_mac: [2; 6],
            target_ip: [10, 0, 0, 1],
        };
        write(&mut buf, &reply).unwrap();
        assert_eq!(parse(&buf).unwrap().operation, Operation::Reply);
        buf[1] = 6; // hardware type is not Ethernet
        assert_eq!(parse(&buf), Err(NetError::Unsupported));
        assert_eq!(parse(&buf[..27]), Err(NetError::Truncated));
    }
}
