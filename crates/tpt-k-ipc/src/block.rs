// SPDX-License-Identifier: MIT OR Apache-2.0
//! Block-device IPC protocol: sector-addressed reads, writes, flushes and
//! device information, carried as `tpt-k-ipc-serde` frames.
//!
//! A request frame has three words, `[op, lba, count]`, and a payload. Writes
//! carry their sector data as the payload; the other operations have none.
//! A reply frame has a status word (0 for success, otherwise an error code),
//! then either a payload (reads) or, for `GetInfo`, the device description in
//! three more words.
//!
//! The server side is [`serve_block`]. It checks each request's arithmetic and
//! bounds against the device's [`BlockInfo`] before the device is called, and
//! the device checks them again through [`check_range`].
//!
//! Capability checks are not part of this module. The request travels inside
//! the generic capability dispatch of this crate (see [`crate::serve`]).

use tpt_k_ipc_serde::{decode, encode, frame_len, seal, SerdeError};

/// Words in a request frame: operation, LBA and count.
pub const REQUEST_WORDS: usize = 3;

/// A block-protocol operation.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Op {
    /// Read `count` sectors starting at `lba`. The reply payload is the data.
    Read,
    /// Write the request payload to `count` sectors starting at `lba`.
    Write,
    /// Make earlier writes durable.
    Flush,
    /// Describe the device. The reply carries a [`BlockInfo`].
    GetInfo,
}

impl Op {
    /// The wire code of the operation.
    #[must_use]
    pub const fn word(self) -> u64 {
        match self {
            Self::Read => 1,
            Self::Write => 2,
            Self::Flush => 3,
            Self::GetInfo => 4,
        }
    }

    /// The operation for a wire code, or `None` if the code is unknown.
    #[must_use]
    pub const fn from_word(word: u64) -> Option<Self> {
        Some(match word {
            1 => Self::Read,
            2 => Self::Write,
            3 => Self::Flush,
            4 => Self::GetInfo,
            _ => return None,
        })
    }
}

/// Why a block operation failed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BlockError {
    /// The sector range runs past the end of the device, or its byte length
    /// does not fit.
    OutOfRange,
    /// The device failed to complete the operation.
    Io,
    /// A write was sent to a read-only device.
    ReadOnly,
    /// The request is malformed: zero count, a payload of the wrong length, or
    /// a reply buffer too small for the data.
    BadRequest,
}

impl BlockError {
    /// The non-zero status word for this error.
    #[must_use]
    pub const fn code(self) -> u64 {
        match self {
            Self::OutOfRange => 1,
            Self::Io => 2,
            Self::ReadOnly => 3,
            Self::BadRequest => 4,
        }
    }

    /// The error for a non-zero status word, or `None` if it is not one.
    #[must_use]
    pub const fn from_code(code: u64) -> Option<Self> {
        Some(match code {
            1 => Self::OutOfRange,
            2 => Self::Io,
            3 => Self::ReadOnly,
            4 => Self::BadRequest,
            _ => return None,
        })
    }
}

/// The status word for a result: 0 for success, the error code otherwise.
#[must_use]
pub fn status_word(result: Result<(), BlockError>) -> u64 {
    match result {
        Ok(()) => 0,
        Err(e) => e.code(),
    }
}

/// The result for a status word, or `None` if the word is not a status.
#[must_use]
pub fn status_from_word(word: u64) -> Option<Result<(), BlockError>> {
    if word == 0 {
        Some(Ok(()))
    } else {
        BlockError::from_code(word).map(Err)
    }
}

/// What a device is.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BlockInfo {
    /// Bytes per sector.
    pub sector_size: u32,
    /// Number of sectors.
    pub sector_count: u64,
    /// True if writes are refused.
    pub read_only: bool,
}

impl BlockInfo {
    /// Capacity in bytes, or `None` if it does not fit in a `u64`.
    #[must_use]
    pub fn capacity_bytes(&self) -> Option<u64> {
        self.sector_count.checked_mul(u64::from(self.sector_size))
    }
}

/// Why a block frame could not be read.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ProtoError {
    /// The frame itself is malformed.
    Frame(SerdeError),
    /// The frame does not have the words its kind requires.
    WordCount,
    /// The operation code is unknown.
    BadOp(u64),
    /// The status word is not a status.
    BadStatus(u64),
    /// A field does not fit its type on this host.
    Range,
}

