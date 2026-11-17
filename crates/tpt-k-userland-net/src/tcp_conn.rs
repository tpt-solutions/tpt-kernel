// SPDX-License-Identifier: MIT OR Apache-2.0
//! TCP connection state machine (RFC 793), as pure logic.
//!
//! A [`Connection`] takes parsed segments in and produces segment bytes out. It
//! does no I/O and reads no clock: the caller passes the current tick to
//! [`Connection::input`] and [`Connection::tick`]. The IP layer carries the
//! bytes that [`Outbox`] holds. Each connection serves one peer. A passive
//! connection takes the first SYN it sees and ignores every other peer.
//!
//! Limits, stated plainly:
//! - No retransmission timer. A segment that is lost is never sent again, so a
//!   connection that loses a segment stalls.
//! - No congestion control. The send window is the peer's advertised window.
//! - No TCP options: no MSS negotiation (the segment size is fixed at [`MSS`]),
//!   no window scaling, no SACK and no timestamps.
//! - Data that arrives out of order is dropped and acknowledged as a duplicate.
//!   Data that does not fit the receive buffer is only partly kept.
//! - Data that arrives with a SYN or SYN-ACK is ignored.
//! - No zero-window probes and no window-update segments. The window reopens
//!   in the acknowledgements that follow the next segment we send.
//! - A passive connection accepts one peer. Listening for more needs more
//!   connections.

use crate::tcp::{self, Header, ACK, FIN, PSH, RST, SYN};

/// Largest payload this stack sends in one segment, in bytes. There are no
/// options, so the segment size is fixed.
pub const MSS: usize = 512;
/// Segments one [`Outbox`] holds.
pub const OUTBOX_CAP: usize = 8;
/// Ticks in the maximum segment lifetime. TimeWait lasts twice this.
pub const MSL_TICKS: u64 = 30;

/// One end of a connection: an IPv4 address and a TCP port.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Endpoint {
    /// IPv4 address.
    pub ip: [u8; 4],
    /// TCP port.
    pub port: u16,
}

/// The RFC 793 states.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum State {
    /// No connection.
    Closed,
    /// Waiting for a SYN.
    Listen,
    /// SYN sent, waiting for a SYN-ACK.
    SynSent,
    /// SYN received and SYN-ACK sent, waiting for the final ACK.
    SynReceived,
    /// Data may flow both ways.
    Established,
    /// Our FIN is sent and not yet acknowledged.
    FinWait1,
    /// Our FIN is acknowledged, waiting for the peer's FIN.
    FinWait2,
    /// The peer's FIN arrived; we may still send.
    CloseWait,
    /// Both FINs crossed on the way; waiting for our FIN's acknowledgement.
    Closing,
    /// We sent our FIN after the peer's; waiting for its acknowledgement.
    LastAck,
    /// Waiting out two segment lifetimes before the connection is forgotten.
    TimeWait,
}

/// The outbox has no room for another segment.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct OutboxFull;

#[derive(Clone, Copy)]
struct Segment {
    len: usize,
    bytes: [u8; tcp::HEADER_LEN + MSS],
}

/// Segments waiting to be sent, held in a fixed array with no heap.
pub struct Outbox {
    segments: [Segment; OUTBOX_CAP],
    count: usize,
}

impl Outbox {
    /// An empty outbox.
    #[must_use]
    pub fn new() -> Self {
        Self {
            segments: [Segment {
                len: 0,
                bytes: [0; tcp::HEADER_LEN + MSS],
            }; OUTBOX_CAP],
            count: 0,
        }
    }

    /// Number of segments held.
    #[must_use]
    pub fn len(&self) -> usize {
        self.count
    }

    /// True when no segment is held.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.count == 0
    }

    /// True when no more segments fit.
    #[must_use]
    pub fn is_full(&self) -> bool {
        self.count == OUTBOX_CAP
    }

    /// Removes every segment.
    pub fn clear(&mut self) {
        self.count = 0;
    }

    /// The TCP segment bytes in send order, each with its header and checksum.
    pub fn iter(&self) -> impl Iterator<Item = &[u8]> + '_ {
        self.segments[..self.count]
            .iter()
            .map(|s| &s.bytes[..s.len])
    }

    fn push(
        &mut self,
        src_ip: [u8; 4],
        dst_ip: [u8; 4],
        header: &Header,
        payload: &[u8],
    ) -> Result<(), OutboxFull> {
        if self.is_full() {
            return Err(OutboxFull);
        }
        let seg = &mut self.segments[self.count];
        seg.len =
            tcp::write(&mut seg.bytes, src_ip, dst_ip, header, payload).map_err(|_| OutboxFull)?;
        self.count += 1;
        Ok(())
    }
}

