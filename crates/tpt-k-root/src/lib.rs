// SPDX-License-Identifier: MIT OR Apache-2.0
//! tpt-k root server: the capability service.
//!
//! [`CapService`] runs the root's capability table behind the `tpt-k-ipc`
//! request framework. Callers present a [`Token`] and the service checks it
//! before any table operation:
//!
//! * `MINT` creates a root capability for an object (needs `MINT`, not just `GRANT`).
//! * `DELEGATE` copies the presented capability with fewer rights (needs `GRANT`).
//! * `REVOKE` kills the presented capability and its descendants (needs `REVOKE`).
//! * `ATTENUATE` returns a narrower token for the same capability (no rights needed).
//!
//! A token's object field is a service-assigned id, never reused, which names a
//! table handle. The real object id lives in the table. The service does not
//! touch the kernel and needs no transport.

#![no_std]

#[cfg(test)]
extern crate std;

use tpt_k_ipc::{Service, Status};
use tpt_k_ipc_cap::{Capability, ObjectId, Rights, Token, TOKEN_LEN};
use tpt_k_root_caps::{CapError, CapTable, Handle};

/// Opcode: create a root capability for an object. Args: object id (u64 LE), rights (u32 LE).
pub const OP_MINT: u32 = 1;
/// Opcode: delegate the presented capability. Args: rights (u32 LE).
pub const OP_DELEGATE: u32 = 2;
/// Opcode: revoke the presented capability and its descendants. Args: none.
pub const OP_REVOKE: u32 = 3;
/// Opcode: narrow the presented capability. Args: rights (u32 LE).
pub const OP_ATTENUATE: u32 = 4;

/// The root's capability service: a table of `N` slots and `M` token entries.
#[derive(Debug)]
pub struct CapService<const N: usize, const M: usize> {
    key: [u8; 32],
    table: CapTable<N>,
    entries: [Option<(u64, Handle)>; M],
    next_id: u64,
}

impl<const N: usize, const M: usize> CapService<N, M> {
    /// An empty service that signs tokens with `key`.
    #[must_use]
    pub fn new(key: [u8; 32]) -> Self {
        Self {
            key,
            table: CapTable::new(),
            entries: [None; M],
            next_id: 1,
        }
    }

    /// The key tokens are signed with. Callers pass it to `tpt_k_ipc::serve`.
    #[must_use]
    pub fn key(&self) -> &[u8; 32] {
        &self.key
    }

    /// Creates a root capability and returns a token for it. This is the root's
    /// own authority, used at boot to hand out the first token.
    pub fn issue_root(&mut self, object: ObjectId, rights: Rights) -> Result<Token, CapError> {
        let slot = self.free_entry().ok_or(CapError::Full)?;
        let handle = self.table.mint(object, rights)?;
        self.install(slot, handle)
    }

    /// The first entry that is empty or names a dead handle.
    fn free_entry(&self) -> Option<usize> {
        self.entries.iter().position(|e| match e {
            None => true,
            Some((_, h)) => self.table.lookup(*h).is_err(),
        })
    }

    /// Records `handle` under a fresh id in `slot`, and returns its token.
    fn install(&mut self, slot: usize, handle: Handle) -> Result<Token, CapError> {
        let rights = self.table.lookup(handle)?.rights;
        let id = self.next_id;
        self.next_id += 1;
        self.entries[slot] = Some((id, handle));
        Ok(Token::mint(
            &self.key,
            Capability {
                object: ObjectId(id),
                rights,
            },
        ))
    }

    /// The table handle behind a token id.
    fn find(&self, id: u64) -> Result<Handle, CapError> {
        self.entries
            .iter()
            .flatten()
            .find(|(eid, _)| *eid == id)
            .map(|(_, h)| *h)
            .ok_or(CapError::Stale)
    }
}

/// Maps a table error to the status a caller sees. A full table is distinct
/// from a refused request.
fn status_of(e: CapError) -> Status {
    match e {
        CapError::Full => Status::Full,
        CapError::Stale | CapError::NoRight | CapError::Widen => Status::Denied,
    }
}

/// Writes a token into the reply buffer and returns its length.
fn write_token(reply: &mut [u8], token: &Token) -> Result<usize, Status> {
    reply
        .get_mut(..TOKEN_LEN)
        .ok_or(Status::Full)?
        .copy_from_slice(&token.to_bytes());
    Ok(TOKEN_LEN)
}

impl<const N: usize, const M: usize> Service for CapService<N, M> {
    fn rights_for(&self, opcode: u32) -> Option<Rights> {
        match opcode {
            OP_MINT => Some(Rights::MINT),
            OP_DELEGATE => Some(Rights::GRANT),
            OP_REVOKE => Some(Rights::REVOKE),
            OP_ATTENUATE => Some(Rights::NONE),
            _ => None,
        }
    }