/// A decoded request, borrowing its payload from the frame.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Request<'a> {
    /// The operation.
    pub op: Op,
    /// First sector.
    pub lba: u64,
    /// Number of sectors.
    pub count: u32,
    /// The write data; empty for other operations.
    pub data: &'a [u8],
}

/// Writes a request frame into `out` and returns its length.
pub fn encode_request(
    out: &mut [u8],
    op: Op,
    lba: u64,
    count: u32,
    data: &[u8],
) -> Result<usize, SerdeError> {
    encode(out, &[op.word(), lba, u64::from(count)], data)
}

/// Reads a request frame.
pub fn decode_request(bytes: &[u8]) -> Result<Request<'_>, ProtoError> {
    let frame = decode(bytes).map_err(ProtoError::Frame)?;
    if frame.word_count() != REQUEST_WORDS {
        return Err(ProtoError::WordCount);
    }
    let raw_op = frame.word(0).ok_or(ProtoError::WordCount)?;
    let op = Op::from_word(raw_op).ok_or(ProtoError::BadOp(raw_op))?;
    let lba = frame.word(1).ok_or(ProtoError::WordCount)?;
    let count = u32::try_from(frame.word(2).ok_or(ProtoError::WordCount)?)
        .map_err(|_| ProtoError::Range)?;
    Ok(Request {
        op,
        lba,
        count,
        data: frame.payload(),
    })
}

/// Writes a reply frame with a status and a payload (for reads, or empty).
pub fn encode_status_reply(
    out: &mut [u8],
    result: Result<(), BlockError>,
    payload: &[u8],
) -> Result<usize, SerdeError> {
    encode(out, &[status_word(result)], payload)
}

/// Reads a status reply, returning the status and the payload.
pub fn decode_status_reply(bytes: &[u8]) -> Result<(Result<(), BlockError>, &[u8]), ProtoError> {
    let frame = decode(bytes).map_err(ProtoError::Frame)?;
    if frame.word_count() != 1 {
        return Err(ProtoError::WordCount);
    }
    let word = frame.word(0).ok_or(ProtoError::WordCount)?;
    let status = status_from_word(word).ok_or(ProtoError::BadStatus(word))?;
    Ok((status, frame.payload()))
}

/// Reads a `GetInfo` reply: the device description, or the error the device
/// reported.
pub fn decode_info_reply(bytes: &[u8]) -> Result<Result<BlockInfo, BlockError>, ProtoError> {
    let frame = decode(bytes).map_err(ProtoError::Frame)?;
    match frame.word_count() {
        1 => {
            let word = frame.word(0).ok_or(ProtoError::WordCount)?;
            match status_from_word(word).ok_or(ProtoError::BadStatus(word))? {
                Err(e) => Ok(Err(e)),
                Ok(()) => Err(ProtoError::WordCount),
            }
        }
        4 => {
            let status = frame.word(0).ok_or(ProtoError::WordCount)?;
            if status != 0 {
                return Err(ProtoError::BadStatus(status));
            }
            let sector_size = u32::try_from(frame.word(1).ok_or(ProtoError::WordCount)?)
                .map_err(|_| ProtoError::Range)?;
            let sector_count = frame.word(2).ok_or(ProtoError::WordCount)?;
            let read_only = frame.word(3).ok_or(ProtoError::WordCount)? != 0;
            Ok(Ok(BlockInfo {
                sector_size,
                sector_count,
                read_only,
            }))
        }
        _ => Err(ProtoError::WordCount),
    }
}

/// Writes a `GetInfo` reply describing `info`.
pub fn encode_info_reply(out: &mut [u8], info: &BlockInfo) -> Result<usize, SerdeError> {
    encode(
        out,
        &[
            0,
            u64::from(info.sector_size),
            info.sector_count,
            u64::from(info.read_only),
        ],
        &[],
    )
}

