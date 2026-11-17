// SPDX-License-Identifier: MIT OR Apache-2.0
//! tpt-k user-space TCP/IP network stack: pure protocol codecs.
//!
//! Each module parses and builds one wire format over byte slices, without
//! copying and without a heap: Ethernet II, ARP, IPv4, ICMP echo, UDP and a
//! TCP header. The module [`arp_cache`] holds a fixed-size ARP table. There are
//! no sockets and no device I/O here; the daemon supplies those.
//!
//! Checksums are checked on parse, and written on build. A TCP state machine is
//! not included.

pub mod arp;
pub mod arp_cache;
pub mod checksum;
pub mod ethernet;
pub mod icmp;
pub mod ipv4;
pub mod tcp;
pub mod udp;

/// Why a frame or packet could not be parsed or built.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NetError {
    /// The input ends before the structure does.
    Truncated,
    /// The IP version field is not the one expected.
    BadVersion,
    /// A header length field is below its minimum or past the input.
    BadHeaderLength,
    /// A length field disagrees with the input or with the protocol limits.
    BadLength,
    /// A checksum does not verify.
    BadChecksum,
    /// A field names a hardware or protocol type this stack does not handle.
    Unsupported,
    /// The output buffer cannot hold the structure.
    BufferTooSmall,
}

/// Reads a big-endian `u16` at `at`.
pub(crate) fn be16(bytes: &[u8], at: usize) -> Result<u16, NetError> {
    let s = bytes.get(at..at + 2).ok_or(NetError::Truncated)?;
    Ok(u16::from_be_bytes([s[0], s[1]]))
}

/// Reads a big-endian `u32` at `at`.
pub(crate) fn be32(bytes: &[u8], at: usize) -> Result<u32, NetError> {
    let s = bytes.get(at..at + 4).ok_or(NetError::Truncated)?;
    Ok(u32::from_be_bytes([s[0], s[1], s[2], s[3]]))
}

/// Copies `N` bytes from `at`.
pub(crate) fn array<const N: usize>(bytes: &[u8], at: usize) -> Result<[u8; N], NetError> {
    let s = bytes.get(at..at + N).ok_or(NetError::Truncated)?;
    let mut out = [0u8; N];
    out.copy_from_slice(s);
    Ok(out)
}

/// Writes a big-endian `u16` at `at` of `out`.
pub(crate) fn put16(out: &mut [u8], at: usize, value: u16) -> Result<(), NetError> {
    out.get_mut(at..at + 2)
        .ok_or(NetError::BufferTooSmall)?
        .copy_from_slice(&value.to_be_bytes());
    Ok(())
}

/// Writes a big-endian `u32` at `at` of `out`.
pub(crate) fn put32(out: &mut [u8], at: usize, value: u32) -> Result<(), NetError> {
    out.get_mut(at..at + 4)
        .ok_or(NetError::BufferTooSmall)?
        .copy_from_slice(&value.to_be_bytes());
    Ok(())
}

/// Writes `bytes` at `at` of `out`.
pub(crate) fn put_bytes(out: &mut [u8], at: usize, bytes: &[u8]) -> Result<(), NetError> {
    out.get_mut(at..at + bytes.len())
        .ok_or(NetError::BufferTooSmall)?
        .copy_from_slice(bytes);
    Ok(())
}