impl Default for Outbox {
    fn default() -> Self {
        Self::new()
    }
}

/// A control reply: sequence and acknowledgement numbers and flags.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Reply {
    seq: u32,
    ack: u32,
    flags: u8,
}

/// `a` is before `b` in sequence space (modulo 2^32).
fn seq_lt(a: u32, b: u32) -> bool {
    (a.wrapping_sub(b) as i32) < 0
}

/// `a` is before or equal to `b` in sequence space.
fn seq_le(a: u32, b: u32) -> bool {
    a == b || seq_lt(a, b)
}

/// Sequence space used by a segment: its payload, plus one for SYN and one for FIN.
fn seg_len(h: &Header, payload_len: usize) -> u32 {
    let data = u32::try_from(payload_len).unwrap_or(u32::MAX / 2);
    data.wrapping_add(u32::from(h.flags & SYN != 0))
        .wrapping_add(u32::from(h.flags & FIN != 0))
}

/// The reply to a segment that reaches no connection (RFC 793, "Reset generation").
fn reset_reply(h: &Header, seg_len: u32) -> Option<Reply> {
    if h.flags & RST != 0 {
        None
    } else if h.flags & ACK != 0 {
        Some(Reply {
            seq: h.ack,
            ack: 0,
            flags: RST,
        })
    } else {
        Some(Reply {
            seq: 0,
            ack: h.seq.wrapping_add(seg_len),
            flags: RST | ACK,
        })
    }
}

/// One TCP connection with `N` bytes of send and `N` bytes of receive buffer.
pub struct Connection<const N: usize> {
    local: Endpoint,
    remote: Option<Endpoint>,
    state: State,
    iss: u32,
    snd_una: u32,
    snd_nxt: u32,
    snd_wnd: u32,
    rcv_nxt: u32,
    /// Bytes from `snd_una` onward: sent but unacknowledged, then unsent.
    tx: [u8; N],
    tx_len: usize,
    rx: [u8; N],
    rx_len: usize,
    fin_queued: bool,
    fin_sent: bool,
    tw_since: u64,
}

impl<const N: usize> Connection<N> {
    /// A closed connection bound to `local`, with initial send sequence `iss`.
    #[must_use]
    pub fn new(local: Endpoint, iss: u32) -> Self {
        Self {
            local,
            remote: None,
            state: State::Closed,
            iss,
            snd_una: iss,
            snd_nxt: iss,
            snd_wnd: 0,
            rcv_nxt: 0,
            tx: [0; N],
            tx_len: 0,
            rx: [0; N],
            rx_len: 0,
            fin_queued: false,
            fin_sent: false,
            tw_since: 0,
        }
    }

    /// The current state.
    #[must_use]
    pub fn state(&self) -> State {
        self.state
    }

    /// The local endpoint.
    #[must_use]
    pub fn local(&self) -> Endpoint {
        self.local
    }

    /// The peer, once a connection or a SYN names one.
    #[must_use]
    pub fn remote(&self) -> Option<Endpoint> {
        self.remote
    }

    /// Bytes queued for sending that the peer has not yet acknowledged.
    #[must_use]
    pub fn tx_pending(&self) -> usize {
        self.tx_len
    }

    /// Bytes received and waiting for [`Connection::recv`].
    #[must_use]
    pub fn rx_available(&self) -> usize {
        self.rx_len
    }

    /// Starts a passive open. Returns false unless the connection is closed.
    pub fn listen(&mut self) -> bool {
        if self.state == State::Closed {
            self.state = State::Listen;
            true
        } else {
            false
        }
    }