/// Checks a sector range and returns its byte length.
///
/// Refuses a zero count (`BadRequest`), a range that runs past the device or
/// wraps (`OutOfRange`), and a byte length that does not fit in `usize`
/// (`OutOfRange`).
pub fn check_range(info: &BlockInfo, lba: u64, count: u32) -> Result<usize, BlockError> {
    if count == 0 {
        return Err(BlockError::BadRequest);
    }
    let end = lba
        .checked_add(u64::from(count))
        .ok_or(BlockError::OutOfRange)?;
    if end > info.sector_count {
        return Err(BlockError::OutOfRange);
    }
    let bytes = u64::from(count)
        .checked_mul(u64::from(info.sector_size))
        .ok_or(BlockError::OutOfRange)?;
    usize::try_from(bytes).map_err(|_| BlockError::OutOfRange)
}

/// A block device, as the server sees it.
pub trait BlockDevice {
    /// The device description.
    fn info(&self) -> BlockInfo;
    /// Reads `out.len()` bytes starting at sector `lba`. The caller has checked
    /// the range with [`check_range`]; a device should check it again.
    fn read(&mut self, lba: u64, out: &mut [u8]) -> Result<(), BlockError>;
    /// Writes `data` starting at sector `lba`. Same checking rule as `read`.
    fn write(&mut self, lba: u64, data: &[u8]) -> Result<(), BlockError>;
    /// Makes earlier writes durable.
    fn flush(&mut self) -> Result<(), BlockError>;
}

enum Out {
    Empty,
    Data(usize),
    Info,
}

fn dispatch<D: BlockDevice>(
    dev: &mut D,
    info: &BlockInfo,
    req: &Request<'_>,
    out: &mut [u8],
) -> Result<Out, BlockError> {
    match req.op {
        Op::Read => {
            let len = check_range(info, req.lba, req.count)?;
            if len > out.len() {
                return Err(BlockError::BadRequest);
            }
            dev.read(req.lba, &mut out[..len])?;
            Ok(Out::Data(len))
        }
        Op::Write => {
            if info.read_only {
                return Err(BlockError::ReadOnly);
            }
            let len = check_range(info, req.lba, req.count)?;
            if req.data.len() != len {
                return Err(BlockError::BadRequest);
            }
            dev.write(req.lba, req.data)?;
            Ok(Out::Empty)
        }
        Op::Flush => {
            dev.flush()?;
            Ok(Out::Empty)
        }
        Op::GetInfo => Ok(Out::Info),
    }
}

