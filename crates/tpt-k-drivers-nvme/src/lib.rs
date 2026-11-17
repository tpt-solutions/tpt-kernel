// SPDX-License-Identifier: MIT OR Apache-2.0
//! tpt-k NVMe driver: command and completion encodings, queue ring math, doorbell
//! offsets and PRP planning, with no device I/O.
//!
//! The byte layouts follow the NVMe base specification. A submission queue entry
//! is 64 bytes and a completion queue entry is 16 bytes, both little-endian. The
//! driver that talks to the controller builds commands here, writes them to
//! queue memory, and rings the doorbells this crate computes.

#![no_std]

#[cfg(test)]
extern crate std;

/// Size of a submission queue entry in bytes.
pub const SQE_LEN: usize = 64;
/// Size of a completion queue entry in bytes.
pub const CQE_LEN: usize = 16;
/// Memory page size that PRP entries are built against.
pub const PAGE_SIZE: u64 = 4096;
/// Most sectors one Read or Write may carry (NLB is 16 bits, 0-based).
pub const MAX_SECTORS_PER_COMMAND: u32 = 1 << 16;

/// Admin opcode: Create I/O Submission Queue.
pub const OP_CREATE_IO_SQ: u8 = 0x01;
/// Admin opcode: Create I/O Completion Queue.
pub const OP_CREATE_IO_CQ: u8 = 0x05;
/// Admin opcode: Identify.
pub const OP_IDENTIFY: u8 = 0x06;
/// I/O opcode: Write.
pub const OP_WRITE: u8 = 0x01;
/// I/O opcode: Read.
pub const OP_READ: u8 = 0x02;
/// I/O opcode: Flush.
pub const OP_FLUSH: u8 = 0x00;

/// Identify CNS value for the controller.
pub const CNS_CONTROLLER: u32 = 0x01;
/// Identify CNS value for a namespace.
pub const CNS_NAMESPACE: u32 = 0x00;

/// Why a command or plan could not be built.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NvmeError {
    /// A queue has fewer than 2 or more than 65536 entries.
    BadQueueSize,
    /// A transfer has zero sectors, or more than `MAX_SECTORS_PER_COMMAND`.
    BadSectorCount,
    /// A buffer address is not dword-aligned, which NVMe requires.
    BadAlignment,
    /// The doorbell stride field is above 15.
    BadStride,
}

/// A submission queue entry in structured form. [`Command::to_bytes`] writes the
/// 64-byte layout.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub struct Command {
    /// Opcode (CDW0 bits 0-7).
    pub opcode: u8,
    /// Command identifier, echoed in the completion (CDW0 bits 16-31).
    pub cid: u16,
    /// Namespace ID (bytes 4-7).
    pub nsid: u32,
    /// First physical buffer pointer (bytes 24-31).
    pub prp1: u64,
    /// Second PRP entry or PRP list pointer (bytes 32-39).
    pub prp2: u64,
    /// Command dwords 10 to 15.
    pub cdw: [u32; 6],
}

impl Command {
    /// A command with the given opcode and identifier and everything else zero.
    #[must_use]
    pub const fn new(opcode: u8, cid: u16) -> Self {
        Self {
            opcode,
            cid,
            nsid: 0,
            prp1: 0,
            prp2: 0,
            cdw: [0; 6],
        }
    }

    /// The 64-byte wire form.
    #[must_use]
    pub fn to_bytes(&self) -> [u8; SQE_LEN] {
        let mut out = [0u8; SQE_LEN];
        let cdw0 = u32::from(self.opcode) | (u32::from(self.cid) << 16);
        out[0..4].copy_from_slice(&cdw0.to_le_bytes());
        out[4..8].copy_from_slice(&self.nsid.to_le_bytes());
        // Bytes 8-23 are reserved, and the metadata pointer stays zero.
        out[24..32].copy_from_slice(&self.prp1.to_le_bytes());
        out[32..40].copy_from_slice(&self.prp2.to_le_bytes());
        for (i, dw) in self.cdw.iter().enumerate() {
            let at = 40 + 4 * i;
            out[at..at + 4].copy_from_slice(&dw.to_le_bytes());
        }
        out
    }
}

/// Identify the controller (CNS 1) or a namespace (CNS 0). The response is 4 KiB
/// written to `prp1`.
#[must_use]
pub fn identify(cid: u16, nsid: u32, cns: u32, prp1: u64) -> Command {
    let mut c = Command::new(OP_IDENTIFY, cid);
    c.nsid = nsid;
    c.prp1 = prp1;
    c.cdw[0] = cns & 0xFF;
    c
}