    /// Starts an active open to `to` by queueing a SYN. Returns false unless
    /// the connection is closed, or the outbox has no room.
    pub fn connect(&mut self, to: Endpoint, out: &mut Outbox) -> bool {
        if self.state != State::Closed || out.is_full() {
            return false;
        }
        self.remote = Some(to);
        self.snd_una = self.iss;
        self.snd_nxt = self.iss.wrapping_add(1);
        self.state = State::SynSent;
        self.emit(out, to, self.iss, 0, SYN, &[]);
        true
    }

    /// Queues up to the free space of the send buffer. Returns how many bytes
    /// were taken. Nothing is taken unless the connection can still send.
    pub fn send(&mut self, data: &[u8]) -> usize {
        if !matches!(self.state, State::Established | State::CloseWait) || self.fin_queued {
            return 0;
        }
        let n = data.len().min(N - self.tx_len);
        self.tx[self.tx_len..self.tx_len + n].copy_from_slice(&data[..n]);
        self.tx_len += n;
        n
    }

    /// Copies received bytes into `buf` and frees that space. Returns the count.
    pub fn recv(&mut self, buf: &mut [u8]) -> usize {
        let n = buf.len().min(self.rx_len);
        buf[..n].copy_from_slice(&self.rx[..n]);
        self.rx.copy_within(n..self.rx_len, 0);
        self.rx_len -= n;
        n
    }

    /// Asks to close after the queued data. Sends whatever the window allows.
    pub fn close(&mut self, out: &mut Outbox) {
        match self.state {
            State::Listen | State::SynSent => self.state = State::Closed,
            State::Established => {
                self.fin_queued = true;
                self.state = State::FinWait1;
            }
            State::SynReceived => {
                // Our FIN follows the handshake; the ACK that completes it moves
                // the state to FinWait1.
                self.fin_queued = true;
            }
            State::CloseWait => {
                self.fin_queued = true;
                self.state = State::LastAck;
            }
            _ => {}
        }
        self.transmit(out);
    }

    /// Sends queued data and a FIN, as far as the peer's window allows.
    pub fn poll(&mut self, out: &mut Outbox) {
        self.transmit(out);
    }

    /// Ends TimeWait once two segment lifetimes have passed since it began.
    pub fn tick(&mut self, now: u64) {
        if self.state == State::TimeWait && now.saturating_sub(self.tw_since) >= 2 * MSL_TICKS {
            self.state = State::Closed;
        }
    }

    /// Processes one parsed segment from `from`, and queues the replies in `out`.
    ///
    /// Returns [`OutboxFull`] without touching the state when `out` has no room.
    /// The caller clears or drains the outbox and gives the segment again.
    pub fn input(
        &mut self,
        now: u64,
        from: Endpoint,
        header: &Header,
        payload: &[u8],
        out: &mut Outbox,
    ) -> Result<(), OutboxFull> {
        if out.is_full() {
            return Err(OutboxFull);
        }
        if self.state != State::Closed && self.remote.is_some_and(|r| r != from) {
            return Ok(());
        }
        let len = seg_len(header, payload.len());
        let reply = match self.state {
            State::Closed => reset_reply(header, len),
            State::Listen => self.listen_input(from, header, len),
            State::SynSent => self.syn_sent_input(header, len),
            _ => self.sync_input(now, header, payload, len),
        };
        if let Some(r) = reply {
            self.emit(out, from, r.seq, r.ack, r.flags, &[]);
        }
        self.transmit(out);
        Ok(())
    }

    fn listen_input(&mut self, from: Endpoint, h: &Header, len: u32) -> Option<Reply> {
        if h.flags & RST != 0 {
            return None;
        }
        if h.flags & ACK != 0 {
            return reset_reply(h, len);
        }
        if h.flags & SYN == 0 {
            return None;
        }
        self.remote = Some(from);
        self.rcv_nxt = h.seq.wrapping_add(1);
        self.snd_wnd = u32::from(h.window);
        self.snd_una = self.iss;
        self.snd_nxt = self.iss.wrapping_add(1);
        self.state = State::SynReceived;
        Some(Reply {
            seq: self.iss,
            ack: self.rcv_nxt,
            flags: SYN | ACK,
        })
    }