/// Serves one block request frame, writing the reply frame into `reply`.
///
/// Every request gets a reply. Refusals are reported as a status, not as an
/// error from this function. The error case is only a `reply` buffer too small
/// for a reply header.
pub fn serve_block<D: BlockDevice>(
    dev: &mut D,
    request: &[u8],
    reply: &mut [u8],
) -> Result<usize, SerdeError> {
    let body = frame_len(1, 0);
    if reply.len() < body {
        return Err(SerdeError::BufferTooSmall);
    }
    let info = dev.info();
    let (result, out) = match decode_request(request) {
        Err(_) => (Err(BlockError::BadRequest), Out::Empty),
        Ok(req) => match dispatch(dev, &info, &req, &mut reply[body..]) {
            Ok(out) => (Ok(()), out),
            Err(e) => (Err(e), Out::Empty),
        },
    };
    match out {
        Out::Empty => seal(reply, &[status_word(result)], 0),
        Out::Data(n) => seal(reply, &[status_word(result)], n),
        Out::Info => {
            let words = [
                0,
                u64::from(info.sector_size),
                info.sector_count,
                u64::from(info.read_only),
            ];
            seal(reply, &words, 0)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::vec;
    use std::vec::Vec;

    /// A RAM disk that checks its own bounds, as a device layer should.
    struct RamDisk {
        sector_size: u32,
        read_only: bool,
        flushes: u32,
        bytes: Vec<u8>,
    }

    impl RamDisk {
        fn new(sector_size: u32, sectors: u64, read_only: bool) -> Self {
            Self {
                sector_size,
                read_only,
                flushes: 0,
                bytes: vec![0; sector_size as usize * sectors as usize],
            }
        }
    }

    impl BlockDevice for RamDisk {
        fn info(&self) -> BlockInfo {
            BlockInfo {
                sector_size: self.sector_size,
                sector_count: self.bytes.len() as u64 / u64::from(self.sector_size),
                read_only: self.read_only,
            }
        }

        fn read(&mut self, lba: u64, out: &mut [u8]) -> Result<(), BlockError> {
            let count = out.len() / self.sector_size as usize;
            let info = self.info();
            check_range(&info, lba, count as u32)?;
            let at = lba as usize * self.sector_size as usize;
            out.copy_from_slice(&self.bytes[at..at + out.len()]);
            Ok(())
        }

        fn write(&mut self, lba: u64, data: &[u8]) -> Result<(), BlockError> {
            if self.read_only {
                return Err(BlockError::ReadOnly);
            }
            let count = data.len() / self.sector_size as usize;
            let info = self.info();
            check_range(&info, lba, count as u32)?;
            let at = lba as usize * self.sector_size as usize;
            self.bytes[at..at + data.len()].copy_from_slice(data);
            Ok(())
        }

        fn flush(&mut self) -> Result<(), BlockError> {
            self.flushes += 1;
            Ok(())
        }
    }

    /// Runs one request through the server and decodes the status reply.
    fn call(
        disk: &mut RamDisk,
        op: Op,
        lba: u64,
        count: u32,
        data: &[u8],
        reply_len: usize,
    ) -> (Result<(), BlockError>, Vec<u8>) {
        let mut req = [0u8; 4096];
        let n = encode_request(&mut req, op, lba, count, data).unwrap();
        let mut reply = vec![0u8; reply_len];
        let m = serve_block(disk, &req[..n], &mut reply).unwrap();
        let (status, payload) = decode_status_reply(&reply[..m]).unwrap();
        (status, payload.to_vec())
    }

    #[test]
    fn request_round_trips_with_payload() {
        let mut buf = [0u8; 64];
        let n = encode_request(&mut buf, Op::Write, 7, 2, &[9; 8]).unwrap();
        let req = decode_request(&buf[..n]).unwrap();
        assert_eq!(
            req,
            Request {
                op: Op::Write,
                lba: 7,
                count: 2,
                data: &[9; 8],
            }
        );
    }

    #[test]
    fn op_and_status_codes_round_trip() {
        for op in [Op::Read, Op::Write, Op::Flush, Op::GetInfo] {
            assert_eq!(Op::from_word(op.word()), Some(op));
        }
        assert_eq!(Op::from_word(0), None);
        for e in [
            BlockError::OutOfRange,
            BlockError::Io,
            BlockError::ReadOnly,
            BlockError::BadRequest,
        ] {
            assert_eq!(status_from_word(status_word(Err(e))), Some(Err(e)));
        }
        assert_eq!(status_from_word(status_word(Ok(()))), Some(Ok(())));
        assert_eq!(status_from_word(99), None);
    }

    #[test]
    fn malformed_requests_are_refused() {
        let mut buf = [0u8; 64];
        let n = encode(&mut buf, &[1, 0], &[]).unwrap();
        assert_eq!(decode_request(&buf[..n]), Err(ProtoError::WordCount));
        let n = encode(&mut buf, &[42, 0, 1], &[]).unwrap();
        assert_eq!(decode_request(&buf[..n]), Err(ProtoError::BadOp(42)));
        let n = encode(&mut buf, &[1, 0, u64::MAX], &[]).unwrap();
        assert_eq!(decode_request(&buf[..n]), Err(ProtoError::Range));
    }

    #[test]
    fn write_then_read_returns_the_same_sectors() {
        let mut disk = RamDisk::new(512, 8, false);
        let pattern: Vec<u8> = (0..1024).map(|i| (i % 251) as u8).collect();
        let (status, _) = call(&mut disk, Op::Write, 2, 2, &pattern, 64);
        assert_eq!(status, Ok(()));
        let (status, payload) = call(&mut disk, Op::Read, 2, 2, &[], 1100);
        assert_eq!(status, Ok(()));
        assert_eq!(payload, pattern);
        // A neighbouring sector is still zero.
        let (_, payload) = call(&mut disk, Op::Read, 4, 1, &[], 600);
        assert!(payload.iter().all(|b| *b == 0));
    }

    #[test]
    fn ranges_past_the_end_are_refused() {
        let mut disk = RamDisk::new(512, 8, false);
        let (status, _) = call(&mut disk, Op::Read, 7, 2, &[], 2048);
        assert_eq!(status, Err(BlockError::OutOfRange));
        let (status, _) = call(&mut disk, Op::Read, 8, 1, &[], 600);
        assert_eq!(status, Err(BlockError::OutOfRange));
    }

    #[test]
    fn count_and_lba_overflow_are_refused_not_wrapped() {
        let mut disk = RamDisk::new(512, 8, false);
        // lba + count wraps past u64::MAX.
        let (status, _) = call(&mut disk, Op::Read, u64::MAX, 2, &[], 600);
        assert_eq!(status, Err(BlockError::OutOfRange));
        // A huge count is past the device end, so it is refused before any
        // byte length is computed or copied.
        let (status, _) = call(&mut disk, Op::Read, 0, u32::MAX, &[], 600);
        assert_eq!(status, Err(BlockError::OutOfRange));
        let info = BlockInfo {
            sector_size: u32::MAX,
            sector_count: u64::MAX,
            read_only: false,
        };
        assert_eq!(
            check_range(&info, 0, u32::MAX),
            Ok(u32::MAX as usize * u32::MAX as usize)
        );
    }

    #[test]
    fn zero_count_and_size_mismatch_are_bad_requests() {
        let mut disk = RamDisk::new(512, 8, false);
        let (status, _) = call(&mut disk, Op::Read, 0, 0, &[], 600);
        assert_eq!(status, Err(BlockError::BadRequest));
        let (status, _) = call(&mut disk, Op::Write, 0, 1, &[0; 100], 600);
        assert_eq!(status, Err(BlockError::BadRequest));
        // A read into a reply buffer too small for the data.
        let (status, _) = call(&mut disk, Op::Read, 0, 2, &[], 100);
        assert_eq!(status, Err(BlockError::BadRequest));
    }

    #[test]
    fn read_only_device_refuses_writes_but_allows_reads_and_flush() {
        let mut disk = RamDisk::new(512, 4, true);
        let (status, _) = call(&mut disk, Op::Write, 0, 1, &[1; 512], 64);
        assert_eq!(status, Err(BlockError::ReadOnly));
        let (status, _) = call(&mut disk, Op::Read, 0, 1, &[], 600);
        assert_eq!(status, Ok(()));
        let (status, _) = call(&mut disk, Op::Flush, 0, 0, &[], 64);
        assert_eq!(status, Ok(()));
        assert_eq!(disk.flushes, 1);
    }

    #[test]
    fn get_info_describes_the_device() {
        let mut disk = RamDisk::new(4096, 16, true);
        let mut req = [0u8; 64];
        let n = encode_request(&mut req, Op::GetInfo, 0, 0, &[]).unwrap();
        let mut reply = [0u8; 128];
        let m = serve_block(&mut disk, &req[..n], &mut reply).unwrap();
        let info = decode_info_reply(&reply[..m]).unwrap().unwrap();
        assert_eq!(
            info,
            BlockInfo {
                sector_size: 4096,
                sector_count: 16,
                read_only: true,
            }
        );
        assert_eq!(info.capacity_bytes(), Some(65536));
    }

    #[test]
    fn info_reply_round_trips_and_reports_errors() {
        let info = BlockInfo {
            sector_size: 512,
            sector_count: 2048,
            read_only: false,
        };
        let mut out = [0u8; 64];
        let n = encode_info_reply(&mut out, &info).unwrap();
        assert_eq!(decode_info_reply(&out[..n]), Ok(Ok(info)));
        let n = encode(&mut out, &[BlockError::Io.code()], &[]).unwrap();
        assert_eq!(decode_info_reply(&out[..n]), Ok(Err(BlockError::Io)));
    }

    #[test]
    fn garbage_request_gets_a_bad_request_reply() {
        let mut disk = RamDisk::new(512, 4, false);
        let mut reply = [0u8; 64];
        let m = serve_block(&mut disk, &[0u8; 3], &mut reply).unwrap();
        let (status, payload) = decode_status_reply(&reply[..m]).unwrap();
        assert_eq!(status, Err(BlockError::BadRequest));
        assert!(payload.is_empty());
    }

    #[test]
    fn device_layer_refuses_out_of_range_on_its_own() {
        let mut disk = RamDisk::new(512, 4, false);
        let mut out = [0u8; 512];
        assert_eq!(disk.read(4, &mut out), Err(BlockError::OutOfRange));
        assert_eq!(disk.write(u64::MAX, &[0; 512]), Err(BlockError::OutOfRange));
    }
}
