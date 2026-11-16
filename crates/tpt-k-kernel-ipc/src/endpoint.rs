// SPDX-License-Identifier: MIT OR Apache-2.0
//! Synchronous endpoints. A message moves when a sender and a receiver are both
//! present. A party that arrives with no partner waits in a queue, and the next
//! arrival on the other side completes the transfer and wakes it.
//!
//! Queues are first-in, first-out and bounded by `N`, so an endpoint needs no
//! heap. This module only decides what happens. The kernel blocks and wakes
//! threads according to the outcome.

/// Words in one message.
pub const MESSAGE_WORDS: usize = 4;

/// A message: a few words copied from sender to receiver.
pub type Message = [u64; MESSAGE_WORDS];

/// A thread, named by the kernel's thread slot.
pub type ThreadId = usize;

/// What a send did.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SendOutcome {
    /// The message is held for `receiver`, which was waiting. Wake it, and it
    /// takes the message with [`Endpoint::take_delivery`].
    Delivered {
        /// The receiver the message is for.
        receiver: ThreadId,
    },
    /// No receiver was waiting. The sender is queued and must block.
    Blocked,
}

/// What a receive did.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RecvOutcome {
    /// A sender was waiting. Wake `sender`, and use `message`.
    Received {
        /// The message, now taken from the sender.
        message: Message,
        /// The sender to wake.
        sender: ThreadId,
    },
    /// No sender was waiting. The receiver is queued and must block.
    Blocked,
}

/// Why an endpoint refused an operation.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EndpointError {
    /// The queue on that side is full.
    Full,
}

/// One endpoint, with room for `N` waiting senders, `N` waiting receivers, and
/// `N` messages held for receivers that have not yet taken them.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Endpoint<const N: usize> {
    senders: [Option<(ThreadId, Message)>; N],
    receivers: [Option<ThreadId>; N],
    delivered: [Option<(ThreadId, Message)>; N],
}

impl<const N: usize> Default for Endpoint<N> {
    fn default() -> Self {
        Self::new()
    }
}

impl<const N: usize> Endpoint<N> {
    /// An endpoint with empty queues.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            senders: [None; N],
            receivers: [None; N],
            delivered: [None; N],
        }
    }

    /// Sends `message` from `sender`. If a receiver is waiting, the message is
    /// held for it. Otherwise the sender joins the send queue.
    ///
    /// # Errors
    ///
    /// [`EndpointError::Full`] if no receiver is waiting and the send queue is
    /// full.
    pub fn send(
        &mut self,
        sender: ThreadId,
        message: Message,
    ) -> Result<SendOutcome, EndpointError> {
        if let Some(receiver) = pop_front(&mut self.receivers) {
            push_back(&mut self.delivered, (receiver, message))?;
            Ok(SendOutcome::Delivered { receiver })
        } else {
            push_back(&mut self.senders, (sender, message))?;
            Ok(SendOutcome::Blocked)
        }
    }

    /// Receives on behalf of `receiver`. If a sender is waiting, its message is
    /// taken. Otherwise the receiver joins the receive queue.
    ///
    /// # Errors
    ///
    /// [`EndpointError::Full`] if no sender is waiting and the receive queue is
    /// full.
    pub fn receive(&mut self, receiver: ThreadId) -> Result<RecvOutcome, EndpointError> {
        if let Some((sender, message)) = pop_front(&mut self.senders) {
            Ok(RecvOutcome::Received { message, sender })
        } else {
            push_back(&mut self.receivers, receiver)?;
            Ok(RecvOutcome::Blocked)
        }
    }

    /// Takes the message held for `receiver`, if one was delivered to it.
    pub fn take_delivery(&mut self, receiver: ThreadId) -> Option<Message> {
        let index = self
            .delivered
            .iter()
            .position(|slot| matches!(slot, Some((to, _)) if *to == receiver))?;
        self.delivered[index].take().map(|(_, message)| message)
    }

    /// Number of senders waiting.
    #[must_use]
    pub fn waiting_senders(&self) -> usize {
        self.senders.iter().flatten().count()
    }

    /// Number of receivers waiting.
    #[must_use]
    pub fn waiting_receivers(&self) -> usize {
        self.receivers.iter().flatten().count()
    }
}