    fn syn_sent_input(&mut self, h: &Header, len: u32) -> Option<Reply> {
        let ack_present = h.flags & ACK != 0;
        if ack_present && !(seq_lt(self.iss, h.ack) && seq_le(h.ack, self.snd_nxt)) {
            return reset_reply(h, len);
        }
        if h.flags & RST != 0 {
            if ack_present {
                self.state = State::Closed;
            }
            return None;
        }
        if h.flags & SYN == 0 {
            return None;
        }
        self.rcv_nxt = h.seq.wrapping_add(1);
        self.snd_wnd = u32::from(h.window);
        if ack_present {
            self.snd_una = h.ack;
            self.state = State::Established;
            Some(self.ack_reply())
        } else {
            // Simultaneous open: answer with our SYN-ACK and wait for the ACK.
            self.state = State::SynReceived;
            Some(Reply {
                seq: self.iss,
                ack: self.rcv_nxt,
                flags: SYN | ACK,
            })
        }
    }

    fn sync_input(&mut self, now: u64, h: &Header, payload: &[u8], len: u32) -> Option<Reply> {
        if !self.acceptable(h.seq, len) {
            return if h.flags & RST != 0 {
                None
            } else {
                Some(self.ack_reply())
            };
        }
        if h.flags & RST != 0 {
            self.state = State::Closed;
            return None;
        }
        if h.flags & SYN != 0 {
            self.state = State::Closed;
            return Some(Reply {
                seq: self.snd_nxt,
                ack: 0,
                flags: RST,
            });
        }
        if h.flags & ACK == 0 {
            return None;
        }

        if self.state == State::SynReceived {
            if !(seq_lt(self.snd_una, h.ack) && seq_le(h.ack, self.snd_nxt)) {
                return reset_reply(h, len);
            }
            self.snd_una = h.ack;
            self.state = if self.fin_queued {
                State::FinWait1
            } else {
                State::Established
            };
        } else if seq_lt(self.snd_una, h.ack) {
            if seq_lt(self.snd_nxt, h.ack) {
                return Some(self.ack_reply());
            }
            let before = self.data_inflight();
            self.snd_una = h.ack;
            let acked = before.saturating_sub(self.data_inflight()).min(self.tx_len);
            self.tx.copy_within(acked..self.tx_len, 0);
            self.tx_len -= acked;
        }
        self.snd_wnd = u32::from(h.window);

        let fin_acked = self.fin_sent && self.snd_una == self.snd_nxt;
        match self.state {
            State::FinWait1 if fin_acked => self.state = State::FinWait2,
            State::Closing if fin_acked => {
                self.state = State::TimeWait;
                self.tw_since = now;
            }
            State::LastAck if fin_acked => self.state = State::Closed,
            _ => {}
        }
        if self.state == State::Closed {
            return None;
        }

        let mut need_ack = false;
        if !payload.is_empty() {
            need_ack = true;
            if matches!(
                self.state,
                State::Established | State::FinWait1 | State::FinWait2
            ) {
                self.accept_text(h.seq, payload);
            }
        }
        if h.flags & FIN != 0 {
            need_ack = true;
            let fin_seq = h.seq.wrapping_add(payload.len() as u32);
            if fin_seq == self.rcv_nxt {
                self.rcv_nxt = self.rcv_nxt.wrapping_add(1);
                self.state = match self.state {
                    State::Established => State::CloseWait,
                    State::FinWait1 => State::Closing,
                    State::FinWait2 => {
                        self.tw_since = now;
                        State::TimeWait
                    }
                    State::TimeWait => {
                        self.tw_since = now;
                        State::TimeWait
                    }
                    other => other,
                };
            }
        }
        if need_ack {
            Some(self.ack_reply())
        } else {
            None
        }
    }

    /// Stores in-order data in the receive buffer, trimming any part already
    /// received. Data past `rcv_nxt` is dropped.
    fn accept_text(&mut self, seq: u32, payload: &[u8]) {
        let mut seq = seq;
        let mut data = payload;
        if seq_lt(seq, self.rcv_nxt) {
            let skip = self.rcv_nxt.wrapping_sub(seq) as usize;
            if skip >= data.len() {
                return;
            }
            data = &data[skip..];
            seq = self.rcv_nxt;
        }
        if seq != self.rcv_nxt {
            return;
        }
        let n = data.len().min(N - self.rx_len);
        self.rx[self.rx_len..self.rx_len + n].copy_from_slice(&data[..n]);
        self.rx_len += n;
        self.rcv_nxt = self.rcv_nxt.wrapping_add(n as u32);
    }

