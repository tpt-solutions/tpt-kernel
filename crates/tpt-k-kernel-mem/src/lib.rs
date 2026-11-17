// SPDX-License-Identifier: MIT OR Apache-2.0
//! tpt-k kernel memory. Phase 2 has the physical page frame allocator.
//! Virtual memory and per-process address spaces come next.
//!
//! The allocator keeps a bitmap over one contiguous range of 4 KiB frames. The
//! caller supplies the bitmap storage, so the allocator needs no heap.

#![no_std]

pub mod bootmap;

/// Size of a page frame in bytes.
pub const FRAME_SIZE: u64 = 4096;

/// Why [`clear_frame`] refused a buffer.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ClearError {
    /// The buffer is not exactly one frame (`FRAME_SIZE` bytes) long.
    WrongLength,
}

/// Zeroes one page frame before the frame is handed to another owner.
///
/// This is the scalar path: the slice is filled with `0`, which the compiler
/// lowers to a plain store loop or a `memset`. Acceleration with `tpt-simd`
/// (AVX2 or NEON, with this scalar path as the fallback) is deferred until the
/// Phase 6 and Phase 9 optimisation work.
///
/// Returns `WrongLength` unless `frame` is exactly `FRAME_SIZE` bytes, and
/// leaves the buffer untouched in that case.
pub fn clear_frame(frame: &mut [u8]) -> Result<(), ClearError> {
    if frame.len() != FRAME_SIZE as usize {
        return Err(ClearError::WrongLength);
    }
    frame.fill(0);
    Ok(())
}

/// Frames tracked per bitmap word.
const BITS: usize = u64::BITS as usize;

/// A physical page frame, named by its 4 KiB-aligned start address.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Frame(u64);

impl Frame {
    /// The frame that starts at `address`, or `None` if it is not 4 KiB-aligned.
    #[must_use]
    pub const fn from_address(address: u64) -> Option<Self> {
        if address.is_multiple_of(FRAME_SIZE) {
            Some(Self(address))
        } else {
            None
        }
    }

    /// The first physical address of the frame.
    #[must_use]
    pub const fn start(self) -> u64 {
        self.0
    }
}

/// Why a frame operation was refused.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FrameError {
    /// The frame is outside the range this allocator manages.
    OutOfRange,
    /// `free` was called on a frame that is not allocated (double free, or a
    /// frame that was never handed out).
    NotAllocated,
    /// `reserve` was called on a frame that is already allocated.
    AlreadyAllocated,
}

/// Bitmap allocator over the frames `base .. base + frames * FRAME_SIZE`.
///
/// A set bit marks a frame in use. Bits past the last frame are set at
/// construction, so the search never returns a frame outside the range.
#[derive(Debug)]
pub struct FrameAllocator<'a> {
    bits: &'a mut [u64],
    base: u64,
    frames: usize,
    free: usize,
    hint: usize,
}

impl<'a> FrameAllocator<'a> {
    /// Creates an allocator over `frames` frames starting at `base`, with all of
    /// them free. `bits` holds the bitmap: at least `frames.div_ceil(64)` words,
    /// and each word is overwritten.
    ///
    /// Returns `None` if `base` is not frame-aligned or `bits` is too short.
    pub fn new(base: u64, frames: usize, bits: &'a mut [u64]) -> Option<Self> {
        if !base.is_multiple_of(FRAME_SIZE) || bits.len() < frames.div_ceil(BITS) {
            return None;
        }
        for (word_index, word) in bits.iter_mut().enumerate() {
            let first = word_index * BITS;
            *word = 0;
            for bit in 0..BITS {
                if first + bit >= frames {
                    *word |= 1 << bit;
                }
            }
        }
        Some(Self {
            bits,
            base,
            frames,
            free: frames,
            hint: 0,
        })
    }

    /// Total frames under management.
    #[must_use]
    pub fn total_frames(&self) -> usize {
        self.frames
    }

    /// Frames not currently allocated.
    #[must_use]
    pub fn free_frames(&self) -> usize {
        self.free
    }

    /// Hands out the lowest-numbered free frame near the last allocation, or
    /// `None` if every frame is in use.
    pub fn allocate(&mut self) -> Option<Frame> {
        if self.free == 0 {
            return None;
        }
        let words = self.bits.len();
        for step in 0..words {
            let word_index = (self.hint + step) % words;
            let word = self.bits[word_index];
            if word != u64::MAX {
                let bit = (!word).trailing_zeros() as usize;
                self.bits[word_index] |= 1 << bit;
                self.free -= 1;
                self.hint = word_index;
                return Some(self.frame_at(word_index * BITS + bit));
            }
        }
        None
    }

    /// Returns `frame` to the pool.
    ///
    /// # Errors
    ///
    /// [`FrameError::OutOfRange`] if the frame is not managed here, and
    /// [`FrameError::NotAllocated`] if it is already free.
    pub fn free(&mut self, frame: Frame) -> Result<(), FrameError> {
        let index = self.index_of(frame)?;
        let (word, bit) = (index / BITS, index % BITS);
        if self.bits[word] & (1 << bit) == 0 {
            return Err(FrameError::NotAllocated);
        }
        self.bits[word] &= !(1 << bit);
        self.free += 1;
        self.hint = word;
        Ok(())
    }

    /// Marks a specific frame as in use, for memory the kernel already owns (the
    /// image, the boot stack, firmware-reserved ranges).
    ///
    /// # Errors
    ///
    /// [`FrameError::OutOfRange`] if the frame is not managed here, and
    /// [`FrameError::AlreadyAllocated`] if it is already in use.
    pub fn reserve(&mut self, frame: Frame) -> Result<(), FrameError> {
        let index = self.index_of(frame)?;
        let (word, bit) = (index / BITS, index % BITS);
        if self.bits[word] & (1 << bit) != 0 {
            return Err(FrameError::AlreadyAllocated);
        }
        self.bits[word] |= 1 << bit;
        self.free -= 1;
        Ok(())
    }