/// Removes the first entry of a FIFO queue. Queues stay compact: entries are
/// appended at the first gap and removed from the front, so the used slots are
/// always a prefix.
fn pop_front<T: Copy>(queue: &mut [Option<T>]) -> Option<T> {
    let first = queue.first().copied().flatten()?;
    let len = queue.len();
    queue.copy_within(1..len, 0);
    queue[len - 1] = None;
    Some(first)
}

/// Appends to a FIFO queue, or fails if it is full.
fn push_back<T: Copy>(queue: &mut [Option<T>], value: T) -> Result<(), EndpointError> {
    let slot = queue
        .iter_mut()
        .find(|slot| slot.is_none())
        .ok_or(EndpointError::Full)?;
    *slot = Some(value);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    const HELLO: Message = [1, 2, 3, 4];

    #[test]
    fn sender_first_then_receiver_completes_the_transfer() {
        let mut ep: Endpoint<2> = Endpoint::new();
        assert_eq!(ep.send(3, HELLO), Ok(SendOutcome::Blocked));
        assert_eq!(
            ep.receive(5),
            Ok(RecvOutcome::Received {
                message: HELLO,
                sender: 3
            })
        );
        // Nothing is waiting now, so a receive queues.
        assert_eq!(ep.receive(5), Ok(RecvOutcome::Blocked));
    }

    #[test]
    fn receiver_first_then_sender_holds_the_message_for_it() {
        let mut ep: Endpoint<2> = Endpoint::new();
        assert_eq!(ep.receive(5), Ok(RecvOutcome::Blocked));
        assert_eq!(ep.take_delivery(5), None);
        assert_eq!(
            ep.send(3, HELLO),
            Ok(SendOutcome::Delivered { receiver: 5 })
        );
        assert_eq!(ep.take_delivery(5), Some(HELLO));
        assert_eq!(ep.take_delivery(5), None);
    }

    #[test]
    fn several_senders_queue_in_fifo_order() {
        let mut ep: Endpoint<4> = Endpoint::new();
        assert_eq!(ep.send(1, [1; 4]), Ok(SendOutcome::Blocked));
        assert_eq!(ep.send(2, [2; 4]), Ok(SendOutcome::Blocked));
        assert_eq!(ep.waiting_senders(), 2);
        assert_eq!(
            ep.receive(9),
            Ok(RecvOutcome::Received {
                message: [1; 4],
                sender: 1
            })
        );
        assert_eq!(
            ep.receive(9),
            Ok(RecvOutcome::Received {
                message: [2; 4],
                sender: 2
            })
        );
        assert_eq!(ep.waiting_senders(), 0);
    }

    #[test]
    fn several_receivers_each_get_their_own_message() {
        let mut ep: Endpoint<4> = Endpoint::new();
        assert_eq!(ep.receive(7), Ok(RecvOutcome::Blocked));
        assert_eq!(ep.receive(8), Ok(RecvOutcome::Blocked));
        assert_eq!(ep.waiting_receivers(), 2);
        assert_eq!(
            ep.send(1, [10; 4]),
            Ok(SendOutcome::Delivered { receiver: 7 })
        );
        assert_eq!(
            ep.send(2, [20; 4]),
            Ok(SendOutcome::Delivered { receiver: 8 })
        );
        assert_eq!(ep.take_delivery(8), Some([20; 4]));
        assert_eq!(ep.take_delivery(7), Some([10; 4]));
    }

    #[test]
    fn full_queues_refuse_instead_of_blocking() {
        let mut ep: Endpoint<1> = Endpoint::new();
        ep.send(1, HELLO).unwrap();
        assert_eq!(ep.send(2, HELLO), Err(EndpointError::Full));
        let mut ep: Endpoint<1> = Endpoint::new();
        ep.receive(1).unwrap();
        assert_eq!(ep.receive(2), Err(EndpointError::Full));
    }
}