/// Create an I/O completion queue `qid` with `entries` slots, interrupt vector
/// `vector`, and interrupts enabled or not.
pub fn create_io_cq(
    cid: u16,
    qid: u16,
    entries: u32,
    prp1: u64,
    vector: u16,
    interrupts: bool,
) -> Result<Command, NvmeError> {
    let qsize = queue_size_field(entries)?;
    let mut c = Command::new(OP_CREATE_IO_CQ, cid);
    c.prp1 = prp1;
    c.cdw[0] = (u32::from(qsize) << 16) | u32::from(qid);
    // CDW11: bit 0 physically contiguous, bit 1 interrupts enabled, 16-31 vector.
    c.cdw[1] = 0x1 | (u32::from(interrupts) << 1) | (u32::from(vector) << 16);
    Ok(c)
}

/// Create an I/O submission queue `qid` with `entries` slots that completes into
/// completion queue `cqid`. `priority` is 0 (urgent) to 3 (low).
pub fn create_io_sq(
    cid: u16,
    qid: u16,
    entries: u32,
    prp1: u64,
    cqid: u16,
    priority: u8,
) -> Result<Command, NvmeError> {
    let qsize = queue_size_field(entries)?;
    let mut c = Command::new(OP_CREATE_IO_SQ, cid);
    c.prp1 = prp1;
    c.cdw[0] = (u32::from(qsize) << 16) | u32::from(qid);
    // CDW11: bit 0 contiguous, bits 1-2 priority, bits 16-31 completion queue ID.
    c.cdw[1] = 0x1 | (u32::from(priority & 0x3) << 1) | (u32::from(cqid) << 16);
    Ok(c)
}

/// The 0-based queue size field for a queue of `entries` slots.
fn queue_size_field(entries: u32) -> Result<u16, NvmeError> {
    if !(2..=65536).contains(&entries) {
        return Err(NvmeError::BadQueueSize);
    }
    Ok((entries - 1) as u16)
}

/// Read `sectors` sectors starting at `slba` into the buffer described by
/// `prp1` and `prp2` (see [`plan_prp`]).
pub fn read(
    cid: u16,
    nsid: u32,
    slba: u64,
    sectors: u32,
    prp1: u64,
    prp2: u64,
) -> Result<Command, NvmeError> {
    data_command(OP_READ, cid, nsid, slba, sectors, prp1, prp2)
}

/// Write `sectors` sectors starting at `slba` from the buffer described by
/// `prp1` and `prp2`.
pub fn write(
    cid: u16,
    nsid: u32,
    slba: u64,
    sectors: u32,
    prp1: u64,
    prp2: u64,
) -> Result<Command, NvmeError> {
    data_command(OP_WRITE, cid, nsid, slba, sectors, prp1, prp2)
}

fn data_command(
    opcode: u8,
    cid: u16,
    nsid: u32,
    slba: u64,
    sectors: u32,
    prp1: u64,
    prp2: u64,
) -> Result<Command, NvmeError> {
    if sectors == 0 || sectors > MAX_SECTORS_PER_COMMAND {
        return Err(NvmeError::BadSectorCount);
    }
    let mut c = Command::new(opcode, cid);
    c.nsid = nsid;
    c.prp1 = prp1;
    c.prp2 = prp2;
    // CDW10-11: starting LBA, low dword first. CDW12 bits 0-15: number of
    // logical blocks, 0-based.
    c.cdw[0] = slba as u32;
    c.cdw[1] = (slba >> 32) as u32;
    c.cdw[2] = sectors - 1;
    Ok(c)
}

/// Flush the volatile write cache of a namespace.
#[must_use]
pub fn flush(cid: u16, nsid: u32) -> Command {
    let mut c = Command::new(OP_FLUSH, cid);
    c.nsid = nsid;
    c
}

/// The NVMe status code type and value of a completion.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Status {
    /// Status code type (3 bits): 0 generic, 1 command specific, 2 media, 7 vendor.
    pub sct: u8,
    /// Status code (8 bits). Zero with `sct` zero means success.
    pub sc: u8,
    /// More status information: the command's error is in its log page.
    pub more: bool,
    /// Do Not Retry: retrying the command cannot succeed.
    pub do_not_retry: bool,
    /// Command retry delay selector (2 bits).
    pub crd: u8,
}

