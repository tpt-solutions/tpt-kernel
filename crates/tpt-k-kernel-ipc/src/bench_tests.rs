// SPDX-License-Identifier: MIT OR Apache-2.0
//
// Host timing of the endpoint and capability operations. These are the operations
// the kernel runs inside a syscall, without the entry, the page tables, or a
// context switch. They say what the data structures cost, not what a syscall costs
// on bare metal. The test is ignored by default:
//
//     cargo test -p tpt-k-kernel-ipc bench_ -- --ignored --nocapture

use crate::cap::CapTable;
use crate::endpoint::{Endpoint, RecvOutcome, SendOutcome};
use crate::{ObjectId, Rights};
extern crate std;

use std::{println, time::Instant};

const ROUNDS: u32 = 1_000_000;

#[test]
#[ignore = "timing run, see the module comment"]
fn bench_endpoint_send_then_receive() {
    let mut endpoint: Endpoint<8> = Endpoint::new();
    let message = [1, 2, 3, 4];
    let mut checksum = 0u64;
    let start = Instant::now();
    for _ in 0..ROUNDS {
        // A send with no receiver waiting queues the message, and the receive takes it.
        let sent = endpoint.send(0, message).expect("queue has room");
        assert_eq!(sent, SendOutcome::Blocked);
        match endpoint.receive(1).expect("receive queue has room") {
            RecvOutcome::Received { message, .. } => checksum = checksum.wrapping_add(message[0]),
            RecvOutcome::Blocked => panic!("a queued message was not taken"),
        }
    }
    let elapsed = start.elapsed();
    assert_eq!(checksum, u64::from(ROUNDS));
    println!(
        "endpoint send+receive: {:.1} ns per pair ({ROUNDS} pairs)",
        elapsed.as_nanos() as f64 / f64::from(ROUNDS)
    );
}

#[test]
#[ignore = "timing run, see the module comment"]
fn bench_capability_mint_derive_revoke() {
    let mut table: CapTable<8> = CapTable::new();
    let start = Instant::now();
    for _ in 0..ROUNDS {
        // Mint a root, derive a narrower child, then revoke the root. The revoke
        // removes the child too, so the table is empty again at the end of each round.
        let root = table.mint(ObjectId(1), Rights::ALL).expect("slot free");
        let child = table
            .derive(root, Rights::READ)
            .expect("derive needs grant");
        table.revoke(root).expect("root is live");
        assert!(table.revoke(child).is_err(), "the child went with the root");
    }
    let elapsed = start.elapsed();
    println!(
        "capability mint+derive+revoke: {:.1} ns per round ({ROUNDS} rounds)",
        elapsed.as_nanos() as f64 / f64::from(ROUNDS)
    );
}
