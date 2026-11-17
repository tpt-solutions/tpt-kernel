// SPDX-License-Identifier: MIT OR Apache-2.0
//! ICMP echo request and reply (RFC 792).

use crate::checksum::checksum;
use crate::{be16, put16, NetError};

/// Echo reply type.
pub const TYPE_ECHO_REPLY: u8 = 0;
/// Echo request type.
pub const TYPE_ECHO_REQUEST: u8 = 8;
/// Length of the echo header (type, code, checksum, identifier, sequence).
pub const HEADER_LEN: usize = 8;

/// The fields of an echo message.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Echo {
    /// True for a reply, false for a request.
    pub reply: bool,
    /// Identifier, chosen by the sender.
    pub identifier: u16,
    /// Sequence number.
    pub sequence: u16,
}

/// Parses an echo message into its fields and data. The whole message,
/// header and data, must checksum to zero.
pub fn parse(message: &[u8]) -> Result<(Echo, &[u8]), NetError> {
    if message.len() < HEADER_LEN {
        return Err(NetError::Truncated);
    }
    if checksum(message) != 0 {
        return Err(NetError::BadChecksum);
    }
    if message[1] != 0 {
        return Err(NetError::Unsupported);
    }
    let reply = match message[0] {
        TYPE_ECHO_REPLY => true,
        TYPE_ECHO_REQUEST => false,
        _ => return Err(NetError::Unsupported),
    };
    let echo = Echo {
        reply,
        identifier: be16(message, 4)?,
        sequence: be16(message, 6)?,
    };
    Ok((echo, &message[HEADER_LEN..]))
}

/// Writes an echo message with `data` into `out`, computing the checksum over
/// the whole message. Returns the message length.
pub fn write(out: &mut [u8], echo: &Echo, data: &[u8]) -> Result<usize, NetError> {
    let total = HEADER_LEN + data.len();
    let region = out.get_mut(..total).ok_or(NetError::BufferTooSmall)?;
    region[0] = if echo.reply {
        TYPE_ECHO_REPLY
    } else {
        TYPE_ECHO_REQUEST
    };
    region[1] = 0;
    put16(region, 2, 0)?;
    put16(region, 4, echo.identifier)?;
    put16(region, 6, echo.sequence)?;
    region[HEADER_LEN..].copy_from_slice(data);
    let ck = checksum(region);
    put16(region, 2, ck)?;
    Ok(total)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn echo_request_round_trip() {
        let req = Echo {
            reply: false,
            identifier: 0x1234,
            sequence: 7,
        };
        let data = b"ping data";
        let mut buf = [0u8; 64];
        let n = write(&mut buf, &req, data).unwrap();
        assert_eq!(n, HEADER_LEN + data.len());
        let (parsed, body) = parse(&buf[..n]).unwrap();
        assert_eq!(parsed, req);
        assert_eq!(body, data);
    }

    #[test]
    fn echo_reply_and_corruption() {
        let rep = Echo {
            reply: true,
            identifier: 1,
            sequence: 2,
        };
        let mut buf = [0u8; 16];
        let n = write(&mut buf, &rep, &[0xAA; 4]).unwrap();
        assert!(parse(&buf[..n]).unwrap().0.reply);
        buf[9] ^= 0xFF;
        assert_eq!(parse(&buf[..n]), Err(NetError::BadChecksum));
    }
}
