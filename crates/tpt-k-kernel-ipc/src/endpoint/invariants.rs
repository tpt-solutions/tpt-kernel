// SPDX-License-Identifier: MIT OR Apache-2.0
//! Bounded exhaustive checks of the endpoint invariants.
//!
//! Every sequence of up to [`DEPTH`] operations by [`THREADS`] threads on an
//! endpoint with queues of [`N`] is run, and the invariants are checked after
//! each step. A thread that is waiting in a queue does not act, as in the
//! kernel. The invariant IDs (IPC-n) are the ones in
//! `docs/assurance/invariants.md`.

use super::*;

const N: usize = 2;
const THREADS: usize = 3;
const DEPTH: usize = 6;

/// The endpoint and the ghost counters the invariants need. `seq` numbers the
/// messages, so each one is unique and its position in a queue is visible.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
struct World {
    ep: Endpoint<N>,
    seq: u64,
    sent: u64,
    received: u64,
    taken: u64,
}

#[derive(Clone, Copy, Debug)]
enum Op {
    Send(ThreadId),
    Receive(ThreadId),
    Take(ThreadId),
}

const OP_COUNT: usize = 3 * THREADS;

fn decode(k: usize) -> Op {
    let thread = k % THREADS;
    match k / THREADS {
        0 => Op::Send(thread),
        1 => Op::Receive(thread),
        _ => Op::Take(thread),
    }
}

fn waiting(ep: &Endpoint<N>, thread: ThreadId) -> bool {
    ep.senders.iter().flatten().any(|(t, _, _)| *t == thread)
        || ep.receivers.iter().flatten().any(|t| *t == thread)
}

/// Applies `op`. Returns `None` for an operation the kernel would not issue
/// (a waiting thread acting).
fn apply(world: &mut World, op: Op) -> Option<Result<(), EndpointError>> {
    match op {
        Op::Send(thread) => {
            if waiting(&world.ep, thread) {
                return None;
            }
            let head = world.ep.next_receiver();
            let message = [world.seq, thread as u64, 0, 0];
            match world.ep.send(thread, message) {
                Ok(outcome) => {
                    // IPC-5: a message is held only for the receiver at the head
                    // of the queue, and a send with no receiver blocks.
                    match outcome {
                        SendOutcome::Delivered { receiver } => {
                            assert_eq!(
                                Some(receiver),
                                head,
                                "IPC-5: delivered to a non-head receiver"
                            );
                        }
                        SendOutcome::Blocked => {
                            assert_eq!(head, None, "IPC-5: blocked although a receiver waits");
                        }
                    }
                    world.seq += 1;
                    world.sent += 1;
                    Some(Ok(()))
                }
                Err(e) => Some(Err(e)),
            }
        }
        Op::Receive(thread) => {
            if waiting(&world.ep, thread) {
                return None;
            }
            match world.ep.receive(thread) {
                Ok(RecvOutcome::Received {
                    message,
                    sender,
                    transfer,
                }) => {
                    // IPC-2: the message came from the sender it names, with no
                    // transfer, since this model sends none.
                    assert_eq!(
                        message[1], sender as u64,
                        "IPC-2: message from the wrong sender"
                    );
                    assert_eq!(transfer, None);
                    world.received += 1;
                    Some(Ok(()))
                }
                Ok(RecvOutcome::Blocked) => Some(Ok(())),
                Err(e) => Some(Err(e)),
            }
        }
        Op::Take(thread) => {
            // IPC-5: a thread takes a message exactly when one was held for it.
            // The held message names its sender, not its receiver, so the check
            // reads the receiver field of the held entry.
            let held_for_thread = world
                .ep
                .delivered
                .iter()
                .flatten()
                .any(|(to, _, _)| *to == thread);
            let taken = world.ep.take_delivery(thread);
            assert_eq!(
                taken.is_some(),
                held_for_thread,
                "IPC-5: take disagrees with delivery"
            );
            if taken.is_some() {
                world.taken += 1;
            }
            Some(Ok(()))
        }
    }
}

/// The invariants that must hold after every step.
fn check(world: &World) -> Result<(), &'static str> {
    let ep = &world.ep;
    // IPC-1: no message is lost or duplicated. Every message sent is either
    // still queued, held for a receiver, or has been received or taken, once.
    let queued = ep.senders.iter().flatten().count() as u64;
    let held = ep.delivered.iter().flatten().count() as u64;
    if world.sent != world.received + world.taken + queued + held {
        return Err("IPC-1: a message was lost or duplicated");
    }
    // IPC-3: queues are first-in, first-out and compact, so a used slot is never
    // after a free one, and the send queue is in send order.
    if !is_prefix(&ep.senders) || !is_prefix(&ep.receivers) {
        return Err("IPC-3: a queue has a gap");
    }
    if let [Some((_, first, _)), Some((_, second, _))] = ep.senders {
        if first[0] >= second[0] {
            return Err("IPC-3: send queue is out of send order");
        }
    }
    // IPC-4: a thread waits in at most one place.
    let mut seen = [false; THREADS];
    for thread in ep
        .senders
        .iter()
        .flatten()
        .map(|(t, _, _)| *t)
        .chain(ep.receivers.iter().flatten().copied())
    {
        if seen[thread] {
            return Err("IPC-4: a thread waits in two places");
        }
        seen[thread] = true;
    }
    Ok(())
}

fn is_prefix<T>(queue: &[Option<T>]) -> bool {
    let used = queue.iter().take_while(|s| s.is_some()).count();
    queue[used..].iter().all(Option::is_none)
}

fn explore(world: &World, depth: usize, steps: &mut u64) {
    if depth == DEPTH {
        return;
    }
    for k in 0..OP_COUNT {
        let op = decode(k);
        let mut next = *world;
        let Some(result) = apply(&mut next, op) else {
            continue;
        };
        *steps += 1;
        // IPC-6: a refused send or receive changes nothing.
        if let Err(EndpointError::Full) = result {
            assert_eq!(
                next.ep, world.ep,
                "IPC-6: refused {op:?} changed the endpoint"
            );
        }
        if let Err(message) = check(&next) {
            panic!("{message} after {op:?}");
        }
        explore(&next, depth + 1, steps);
    }
}

#[test]
fn bounded_exhaustive_endpoint_invariants() {
    let initial = World {
        ep: Endpoint::new(),
        seq: 1,
        sent: 0,
        received: 0,
        taken: 0,
    };
    let mut steps = 0;
    explore(&initial, 0, &mut steps);
    // Guard against a vacuous pass: the search must reach real states.
    assert!(steps > 10_000, "only {steps} steps explored");
}