impl Status {
    /// True for a successful completion.
    #[must_use]
    pub const fn is_success(&self) -> bool {
        self.sct == 0 && self.sc == 0
    }
}

/// A decoded completion queue entry.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Completion {
    /// Command-specific result (dword 0).
    pub result: u32,
    /// Submission queue head pointer, which tells the driver how many entries
    /// the controller has consumed.
    pub sq_head: u16,
    /// Submission queue the command came from.
    pub sq_id: u16,
    /// Identifier of the completed command.
    pub cid: u16,
    /// Phase tag: equal to the queue's expected phase for a new entry.
    pub phase: bool,
    /// The status field.
    pub status: Status,
}

impl Completion {
    /// Decodes a 16-byte completion queue entry.
    #[must_use]
    pub fn parse(raw: &[u8; CQE_LEN]) -> Self {
        let dw = |at: usize| u32::from_le_bytes([raw[at], raw[at + 1], raw[at + 2], raw[at + 3]]);
        let dw2 = dw(8);
        let dw3 = dw(12);
        Self {
            result: dw(0),
            sq_head: dw2 as u16,
            sq_id: (dw2 >> 16) as u16,
            cid: dw3 as u16,
            phase: (dw3 >> 16) & 1 == 1,
            status: Status {
                sc: ((dw3 >> 17) & 0xFF) as u8,
                sct: ((dw3 >> 25) & 0x7) as u8,
                crd: ((dw3 >> 28) & 0x3) as u8,
                more: (dw3 >> 30) & 1 == 1,
                do_not_retry: (dw3 >> 31) & 1 == 1,
            },
        }
    }
}

/// Offset of a doorbell register from the controller's BAR0 base.
///
/// `dstrd` is `CAP.DSTRD`, the doorbell stride as a power of two in 4-byte units.
/// The submission tail doorbell of queue `qid` is at `0x1000 + 2*qid*stride` and
/// the completion head doorbell at `0x1000 + (2*qid + 1)*stride`.
pub fn doorbell_offset(qid: u16, completion: bool, dstrd: u8) -> Result<u64, NvmeError> {
    if dstrd > 15 {
        return Err(NvmeError::BadStride);
    }
    let stride = 4u64 << dstrd;
    let index = 2 * u64::from(qid) + u64::from(completion);
    Ok(0x1000 + index * stride)
}

/// The submission side of a queue: producer tail and the head the controller
/// reports back. One slot is always left empty, so `entries - 1` commands fit.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SubmissionRing {
    entries: u16,
    tail: u16,
    head: u16,
}

impl SubmissionRing {
    /// A ring of `entries` slots, empty.
    pub fn new(entries: u32) -> Result<Self, NvmeError> {
        queue_size_field(entries)?;
        Ok(Self {
            entries: entries as u16,
            tail: 0,
            head: 0,
        })
    }

    /// Reserves the next slot and returns its index, or `None` if the ring is full.
    pub fn push(&mut self) -> Option<u16> {
        if self.is_full() {
            return None;
        }
        let slot = self.tail;
        self.tail = (self.tail + 1) % self.entries;
        Some(slot)
    }

    /// The tail value to write to the submission tail doorbell.
    #[must_use]
    pub const fn tail_doorbell_value(&self) -> u16 {
        self.tail
    }

    /// Records the head the controller reported in a completion.
    pub fn update_head(&mut self, sq_head: u16) {
        self.head = sq_head % self.entries;
    }

    /// True when no further command can be pushed.
    #[must_use]
    pub const fn is_full(&self) -> bool {
        (self.tail + 1) % self.entries == self.head
    }

    /// True when every pushed command has been consumed.
    #[must_use]
    pub const fn is_empty(&self) -> bool {
        self.tail == self.head
    }

    /// Commands that can still be pushed.
    #[must_use]
    pub const fn free(&self) -> u16 {
        (self.head + self.entries - self.tail - 1) % self.entries
    }
}

/// The completion side of a queue: consumer head and the phase tag that marks a
/// new entry. The phase flips each time the head wraps to slot 0.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CompletionRing {
    entries: u16,
    head: u16,
    phase: bool,
}

impl CompletionRing {
    /// A ring of `entries` slots. The first pass expects phase 1, as the
    /// controller starts its phase at 1 after queue creation.
    pub fn new(entries: u32) -> Result<Self, NvmeError> {
        queue_size_field(entries)?;
        Ok(Self {
            entries: entries as u16,
            head: 0,
            phase: true,
        })
    }

