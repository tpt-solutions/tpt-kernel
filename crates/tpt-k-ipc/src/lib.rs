// SPDX-License-Identifier: MIT OR Apache-2.0
//! tpt-k IPC client/server framework: capability-checked request dispatch.
//!
//! A request is a frame (from `tpt-k-ipc-serde`) with seven words, which carry
//! the opcode and the capability token, and an argument payload. A reply is a
//! frame with one status word and a reply payload.
//!
//! [`serve`] does the checks every service needs, in order: the frame must
//! parse, the token must verify under the server's key, the opcode must be
//! known, and the verified capability must carry the rights the opcode needs.
//! Only then does the service's [`Service::handle`] run. Every refusal still
//! produces a reply frame, so a client always gets an answer.

#![no_std]

#[cfg(test)]
extern crate std;

pub mod block;

use tpt_k_ipc_cap::{Capability, Rights, Token, TOKEN_LEN};
use tpt_k_ipc_serde::{decode, encode, frame_len, seal, Frame, SerdeError};

/// Words in a request frame: opcode, object, rights, and the four words of the
/// token's tag.
pub const REQUEST_WORDS: usize = 7;

/// Outcome of a request, carried as the reply's first word.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Status {
    /// The request was handled. The reply payload is the result.
    Ok = 0,
    /// The token did not verify, or it lacks the rights the opcode needs.
    Denied = 1,
    /// The request frame or its token was malformed.
    BadRequest = 2,
    /// The service does not know the opcode.
    UnknownOp = 3,
    /// The service could not hold the reply in the buffer it was given.
    Full = 4,
    /// The service refused the arguments.
    Invalid = 5,
}

impl Status {
    /// The status for a reply word, or `None` if the word is not a known status.
    #[must_use]
    pub const fn from_word(word: u64) -> Option<Self> {
        Some(match word {
            0 => Self::Ok,
            1 => Self::Denied,
            2 => Self::BadRequest,
            3 => Self::UnknownOp,
            4 => Self::Full,
            5 => Self::Invalid,
            _ => return None,
        })
    }
}

/// A request as the server sees it, after decoding and before verification.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Request<'a> {
    /// Which operation the caller wants.
    pub opcode: u32,
    /// The capability token the caller presents. Not yet verified.
    pub token: Token,
    /// The argument bytes.
    pub args: &'a [u8],
}

/// Writes a request frame into `out`.
pub fn encode_request(
    out: &mut [u8],
    opcode: u32,
    token: &Token,
    args: &[u8],
) -> Result<usize, SerdeError> {
    let t = token.to_bytes();
    let word = |at: usize| {
        let mut b = [0u8; 8];
        b.copy_from_slice(&t[at..at + 8]);
        u64::from_le_bytes(b)
    };
    let rights = {
        let mut b = [0u8; 4];
        b.copy_from_slice(&t[8..12]);
        u64::from(u32::from_le_bytes(b))
    };
    // The token is 44 bytes: object (0..8), rights (8..12), tag (12..44).
    let words = [
        u64::from(opcode),
        word(0),
        rights,
        word(12),
        word(20),
        word(28),
        word(36),
    ];
    encode(out, &words, args)
}

/// Why a request frame could not be read.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RequestError {
    /// The frame is malformed.
    Frame(SerdeError),
    /// The frame does not have the seven request words.
    WordCount,
    /// The opcode word does not fit in 32 bits.
    OpcodeRange,
    /// The token words do not form a valid token, such as undefined rights bits.
    BadToken,
}

/// Reads a request frame. The token is returned unverified.
pub fn decode_request(bytes: &[u8]) -> Result<Request<'_>, RequestError> {
    let frame = decode(bytes).map_err(RequestError::Frame)?;
    request_from_frame(&frame)
}

fn request_from_frame<'a>(frame: &Frame<'a>) -> Result<Request<'a>, RequestError> {
    if frame.word_count() != REQUEST_WORDS {
        return Err(RequestError::WordCount);
    }
    let w = |i: usize| frame.word(i).unwrap_or(0);
    let opcode = u32::try_from(w(0)).map_err(|_| RequestError::OpcodeRange)?;
    let mut t = [0u8; TOKEN_LEN];
    t[0..8].copy_from_slice(&w(1).to_le_bytes());
    t[8..12].copy_from_slice(&(w(2) as u32).to_le_bytes());
    t[12..20].copy_from_slice(&w(3).to_le_bytes());
    t[20..28].copy_from_slice(&w(4).to_le_bytes());
    t[28..36].copy_from_slice(&w(5).to_le_bytes());
    t[36..44].copy_from_slice(&w(6).to_le_bytes());
    let token = Token::from_bytes(&t).map_err(|_| RequestError::BadToken)?;
    Ok(Request {
        opcode,
        token,
        args: frame.payload(),
    })
}