    /// True if `frame` is managed here and is currently allocated.
    #[must_use]
    pub fn is_allocated(&self, frame: Frame) -> bool {
        match self.index_of(frame) {
            Ok(index) => self.bits[index / BITS] & (1 << (index % BITS)) != 0,
            Err(_) => false,
        }
    }

    fn frame_at(&self, index: usize) -> Frame {
        Frame(self.base + index as u64 * FRAME_SIZE)
    }

    fn index_of(&self, frame: Frame) -> Result<usize, FrameError> {
        let offset = frame
            .0
            .checked_sub(self.base)
            .ok_or(FrameError::OutOfRange)?;
        let index = usize::try_from(offset / FRAME_SIZE).map_err(|_| FrameError::OutOfRange)?;
        if index < self.frames {
            Ok(index)
        } else {
            Err(FrameError::OutOfRange)
        }
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn clear_frame_zeroes_a_dirty_page() {
        let mut page = [0xABu8; FRAME_SIZE as usize];
        assert_eq!(clear_frame(&mut page), Ok(()));
        assert!(page.iter().all(|b| *b == 0));
    }

    #[test]
    fn clear_frame_refuses_wrong_lengths_and_leaves_them_alone() {
        let mut short = [0x5Au8; 4095];
        assert_eq!(clear_frame(&mut short), Err(ClearError::WrongLength));
        assert!(short.iter().all(|b| *b == 0x5A));
        let mut long = [0x5Au8; 4097];
        assert_eq!(clear_frame(&mut long), Err(ClearError::WrongLength));
        assert!(long.iter().all(|b| *b == 0x5A));
        let mut empty: [u8; 0] = [];
        assert_eq!(clear_frame(&mut empty), Err(ClearError::WrongLength));
    }

    use super::*;

    #[test]
    fn frame_must_be_aligned() {
        assert!(Frame::from_address(0x1000).is_some());
        assert!(Frame::from_address(0x1001).is_none());
    }

    #[test]
    fn hands_out_every_frame_exactly_once() {
        let mut bits = [0u64; 2];
        let mut alloc = FrameAllocator::new(0x10_0000, 70, &mut bits).unwrap();
        assert_eq!(alloc.total_frames(), 70);
        let mut seen = [false; 70];
        for _ in 0..70 {
            let frame = alloc.allocate().expect("a free frame");
            let index = ((frame.start() - 0x10_0000) / FRAME_SIZE) as usize;
            assert!(!seen[index], "frame {index} handed out twice");
            seen[index] = true;
        }
        assert_eq!(alloc.free_frames(), 0);
        assert_eq!(alloc.allocate(), None);
        assert!(seen.iter().all(|&s| s));
    }

    #[test]
    fn freed_frames_are_reused() {
        let mut bits = [0u64; 1];
        let mut alloc = FrameAllocator::new(0, 8, &mut bits).unwrap();
        let frame = alloc.allocate().unwrap();
        alloc.free(frame).unwrap();
        assert_eq!(alloc.free_frames(), 8);
        assert_eq!(alloc.allocate(), Some(frame));
    }

    #[test]
    fn double_free_and_foreign_frames_are_refused() {
        let mut bits = [0u64; 1];
        let mut alloc = FrameAllocator::new(0x4000, 4, &mut bits).unwrap();
        let frame = alloc.allocate().unwrap();
        alloc.free(frame).unwrap();
        assert_eq!(alloc.free(frame), Err(FrameError::NotAllocated));
        assert_eq!(
            alloc.free(Frame::from_address(0x3000).unwrap()),
            Err(FrameError::OutOfRange)
        );
        assert_eq!(
            alloc.free(Frame::from_address(0x8000).unwrap()),
            Err(FrameError::OutOfRange)
        );
    }

    #[test]
    fn reserved_frames_are_not_handed_out() {
        let mut bits = [0u64; 1];
        let mut alloc = FrameAllocator::new(0, 3, &mut bits).unwrap();
        alloc
            .reserve(Frame::from_address(FRAME_SIZE).unwrap())
            .unwrap();
        assert_eq!(
            alloc.reserve(Frame::from_address(FRAME_SIZE).unwrap()),
            Err(FrameError::AlreadyAllocated)
        );
        let a = alloc.allocate().unwrap();
        let b = alloc.allocate().unwrap();
        assert_eq!(a.start(), 0);
        assert_eq!(b.start(), 2 * FRAME_SIZE);
        assert_eq!(alloc.allocate(), None);
    }

    #[test]
    fn rejects_bad_construction() {
        let mut bits = [0u64; 1];
        assert!(FrameAllocator::new(0x10, 4, &mut bits).is_none());
        let mut short = [0u64; 1];
        assert!(FrameAllocator::new(0, 65, &mut short).is_none());
    }

    #[test]
    fn zero_frames_allocates_nothing() {
        let mut bits: [u64; 0] = [];
        let mut alloc = FrameAllocator::new(0, 0, &mut bits).unwrap();
        assert_eq!(alloc.allocate(), None);
    }

    #[test]
    fn is_allocated_reports_frames_in_use() {
        let mut bits = [0u64; 1];
        let mut alloc = FrameAllocator::new(0, 4, &mut bits).unwrap();
        let frame = alloc.allocate().unwrap();
        assert!(alloc.is_allocated(frame));
        alloc.free(frame).unwrap();
        assert!(!alloc.is_allocated(frame));
        assert!(!alloc.is_allocated(Frame::from_address(0x9000).unwrap()));
    }
}