    /// The head value to write to the completion head doorbell.
    #[must_use]
    pub const fn head_doorbell_value(&self) -> u16 {
        self.head
    }

    /// The phase tag a new entry must carry right now.
    #[must_use]
    pub const fn expected_phase(&self) -> bool {
        self.phase
    }

    /// Takes the entry at the head if it is new (its phase matches), and advances.
    /// Returns `None` when the head entry is still stale.
    pub fn consume(&mut self, raw: &[u8; CQE_LEN]) -> Option<Completion> {
        let cqe = Completion::parse(raw);
        if cqe.phase != self.phase {
            return None;
        }
        self.head += 1;
        if self.head == self.entries {
            self.head = 0;
            self.phase = !self.phase;
        }
        Some(cqe)
    }
}

/// How a data buffer is described to the controller.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PrpPlan {
    /// The whole transfer is in one page. Only PRP1 is used; PRP2 is zero.
    Single {
        /// Value for PRP1.
        prp1: u64,
    },
    /// The transfer spans two pages. PRP2 points at the second page.
    Double {
        /// Value for PRP1.
        prp1: u64,
        /// Value for PRP2: the second page.
        prp2: u64,
    },
    /// The transfer spans more than two pages. PRP2 must point at a PRP list the
    /// caller builds in a page of its own. The list holds `entries` page
    /// addresses for pages 1 onward.
    List {
        /// Value for PRP1.
        prp1: u64,
        /// Number of page addresses the list must hold.
        entries: u32,
    },
}