    /// RFC 793 acceptability test for a segment that occupies `len` sequence numbers.
    fn acceptable(&self, seq: u32, len: u32) -> bool {
        let wnd = self.rcv_window();
        let in_window = |x: u32| x.wrapping_sub(self.rcv_nxt) < wnd;
        if len == 0 {
            if wnd == 0 {
                seq == self.rcv_nxt
            } else {
                in_window(seq)
            }
        } else {
            wnd != 0 && (in_window(seq) || in_window(seq.wrapping_add(len - 1)))
        }
    }

    /// Sequence numbers sent but not yet acknowledged, minus the FIN and SYN
    /// that occupy sequence space without data in the buffer.
    fn data_inflight(&self) -> usize {
        let span = self.snd_nxt.wrapping_sub(self.snd_una) as usize;
        if self.fin_sent && self.snd_una != self.snd_nxt {
            span.saturating_sub(1)
        } else {
            span
        }
    }

    /// Sends buffered data while the window allows, then the FIN once all data is out.
    fn transmit(&mut self, out: &mut Outbox) {
        if !matches!(
            self.state,
            State::Established
                | State::CloseWait
                | State::FinWait1
                | State::Closing
                | State::LastAck
        ) {
            return;
        }
        let Some(to) = self.remote else {
            return;
        };
        loop {
            if out.is_full() {
                return;
            }
            let inflight = self.data_inflight();
            let unsent = self.tx_len.saturating_sub(inflight);
            let window = (self.snd_wnd as usize).saturating_sub(inflight);
            if unsent == 0 || window == 0 {
                break;
            }
            let n = unsent.min(window).min(MSS);
            let flags = if n == unsent { ACK | PSH } else { ACK };
            let (seq, ack) = (self.snd_nxt, self.rcv_nxt);
            self.emit(out, to, seq, ack, flags, &self.tx[inflight..inflight + n]);
            self.snd_nxt = self.snd_nxt.wrapping_add(n as u32);
        }
        if self.fin_queued
            && !self.fin_sent
            && self.data_inflight() == self.tx_len
            && !out.is_full()
        {
            let (seq, ack) = (self.snd_nxt, self.rcv_nxt);
            self.emit(out, to, seq, ack, ACK | FIN, &[]);
            self.snd_nxt = self.snd_nxt.wrapping_add(1);
            self.fin_sent = true;
        }
    }

    fn ack_reply(&self) -> Reply {
        Reply {
            seq: self.snd_nxt,
            ack: self.rcv_nxt,
            flags: ACK,
        }
    }

    fn rcv_window(&self) -> u32 {
        (N - self.rx_len) as u32
    }