    fn handle(
        &mut self,
        opcode: u32,
        capability: Capability,
        args: &[u8],
        reply: &mut [u8],
    ) -> Result<usize, Status> {
        match opcode {
            OP_MINT => {
                let a: [u8; 12] = args.try_into().map_err(|_| Status::Invalid)?;
                let object = ObjectId(u64::from_le_bytes([
                    a[0], a[1], a[2], a[3], a[4], a[5], a[6], a[7],
                ]));
                let rights = Rights::from_bits(u32::from_le_bytes([a[8], a[9], a[10], a[11]]));
                let slot = self.free_entry().ok_or(Status::Full)?;
                let handle = self.table.mint(object, rights).map_err(status_of)?;
                let token = self.install(slot, handle).map_err(status_of)?;
                write_token(reply, &token)
            }
            OP_DELEGATE => {
                let a: [u8; 4] = args.try_into().map_err(|_| Status::Invalid)?;
                let rights = Rights::from_bits(u32::from_le_bytes(a));
                // The copy may not carry more than the presented token does.
                if !capability.allows(rights) {
                    return Err(Status::Denied);
                }
                let from = self.find(capability.object.0).map_err(status_of)?;
                let slot = self.free_entry().ok_or(Status::Full)?;
                let handle = self.table.delegate(from, rights).map_err(status_of)?;
                let token = self.install(slot, handle).map_err(status_of)?;
                write_token(reply, &token)
            }
            OP_REVOKE => {
                if !args.is_empty() {
                    return Err(Status::Invalid);
                }
                let from = self.find(capability.object.0).map_err(status_of)?;
                self.table.revoke(from).map_err(status_of)?;
                Ok(0)
            }
            OP_ATTENUATE => {
                let a: [u8; 4] = args.try_into().map_err(|_| Status::Invalid)?;
                let rights = Rights::from_bits(u32::from_le_bytes(a));
                let narrowed = capability.attenuate(rights).ok_or(Status::Denied)?;
                // A dead capability cannot be narrowed.
                let from = self.find(capability.object.0).map_err(status_of)?;
                self.table.lookup(from).map_err(status_of)?;
                let token = Token::mint(&self.key, narrowed);
                write_token(reply, &token)
            }
            _ => Err(Status::UnknownOp),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::vec::Vec;
    use tpt_k_ipc::{decode_reply, encode_request, serve};

    const KEY: [u8; 32] = [0x5A; 32];

    fn service() -> CapService<8, 8> {
        CapService::new(KEY)
    }

    /// Sends one request through `serve` and returns the status and payload.
    fn call(
        s: &mut CapService<8, 8>,
        opcode: u32,
        token: &Token,
        args: &[u8],
    ) -> (Status, Vec<u8>) {
        let key = *s.key();
        let mut req = [0u8; 256];
        let n = encode_request(&mut req, opcode, token, args).unwrap();
        let mut reply = [0u8; 128];
        let m = serve(s, &key, &req[..n], &mut reply).unwrap();
        let (status, payload) = decode_reply(&reply[..m]).unwrap();
        (status, payload.to_vec())
    }

    fn token_from(payload: &[u8]) -> Token {
        Token::from_bytes(payload).unwrap()
    }

    fn u32_args(v: u32) -> [u8; 4] {
        v.to_le_bytes()
    }

    fn mint_args(object: u64, rights: u32) -> Vec<u8> {
        let mut a = Vec::new();
        a.extend_from_slice(&object.to_le_bytes());
        a.extend_from_slice(&rights.to_le_bytes());
        a
    }

    #[test]
    fn mint_returns_a_verifying_token() {
        let mut s = service();
        let root = s
            .issue_root(
                ObjectId(1),
                Rights::GRANT.union(Rights::SEND).union(Rights::MINT),
            )
            .unwrap();
        let (status, payload) = call(
            &mut s,
            OP_MINT,
            &root,
            &mint_args(42, Rights::RECV.union(Rights::MAP).bits()),
        );
        assert_eq!(status, Status::Ok);
        let cap = token_from(&payload).verify(&KEY).unwrap();
        assert_eq!(cap.rights, Rights::RECV.union(Rights::MAP));
        // The token names a service entry, not the caller's object.
        assert_ne!(cap.object, ObjectId(42));
    }

    #[test]
    fn mint_without_mint_right_is_denied_even_with_grant() {
        // GRANT alone must not let a holder create arbitrary root capabilities.
        let mut s = service();
        let weak = s
            .issue_root(ObjectId(1), Rights::GRANT.union(Rights::SEND))
            .unwrap();
        let (status, _) = call(&mut s, OP_MINT, &weak, &mint_args(42, Rights::RECV.bits()));
        assert_eq!(status, Status::Denied);
    }

    #[test]
    fn delegate_narrows_and_refuses_widening() {
        let mut s = service();
        let root = s
            .issue_root(ObjectId(1), Rights::GRANT.union(Rights::SEND))
            .unwrap();
        let (status, payload) = call(&mut s, OP_DELEGATE, &root, &u32_args(Rights::SEND.bits()));
        assert_eq!(status, Status::Ok);
        let child = token_from(&payload);
        assert_eq!(child.verify(&KEY).unwrap().rights, Rights::SEND);

        // Asking for a right the presented token lacks is refused.
        let (status, _) = call(&mut s, OP_DELEGATE, &root, &u32_args(Rights::RECV.bits()));
        assert_eq!(status, Status::Denied);

        // The child has no GRANT, so it cannot delegate further.
        let (status, _) = call(&mut s, OP_DELEGATE, &child, &u32_args(Rights::SEND.bits()));
        assert_eq!(status, Status::Denied);
    }

    #[test]
    fn revoke_invalidates_delegated_capabilities() {
        let mut s = service();
        let root = s
            .issue_root(
                ObjectId(1),
                Rights::GRANT.union(Rights::REVOKE).union(Rights::SEND),
            )
            .unwrap();
        let (_, payload) = call(&mut s, OP_DELEGATE, &root, &u32_args(Rights::SEND.bits()));
        let child = token_from(&payload);

        let (status, payload) = call(&mut s, OP_REVOKE, &root, &[]);
        assert_eq!(status, Status::Ok);
        assert!(payload.is_empty());

        let (status, _) = call(&mut s, OP_ATTENUATE, &child, &u32_args(0));
        assert_eq!(status, Status::Denied);
        let (status, _) = call(&mut s, OP_ATTENUATE, &root, &u32_args(0));
        assert_eq!(status, Status::Denied);
    }

    #[test]
    fn revoke_needs_the_revoke_right() {
        let mut s = service();
        let root = s
            .issue_root(ObjectId(1), Rights::GRANT.union(Rights::SEND))
            .unwrap();
        let (status, _) = call(&mut s, OP_REVOKE, &root, &[]);
        assert_eq!(status, Status::Denied);
    }

    #[test]
    fn attenuate_yields_a_narrower_verifying_token() {
        let mut s = service();
        let root = s.issue_root(ObjectId(1), Rights::ALL).unwrap();
        let (status, payload) = call(&mut s, OP_ATTENUATE, &root, &u32_args(Rights::SEND.bits()));
        assert_eq!(status, Status::Ok);
        let narrow = token_from(&payload);
        assert_eq!(narrow.verify(&KEY).unwrap().rights, Rights::SEND);

        // The narrowed token cannot be widened again.
        let (status, _) = call(
            &mut s,
            OP_ATTENUATE,
            &narrow,
            &u32_args(Rights::RECV.bits()),
        );
        assert_eq!(status, Status::Denied);
    }

    #[test]
    fn token_under_another_key_is_denied() {
        let mut s = service();
        let root = s.issue_root(ObjectId(1), Rights::ALL).unwrap();
        let key = [0u8; 32];
        let mut req = [0u8; 256];
        let n = encode_request(&mut req, OP_ATTENUATE, &root, &u32_args(0)).unwrap();
        let mut reply = [0u8; 128];
        let m = serve(&mut s, &key, &req[..n], &mut reply).unwrap();
        let (status, _) = decode_reply(&reply[..m]).unwrap();
        assert_eq!(status, Status::Denied);
    }

    #[test]
    fn bad_argument_lengths_are_invalid() {
        let mut s = service();
        let root = s
            .issue_root(
                ObjectId(1),
                Rights::GRANT.union(Rights::REVOKE).union(Rights::MINT),
            )
            .unwrap();
        let (status, _) = call(&mut s, OP_MINT, &root, &[0u8; 11]);
        assert_eq!(status, Status::Invalid);
        let (status, _) = call(&mut s, OP_DELEGATE, &root, &[0u8; 3]);
        assert_eq!(status, Status::Invalid);
        let (status, _) = call(&mut s, OP_ATTENUATE, &root, &[]);
        assert_eq!(status, Status::Invalid);
        let (status, _) = call(&mut s, OP_REVOKE, &root, &[0u8; 1]);
        assert_eq!(status, Status::Invalid);
    }

    #[test]
    fn unknown_opcode_is_reported() {
        let mut s = service();
        let root = s.issue_root(ObjectId(1), Rights::ALL).unwrap();
        let (status, _) = call(&mut s, 99, &root, &[]);
        assert_eq!(status, Status::UnknownOp);
    }

    #[test]
    fn full_token_table_reports_full() {
        // Two entries: the root uses one, so one more mint fits and the next is refused.
        let mut s: CapService<8, 2> = CapService::new(KEY);
        let root = s.issue_root(ObjectId(1), Rights::MINT).unwrap();
        let key = *s.key();
        let mint = |s: &mut CapService<8, 2>, object: u64| {
            let mut req = [0u8; 256];
            let args = mint_args(object, Rights::SEND.bits());
            let n = encode_request(&mut req, OP_MINT, &root, &args).unwrap();
            let mut reply = [0u8; 128];
            let m = serve(s, &key, &req[..n], &mut reply).unwrap();
            decode_reply(&reply[..m]).unwrap().0
        };
        assert_eq!(mint(&mut s, 2), Status::Ok);
        assert_eq!(mint(&mut s, 3), Status::Full);
    }
}