/// A service: the operations it offers and how it handles them.
pub trait Service {
    /// The rights a caller must hold for `opcode`, or `None` if the opcode is
    /// not offered. Checked before `handle` runs.
    fn rights_for(&self, opcode: u32) -> Option<Rights>;

    /// Handles one request whose capability has been verified and carries the
    /// rights from `rights_for`. Writes the result into `reply` and returns its
    /// length, or returns the status to report.
    fn handle(
        &mut self,
        opcode: u32,
        capability: Capability,
        args: &[u8],
        reply: &mut [u8],
    ) -> Result<usize, Status>;
}

/// Serves one request frame and writes the reply frame into `reply`.
///
/// Returns the reply's length. A refusal is still a reply, with a status other
/// than `Ok` and no payload. The error case is only a `reply` buffer too small
/// for even an empty reply frame.
pub fn serve<S: Service>(
    service: &mut S,
    key: &[u8; 32],
    request: &[u8],
    reply: &mut [u8],
) -> Result<usize, SerdeError> {
    let body = frame_len(1, 0);
    if reply.len() < body {
        return Err(SerdeError::BufferTooSmall);
    }

    let (status, payload_len) = match decode_request(request) {
        Err(_) => (Status::BadRequest, 0),
        Ok(req) => match req.token.verify(key) {
            Err(_) => (Status::Denied, 0),
            Ok(capability) => match service.rights_for(req.opcode) {
                None => (Status::UnknownOp, 0),
                Some(needed) if !capability.allows(needed) => (Status::Denied, 0),
                Some(_) => {
                    match service.handle(req.opcode, capability, req.args, &mut reply[body..]) {
                        Ok(n) => (Status::Ok, n),
                        Err(status) => (status, 0),
                    }
                }
            },
        },
    };
    seal(reply, &[status as u64], payload_len)
}