    fn emit(&self, out: &mut Outbox, to: Endpoint, seq: u32, ack: u32, flags: u8, payload: &[u8]) {
        let window = if flags & RST != 0 {
            0
        } else {
            u16::try_from(self.rcv_window()).unwrap_or(u16::MAX)
        };
        let header = Header {
            src_port: self.local.port,
            dst_port: to.port,
            seq,
            ack,
            flags,
            window,
        };
        // Callers check for room before each reply, and transmit checks too, so
        // this push cannot fail. Ignoring the result keeps the path panic-free.
        let _ = out.push(self.local.ip, to.ip, &header, payload);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::vec::Vec;

    const A_IP: [u8; 4] = [10, 0, 0, 1];
    const B_IP: [u8; 4] = [10, 0, 0, 2];
    const A_EP: Endpoint = Endpoint {
        ip: A_IP,
        port: 40000,
    };
    const B_EP: Endpoint = Endpoint { ip: B_IP, port: 80 };

    fn copy(out: &Outbox) -> Vec<Vec<u8>> {
        out.iter().map(|s| s.to_vec()).collect()
    }

    fn deliver<const N: usize>(
        to: &mut Connection<N>,
        from: Endpoint,
        bytes: &[u8],
        now: u64,
        out: &mut Outbox,
    ) {
        let (h, payload) = tcp::parse(from.ip, to.local().ip, bytes).unwrap();
        to.input(now, from, &h, payload, out).unwrap();
    }

    /// One exchange: every segment queued by `a` goes to `b` and the reverse.
    fn round<const N: usize>(
        a: &mut Connection<N>,
        b: &mut Connection<N>,
        now: u64,
        oa: &mut Outbox,
        ob: &mut Outbox,
    ) {
        let (a_ep, b_ep) = (a.local(), b.local());
        let to_b = copy(oa);
        oa.clear();
        let to_a = copy(ob);
        ob.clear();
        for s in &to_b {
            deliver(b, a_ep, s, now, ob);
        }
        for s in &to_a {
            deliver(a, b_ep, s, now, oa);
        }
    }

    fn exchange<const N: usize>(
        a: &mut Connection<N>,
        b: &mut Connection<N>,
        now: u64,
        oa: &mut Outbox,
        ob: &mut Outbox,
    ) {
        for _ in 0..64 {
            if oa.is_empty() && ob.is_empty() {
                return;
            }
            round(a, b, now, oa, ob);
        }
        panic!("segments did not settle");
    }

    /// A passive `b` and an active `a`, with the handshake complete.
    fn pair<const N: usize>() -> (Connection<N>, Connection<N>) {
        let mut a = Connection::<N>::new(A_EP, 1000);
        let mut b = Connection::<N>::new(B_EP, 5000);
        assert!(b.listen());
        let (mut oa, mut ob) = (Outbox::new(), Outbox::new());
        assert!(a.connect(B_EP, &mut oa));
        exchange(&mut a, &mut b, 0, &mut oa, &mut ob);
        (a, b)
    }

    #[test]
    fn three_way_handshake_establishes_both_ends() {
        let (a, b) = pair::<64>();
        assert_eq!(a.state(), State::Established);
        assert_eq!(b.state(), State::Established);
        assert_eq!(a.remote(), Some(B_EP));
        assert_eq!(b.remote(), Some(A_EP));
    }

    #[test]
    fn data_flows_both_ways_and_is_acknowledged() {
        let (mut a, mut b) = pair::<64>();
        let (mut oa, mut ob) = (Outbox::new(), Outbox::new());
        assert_eq!(a.send(b"hello"), 5);
        a.poll(&mut oa);
        exchange(&mut a, &mut b, 1, &mut oa, &mut ob);
        let mut buf = [0u8; 16];
        let n = b.recv(&mut buf);
        assert_eq!(&buf[..n], b"hello");
        assert_eq!(a.tx_pending(), 0, "the ACK frees the send buffer");

        assert_eq!(b.send(b"world"), 5);
        b.poll(&mut ob);
        exchange(&mut a, &mut b, 2, &mut oa, &mut ob);
        let n = a.recv(&mut buf);
        assert_eq!(&buf[..n], b"world");
        assert_eq!(b.tx_pending(), 0);
    }

    #[test]
    fn large_send_is_split_at_mss() {
        let (mut a, _b) = pair::<2048>();
        let mut oa = Outbox::new();
        assert_eq!(a.send(&[7u8; 1000]), 1000);
        a.poll(&mut oa);
        let payload_lens: Vec<usize> = oa.iter().map(|s| s.len() - tcp::HEADER_LEN).collect();
        assert_eq!(payload_lens, [MSS, 1000 - MSS]);
    }

    #[test]
    fn orderly_close_reaches_time_wait_then_closed() {
        let (mut a, mut b) = pair::<64>();
        let (mut oa, mut ob) = (Outbox::new(), Outbox::new());
        a.close(&mut oa);
        assert_eq!(a.state(), State::FinWait1);
        exchange(&mut a, &mut b, 100, &mut oa, &mut ob);
        assert_eq!(a.state(), State::FinWait2);
        assert_eq!(b.state(), State::CloseWait);

        b.close(&mut ob);
        assert_eq!(b.state(), State::LastAck);
        exchange(&mut a, &mut b, 100, &mut oa, &mut ob);
        assert_eq!(a.state(), State::TimeWait);
        assert_eq!(b.state(), State::Closed);

        a.tick(100 + 2 * MSL_TICKS - 1);
        assert_eq!(a.state(), State::TimeWait);
        a.tick(100 + 2 * MSL_TICKS);
        assert_eq!(a.state(), State::Closed);
    }

    #[test]
    fn simultaneous_close_passes_through_closing() {
        let (mut a, mut b) = pair::<64>();
        let (mut oa, mut ob) = (Outbox::new(), Outbox::new());
        a.close(&mut oa);
        b.close(&mut ob);
        // The two FINs cross: each side sees the other's FIN before its own is acked.
        round(&mut a, &mut b, 100, &mut oa, &mut ob);
        assert_eq!(a.state(), State::Closing);
        assert_eq!(b.state(), State::Closing);
        exchange(&mut a, &mut b, 100, &mut oa, &mut ob);
        assert_eq!(a.state(), State::TimeWait);
        assert_eq!(b.state(), State::TimeWait);
    }

    #[test]
    fn syn_to_a_closed_port_is_reset() {
        let mut a = Connection::<64>::new(A_EP, 1000);
        let mut b = Connection::<64>::new(B_EP, 5000);
        let (mut oa, mut ob) = (Outbox::new(), Outbox::new());
        assert!(a.connect(B_EP, &mut oa));
        let syn = copy(&oa).remove(0);
        oa.clear();

        deliver(&mut b, A_EP, &syn, 0, &mut ob);
        assert_eq!(b.state(), State::Closed);
        let rst = copy(&ob).remove(0);
        assert_eq!(rst[13] & RST, RST, "the reply is a reset");

        deliver(&mut a, B_EP, &rst, 0, &mut oa);
        assert_eq!(a.state(), State::Closed, "a refused connect ends in Closed");
    }

    #[test]
    fn out_of_window_segment_is_ignored_with_an_ack() {
        let (_a, mut b) = pair::<64>();
        let mut ob = Outbox::new();
        let stray = Header {
            src_port: A_EP.port,
            dst_port: B_EP.port,
            seq: b.rcv_nxt.wrapping_add(100_000),
            ack: b.snd_nxt,
            flags: ACK,
            window: 65535,
        };
        let mut buf = [0u8; 64];
        let n = tcp::write(&mut buf, A_IP, B_IP, &stray, b"x").unwrap();
        let (h, payload) = tcp::parse(A_IP, B_IP, &buf[..n]).unwrap();
        b.input(0, A_EP, &h, payload, &mut ob).unwrap();

        assert_eq!(b.state(), State::Established);
        assert_eq!(b.rx_available(), 0, "no data from an out-of-window segment");
        assert_eq!(ob.len(), 1);
        let reply = copy(&ob).remove(0);
        assert_eq!(reply[13] & ACK, ACK);
        assert_eq!(
            u32::from_be_bytes([reply[8], reply[9], reply[10], reply[11]]),
            b.rcv_nxt,
            "the reply acknowledges the next expected byte"
        );
    }

    #[test]
    fn full_outbox_refuses_input_without_changing_state() {
        let (_a, mut b) = pair::<64>();
        let mut full = Outbox::new();
        while !full.is_full() {
            full.push(
                B_IP,
                A_IP,
                &tcp::Header {
                    src_port: B_EP.port,
                    dst_port: A_EP.port,
                    seq: 0,
                    ack: 0,
                    flags: ACK,
                    window: 0,
                },
                &[],
            )
            .unwrap();
        }
        let before = b.rcv_nxt;
        let seg = Header {
            src_port: A_EP.port,
            dst_port: B_EP.port,
            seq: before,
            ack: b.snd_nxt,
            flags: ACK,
            window: 64,
        };
        assert_eq!(b.input(0, A_EP, &seg, b"data", &mut full), Err(OutboxFull));
        assert_eq!(
            b.rcv_nxt, before,
            "no state change when the reply cannot be sent"
        );
        assert_eq!(b.rx_available(), 0);
    }

    #[test]
    fn sequence_comparison_wraps() {
        assert!(seq_lt(u32::MAX, 1));
        assert!(!seq_lt(1, u32::MAX));
        assert!(seq_le(7, 7));
        assert!(seq_lt(0xFFFF_FFF0, 0x10));
    }
}
