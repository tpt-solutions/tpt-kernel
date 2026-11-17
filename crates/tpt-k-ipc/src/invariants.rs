// SPDX-License-Identifier: MIT OR Apache-2.0
//! Bounded exhaustive check of the service gate (`SRV-1`): the service body runs
//! only when the token verifies and carries the rights the opcode needs.
//!
//! Every defined rights mask, every opcode in a small set (three offered, one
//! unknown), and both a correct and a wrong minting key are run through
//! [`serve`]. The expected status and the number of body calls are computed
//! independently of `serve`, and both are asserted.

use super::*;
use tpt_k_ipc_cap::{Capability, ObjectId, Rights, Token};

const KEY: [u8; 32] = [0x42; 32];
const OTHER_KEY: [u8; 32] = [0x43; 32];
const OPCODES: [u32; 4] = [1, 2, 3, 9];

/// The rights each offered opcode needs. Opcode 9 is not offered.
fn required(opcode: u32) -> Option<Rights> {
    match opcode {
        1 => Some(Rights::RECV),
        2 => Some(Rights::SEND),
        3 => Some(Rights::GRANT.union(Rights::MAP)),
        _ => None,
    }
}

/// A service that counts how often its body runs.
struct Probe {
    calls: u32,
}

impl Service for Probe {
    fn rights_for(&self, opcode: u32) -> Option<Rights> {
        required(opcode)
    }

    fn handle(
        &mut self,
        _opcode: u32,
        _capability: Capability,
        _args: &[u8],
        _reply: &mut [u8],
    ) -> Result<usize, Status> {
        self.calls += 1;
        Ok(0)
    }
}

#[test]
fn service_body_runs_exactly_when_token_is_valid_and_rights_allow() {
    let mut cases = 0u32;
    for bits in 0..=Rights::ALL.bits() {
        let rights = Rights::from_bits(bits);
        for opcode in OPCODES {
            for minting_key in [KEY, OTHER_KEY] {
                let token = Token::mint(
                    &minting_key,
                    Capability {
                        object: ObjectId(7),
                        rights,
                    },
                );
                let mut frame = [0u8; 256];
                let n = encode_request(&mut frame, opcode, &token, &[]).expect("request encodes");
                let mut service = Probe { calls: 0 };
                let mut reply = [0u8; 256];
                let m = serve(&mut service, &KEY, &frame[..n], &mut reply).expect("reply fits");
                let (status, _) = decode_reply(&reply[..m]).expect("reply decodes");

                // The expected outcome, from the rules alone.
                let expected = if minting_key != KEY {
                    Status::Denied
                } else if let Some(needed) = required(opcode) {
                    if rights.contains(needed) {
                        Status::Ok
                    } else {
                        Status::Denied
                    }
                } else {
                    Status::UnknownOp
                };

                assert_eq!(
                    status,
                    expected,
                    "rights {bits:#x}, opcode {opcode}, key ok {}",
                    minting_key == KEY
                );
                assert_eq!(
                    service.calls,
                    u32::from(expected == Status::Ok),
                    "body calls for rights {bits:#x}, opcode {opcode}"
                );
                cases += 1;
            }
        }
    }
    // 64 masks, 4 opcodes and 2 keys: 512 cases. Guard against a silent shrink.
    assert_eq!(cases, 512);
}