/// Reads a reply frame into its status and payload.
pub fn decode_reply(bytes: &[u8]) -> Result<(Status, &[u8]), RequestError> {
    let frame = decode(bytes).map_err(RequestError::Frame)?;
    if frame.word_count() != 1 {
        return Err(RequestError::WordCount);
    }
    let status = frame
        .word(0)
        .and_then(Status::from_word)
        .ok_or(RequestError::OpcodeRange)?;
    Ok((status, frame.payload()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use tpt_k_ipc_cap::ObjectId;

    const KEY: [u8; 32] = [0x42; 32];
    const READ: u32 = 1;
    const ADD: u32 = 2;

    /// A counter that reads (needs RECV) and adds (needs SEND).
    struct Counter {
        value: u64,
    }

    impl Service for Counter {
        fn rights_for(&self, opcode: u32) -> Option<Rights> {
            match opcode {
                READ => Some(Rights::RECV),
                ADD => Some(Rights::SEND),
                _ => None,
            }
        }

        fn handle(
            &mut self,
            opcode: u32,
            _capability: Capability,
            args: &[u8],
            reply: &mut [u8],
        ) -> Result<usize, Status> {
            match opcode {
                READ => {
                    reply
                        .get_mut(..8)
                        .ok_or(Status::Full)?
                        .copy_from_slice(&self.value.to_le_bytes());
                    Ok(8)
                }
                ADD => {
                    let raw: [u8; 8] = args.try_into().map_err(|_| Status::Invalid)?;
                    self.value = self.value.wrapping_add(u64::from_le_bytes(raw));
                    Ok(0)
                }
                _ => Err(Status::UnknownOp),
            }
        }
    }

    fn token(rights: Rights) -> Token {
        Token::mint(
            &KEY,
            Capability {
                object: ObjectId(9),
                rights,
            },
        )
    }

    /// Sends one request and returns the decoded status and payload copy.
    fn call(
        counter: &mut Counter,
        key: &[u8; 32],
        opcode: u32,
        tok: &Token,
        args: &[u8],
    ) -> (Status, std::vec::Vec<u8>) {
        let mut req = [0u8; 256];
        let n = encode_request(&mut req, opcode, tok, args).unwrap();
        let mut reply = [0u8; 256];
        let m = serve(counter, key, &req[..n], &mut reply).unwrap();
        let (status, payload) = decode_reply(&reply[..m]).unwrap();
        (status, payload.to_vec())
    }

    #[test]
    fn request_round_trips_through_the_frame() {
        let tok = token(Rights::SEND);
        let mut buf = [0u8; 128];
        let n = encode_request(&mut buf, ADD, &tok, &[1, 2, 3]).unwrap();
        let req = decode_request(&buf[..n]).unwrap();
        assert_eq!(req.opcode, ADD);
        assert_eq!(req.args, &[1, 2, 3]);
        assert_eq!(req.token, tok);
        assert_eq!(req.token.verify(&KEY).unwrap().rights, Rights::SEND);
    }

    #[test]
    fn authorised_calls_reach_the_service() {
        let mut c = Counter { value: 0 };
        let adder = token(Rights::SEND.union(Rights::RECV));
        let (status, _) = call(&mut c, &KEY, ADD, &adder, &5u64.to_le_bytes());
        assert_eq!(status, Status::Ok);
        let (status, payload) = call(&mut c, &KEY, READ, &adder, &[]);
        assert_eq!(status, Status::Ok);
        assert_eq!(payload, 5u64.to_le_bytes());
    }

    #[test]
    fn missing_rights_are_denied_before_the_service_runs() {
        let mut c = Counter { value: 0 };
        let reader_only = token(Rights::RECV);
        let (status, _) = call(&mut c, &KEY, ADD, &reader_only, &5u64.to_le_bytes());
        assert_eq!(status, Status::Denied);
        assert_eq!(c.value, 0);
    }

    #[test]
    fn attenuated_token_carries_only_its_narrowed_rights() {
        let mut c = Counter { value: 0 };
        let full = token(Rights::SEND.union(Rights::RECV));
        let read_only = full.attenuate(&KEY, Rights::RECV).unwrap();
        let (status, _) = call(&mut c, &KEY, ADD, &read_only, &1u64.to_le_bytes());
        assert_eq!(status, Status::Denied);
        let (status, _) = call(&mut c, &KEY, READ, &read_only, &[]);
        assert_eq!(status, Status::Ok);
    }

    #[test]
    fn forged_token_is_denied() {
        let mut c = Counter { value: 0 };
        let mut wire = token(Rights::RECV).to_bytes();
        // Widen the rights in the token wire form without re-tagging.
        wire[8..12].copy_from_slice(&Rights::ALL.bits().to_le_bytes());
        let forged = Token::from_bytes(&wire).unwrap();
        let (status, _) = call(&mut c, &KEY, ADD, &forged, &1u64.to_le_bytes());
        assert_eq!(status, Status::Denied);
        assert_eq!(c.value, 0);
    }

    #[test]
    fn token_under_another_key_is_denied() {
        let mut c = Counter { value: 0 };
        let tok = token(Rights::ALL);
        let (status, _) = call(&mut c, &[0; 32], READ, &tok, &[]);
        assert_eq!(status, Status::Denied);
    }

    #[test]
    fn unknown_opcode_and_bad_arguments_are_reported() {
        let mut c = Counter { value: 0 };
        let tok = token(Rights::ALL);
        let (status, _) = call(&mut c, &KEY, 99, &tok, &[]);
        assert_eq!(status, Status::UnknownOp);
        let (status, _) = call(&mut c, &KEY, ADD, &tok, &[1, 2]);
        assert_eq!(status, Status::Invalid);
    }

    #[test]
    fn garbage_request_gets_a_bad_request_reply() {
        let mut c = Counter { value: 0 };
        let mut reply = [0u8; 64];
        let m = serve(&mut c, &KEY, &[0u8; 5], &mut reply).unwrap();
        let (status, payload) = decode_reply(&reply[..m]).unwrap();
        assert_eq!(status, Status::BadRequest);
        assert!(payload.is_empty());
    }

    #[test]
    fn small_reply_buffer_is_reported_as_full() {
        let mut c = Counter { value: 7 };
        let tok = token(Rights::RECV);
        let mut req = [0u8; 128];
        let n = encode_request(&mut req, READ, &tok, &[]).unwrap();
        // Room for the reply frame, but not for the 8-byte payload.
        let mut reply = [0u8; 24];
        let m = serve(&mut c, &KEY, &req[..n], &mut reply).unwrap();
        let (status, _) = decode_reply(&reply[..m]).unwrap();
        assert_eq!(status, Status::Full);
    }
}

#[cfg(test)]
mod invariants;