/// Works out how to describe a transfer of `len` bytes that starts at `addr`.
///
/// `addr` must be dword-aligned, which NVMe requires for PRP entries. The first
/// PRP may carry an offset within its page, and later entries are page-aligned.
pub fn plan_prp(addr: u64, len: u64) -> Result<PrpPlan, NvmeError> {
    if len == 0 {
        return Err(NvmeError::BadSectorCount);
    }
    if !addr.is_multiple_of(4) {
        return Err(NvmeError::BadAlignment);
    }
    let offset = addr % PAGE_SIZE;
    let pages = (offset + len).div_ceil(PAGE_SIZE);
    let page_after_first = (addr & !(PAGE_SIZE - 1)) + PAGE_SIZE;
    Ok(match pages {
        1 => PrpPlan::Single { prp1: addr },
        2 => PrpPlan::Double {
            prp1: addr,
            prp2: page_after_first,
        },
        n => PrpPlan::List {
            prp1: addr,
            entries: (n - 1) as u32,
        },
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn word(b: &[u8], at: usize) -> u32 {
        u32::from_le_bytes([b[at], b[at + 1], b[at + 2], b[at + 3]])
    }

    fn quad(b: &[u8], at: usize) -> u64 {
        u64::from_le_bytes(b[at..at + 8].try_into().unwrap())
    }

    #[test]
    fn read_one_sector_has_zero_based_nlb_and_exact_offsets() {
        let c = read(0x1234, 1, 0x1_0000_0002, 1, 0x1000_2000, 0x1000_3000).unwrap();
        let b = c.to_bytes();
        assert_eq!(b[0], OP_READ);
        assert_eq!(&b[2..4], &[0x34, 0x12]);
        assert_eq!(word(&b, 4), 1);
        assert_eq!(quad(&b, 24), 0x1000_2000);
        assert_eq!(quad(&b, 32), 0x1000_3000);
        assert_eq!(word(&b, 40), 0x0000_0002);
        assert_eq!(word(&b, 44), 0x0000_0001);
        assert_eq!(word(&b, 48), 0, "NLB is 0-based: one sector is 0");
        assert_eq!(b.len(), SQE_LEN);
    }

    #[test]
    fn read_eight_sectors_sets_nlb_seven() {
        let c = read(1, 1, 0, 8, 0x2000, 0).unwrap();
        assert_eq!(word(&c.to_bytes(), 48), 7);
    }

    #[test]
    fn write_uses_opcode_one_and_keeps_fields() {
        let b = write(9, 2, 5, 4, 0x3000, 0x4000).unwrap().to_bytes();
        assert_eq!(b[0], OP_WRITE);
        assert_eq!(word(&b, 4), 2);
        assert_eq!(word(&b, 40), 5);
        assert_eq!(word(&b, 48), 3);
    }

    #[test]
    fn sector_count_bounds_are_enforced() {
        assert_eq!(read(0, 1, 0, 0, 0x1000, 0), Err(NvmeError::BadSectorCount));
        assert_eq!(
            read(0, 1, 0, MAX_SECTORS_PER_COMMAND + 1, 0x1000, 0),
            Err(NvmeError::BadSectorCount)
        );
        let max = read(0, 1, 0, MAX_SECTORS_PER_COMMAND, 0x1000, 0).unwrap();
        assert_eq!(word(&max.to_bytes(), 48), 0xFFFF);
    }

    #[test]
    fn flush_has_only_opcode_cid_and_nsid() {
        let b = flush(0xABCD, 3).to_bytes();
        assert_eq!(b[0], OP_FLUSH);
        assert_eq!(&b[2..4], &[0xCD, 0xAB]);
        assert_eq!(word(&b, 4), 3);
        assert!(b[8..].iter().all(|x| *x == 0));
    }

    #[test]
    fn identify_puts_cns_in_cdw10() {
        let ctrl = identify(7, 0, CNS_CONTROLLER, 0x5000).to_bytes();
        assert_eq!(ctrl[0], OP_IDENTIFY);
        assert_eq!(quad(&ctrl, 24), 0x5000);
        assert_eq!(word(&ctrl, 40), 1);
        let ns = identify(8, 1, CNS_NAMESPACE, 0x6000).to_bytes();
        assert_eq!(word(&ns, 4), 1);
        assert_eq!(word(&ns, 40), 0);
    }

    #[test]
    fn create_io_cq_encodes_size_id_vector_and_flags() {
        let b = create_io_cq(3, 2, 64, 0x7000, 5, true).unwrap().to_bytes();
        assert_eq!(b[0], OP_CREATE_IO_CQ);
        // CDW10: QSIZE (0-based) in 16-31, QID in 0-15.
        assert_eq!(word(&b, 40), (63 << 16) | 2);
        // CDW11: PC bit 0, IEN bit 1, vector in 16-31.
        assert_eq!(word(&b, 44), (5 << 16) | 0b11);
        assert_eq!(quad(&b, 24), 0x7000);
        let polled = create_io_cq(3, 2, 64, 0x7000, 0, false).unwrap().to_bytes();
        assert_eq!(word(&polled, 44), 0b01);
    }

    #[test]
    fn create_io_sq_encodes_cq_binding_and_priority() {
        let b = create_io_sq(4, 1, 256, 0x8000, 1, 2).unwrap().to_bytes();
        assert_eq!(b[0], OP_CREATE_IO_SQ);
        assert_eq!(word(&b, 40), (255 << 16) | 1);
        // PC bit 0, priority in bits 1-2, completion queue ID in 16-31.
        assert_eq!(word(&b, 44), (1 << 16) | (2 << 1) | 1);
    }

    #[test]
    fn queue_sizes_outside_two_to_65536_are_refused() {
        assert_eq!(
            create_io_cq(0, 1, 1, 0, 0, false),
            Err(NvmeError::BadQueueSize)
        );
        assert_eq!(
            create_io_sq(0, 1, 65537, 0, 1, 0),
            Err(NvmeError::BadQueueSize)
        );
        assert!(create_io_sq(0, 1, 65536, 0, 1, 0).is_ok());
    }

    fn cqe(
        result: u32,
        sq_head: u16,
        sq_id: u16,
        cid: u16,
        phase: bool,
        sc: u8,
        sct: u8,
    ) -> [u8; 16] {
        let mut raw = [0u8; 16];
        raw[0..4].copy_from_slice(&result.to_le_bytes());
        let dw2 = u32::from(sq_head) | (u32::from(sq_id) << 16);
        raw[8..12].copy_from_slice(&dw2.to_le_bytes());
        let dw3 = u32::from(cid)
            | (u32::from(phase) << 16)
            | (u32::from(sc) << 17)
            | (u32::from(sct) << 25)
            | (1 << 30)
            | (1 << 31);
        raw[12..16].copy_from_slice(&dw3.to_le_bytes());
        raw
    }

    #[test]
    fn completion_decodes_every_field() {
        let raw = cqe(0xDEAD_BEEF, 5, 2, 0x77, true, 0x0B, 1);
        let c = Completion::parse(&raw);
        assert_eq!(c.result, 0xDEAD_BEEF);
        assert_eq!(c.sq_head, 5);
        assert_eq!(c.sq_id, 2);
        assert_eq!(c.cid, 0x77);
        assert!(c.phase);
        assert_eq!(c.status.sc, 0x0B);
        assert_eq!(c.status.sct, 1);
        assert!(c.status.more);
        assert!(c.status.do_not_retry);
        assert!(!c.status.is_success());
    }

    #[test]
    fn success_has_zero_status_code_and_type() {
        let c = Completion::parse(&cqe(0, 0, 0, 1, false, 0, 0));
        // The raw helper sets M and DNR, but a success is sct 0 and sc 0.
        assert!(c.status.is_success());
    }

    #[test]
    fn doorbells_for_dstrd_zero_and_two() {
        // dstrd 0: stride 4 bytes.
        assert_eq!(doorbell_offset(0, false, 0), Ok(0x1000));
        assert_eq!(doorbell_offset(0, true, 0), Ok(0x1004));
        assert_eq!(doorbell_offset(1, false, 0), Ok(0x1008));
        assert_eq!(doorbell_offset(1, true, 0), Ok(0x100C));
        // dstrd 2: stride 16 bytes.
        assert_eq!(doorbell_offset(0, false, 2), Ok(0x1000));
        assert_eq!(doorbell_offset(0, true, 2), Ok(0x1010));
        assert_eq!(doorbell_offset(1, false, 2), Ok(0x1020));
        assert_eq!(doorbell_offset(1, true, 2), Ok(0x1030));
        assert_eq!(doorbell_offset(0, false, 16), Err(NvmeError::BadStride));
    }

    #[test]
    fn submission_ring_keeps_one_slot_empty() {
        let mut r = SubmissionRing::new(4).unwrap();
        assert!(r.is_empty());
        assert_eq!(r.free(), 3);
        assert_eq!(r.push(), Some(0));
        assert_eq!(r.push(), Some(1));
        assert_eq!(r.push(), Some(2));
        assert!(r.is_full());
        assert_eq!(r.push(), None);
        // The controller consumes two entries; room opens again.
        r.update_head(2);
        assert_eq!(r.free(), 2);
        assert_eq!(r.push(), Some(3));
        assert_eq!(r.push(), Some(0), "tail wraps");
        assert_eq!(r.tail_doorbell_value(), 1);
    }

    #[test]
    fn completion_ring_phase_flips_on_wrap() {
        let mut r = CompletionRing::new(2).unwrap();
        assert!(r.expected_phase());
        // Entry 0 with phase 1 is new.
        assert!(r.consume(&cqe(1, 1, 1, 10, true, 0, 0)).is_some());
        assert_eq!(r.head_doorbell_value(), 1);
        // Entry 1 with phase 1 is new; the head wraps and the phase flips.
        assert!(r.consume(&cqe(2, 2, 1, 11, true, 0, 0)).is_some());
        assert_eq!(r.head_doorbell_value(), 0);
        assert!(!r.expected_phase());
        // Slot 0 still holds phase 1 from the last pass: it is stale now.
        assert!(r.consume(&cqe(1, 1, 1, 10, true, 0, 0)).is_none());
        // The controller writes phase 0 on its next pass.
        assert!(r.consume(&cqe(3, 1, 1, 12, false, 0, 0)).is_some());
        assert_eq!(r.head_doorbell_value(), 1);
    }

    #[test]
    fn prp_single_double_and_list() {
        assert_eq!(plan_prp(0x1000, 512), Ok(PrpPlan::Single { prp1: 0x1000 }));
        // 1024 bytes that start 0x200 into a page cross into a second page.
        assert_eq!(
            plan_prp(0x1E00, 1024),
            Ok(PrpPlan::Double {
                prp1: 0x1E00,
                prp2: 0x2000
            })
        );
        assert_eq!(
            plan_prp(0x1000, 8192),
            Ok(PrpPlan::Double {
                prp1: 0x1000,
                prp2: 0x2000
            })
        );
        // Three pages need a list of two entries for pages 1 and 2.
        assert_eq!(
            plan_prp(0x1000, 12288),
            Ok(PrpPlan::List {
                prp1: 0x1000,
                entries: 2
            })
        );
    }

    #[test]
    fn prp_refuses_misaligned_or_empty_buffers() {
        assert_eq!(plan_prp(0x1002, 512), Err(NvmeError::BadAlignment));
        assert_eq!(plan_prp(0x1000, 0), Err(NvmeError::BadSectorCount));
        assert_eq!(plan_prp(0x1004, 512), Ok(PrpPlan::Single { prp1: 0x1004 }));
    }
}
