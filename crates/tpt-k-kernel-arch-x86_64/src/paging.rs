// SPDX-License-Identifier: MIT OR Apache-2.0
//! x86_64 4-level paging. The kernel builds its own tables in place of the
//! firmware's: an identity map of the low memory with 2 MiB pages, and single
//! 4 KiB pages mapped on top of it. A 2 MiB page is split into 4 KiB pages when
//! one of them is remapped.
//!
//! Tables live in a caller-provided pool. Table entries hold physical addresses.
//! While the identity map is in force, a table's address is also its physical
//! address, so the kernel can build tables in its own statics during setup.

use tpt_k_kernel_mem::{Frame, FrameAllocator};

/// Size of a 4 KiB page.
pub const PAGE_SIZE: u64 = 4096;
/// Size of a 2 MiB page.
pub const HUGE_PAGE_SIZE: u64 = 2 * 1024 * 1024;
/// Size of a 1 GiB region, which one PDPT entry covers.
const GIGABYTE: u64 = 1024 * 1024 * 1024;

/// The entry is present.
pub const PRESENT: u64 = 1 << 0;
/// The page is writable.
pub const WRITABLE: u64 = 1 << 1;
/// The page is accessible from ring 3.
pub const USER: u64 = 1 << 2;
/// The page is not cached. Used for memory-mapped device registers.
pub const NO_CACHE: u64 = 1 << 4;
/// In a PD entry: the entry maps a 2 MiB page. In a PT entry this bit is PAT,
/// which the kernel does not use, so it is cleared there.
const HUGE: u64 = 1 << 7;
/// The physical-address field of an entry.
const ADDRESS_MASK: u64 = 0x000F_FFFF_FFFF_F000;

const ENTRIES: usize = 512;

/// A page table: 512 entries, page-aligned.
#[repr(C, align(4096))]
#[derive(Clone, Copy, Debug)]
pub struct Table(pub [u64; ENTRIES]);

impl Table {
    /// A table with no present entries.
    pub const EMPTY: Self = Self([0; ENTRIES]);
}

/// Why a mapping could not be made.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PagingError {
    /// No free table in the pool.
    PoolExhausted,
    /// A table entry points outside the pool, or maps a 1 GiB region, so the
    /// tables cannot be walked or split.
    Unsupported,
}

/// The four table levels, from the top.
const fn indices(virt: u64) -> (usize, usize, usize, usize) {
    (
        ((virt >> 39) & 0x1FF) as usize,
        ((virt >> 30) & 0x1FF) as usize,
        ((virt >> 21) & 0x1FF) as usize,
        ((virt >> 12) & 0x1FF) as usize,
    )
}

/// A set of page tables built inside `pool`. The root is table 0.
pub struct PageTables<'a> {
    pool: &'a mut [Table],
    frames: FrameAllocator<'a>,
    root: usize,
}

impl<'a> PageTables<'a> {
    /// Starts a table set in `pool`, with an empty root. Tables are handed out by
    /// a frame allocator over the pool: table `i` is frame `i`, and `bits` is the
    /// allocator's bitmap, which must hold at least `pool.len()` bits.
    ///
    /// # Errors
    ///
    /// [`PagingError::PoolExhausted`] if the pool is empty or `bits` is too short.
    pub fn new(pool: &'a mut [Table], bits: &'a mut [u64]) -> Result<Self, PagingError> {
        let base = pool.as_ptr() as u64;
        let frames =
            FrameAllocator::new(base, pool.len(), bits).ok_or(PagingError::PoolExhausted)?;
        let mut tables = Self {
            pool,
            frames,
            root: 0,
        };
        tables.root = tables.alloc()?;
        Ok(tables)
    }

    /// The active root table's address, to load into CR3.
    #[must_use]
    pub fn root_address(&self) -> u64 {
        self.table_address(self.root)
    }

    /// The address of the table set with root `index`.
    #[must_use]
    pub fn root_address_of(&self, index: usize) -> u64 {
        self.table_address(index)
    }

    /// Makes the table set with root `index` active. Mappings and lookups use
    /// the active root.
    ///
    /// # Errors
    ///
    /// [`PagingError::Unsupported`] if `index` is not an allocated table.
    pub fn select(&mut self, index: usize) -> Result<(), PagingError> {
        if index >= self.pool.len() {
            return Err(PagingError::Unsupported);
        }
        let frame =
            Frame::from_address(self.table_address(index)).ok_or(PagingError::Unsupported)?;
        if !self.frames.is_allocated(frame) {
            return Err(PagingError::Unsupported);
        }
        self.root = index;
        Ok(())
    }

    /// Creates a new address space as a copy of the active one. The copy has its
    /// own root and its own tables for the upper three levels, so mappings made in
    /// one space do not show in the other. Pages are shared until they are
    /// remapped. Returns the new root's index, without making it active.
    ///
    /// # Errors
    ///
    /// [`PagingError::PoolExhausted`] if the pool cannot hold the copy.
    pub fn fork(&mut self) -> Result<usize, PagingError> {
        self.copy_table(self.root, 0)
    }

    /// Copies a table, and the tables under it down to the page tables. Leaf
    /// entries (4 KiB pages, and 2 MiB pages) are copied as they are.
    fn copy_table(&mut self, source: usize, depth: usize) -> Result<usize, PagingError> {
        let copy = self.alloc()?;
        for index in 0..ENTRIES {
            let entry = self.pool[source].0[index];
            if entry & PRESENT == 0 {
                continue;
            }
            let copied = if depth < 3 && entry & HUGE == 0 {
                let child = self.index_of(entry & ADDRESS_MASK)?;
                let child_copy = self.copy_table(child, depth + 1)?;
                self.table_address(child_copy) | (entry & 0xFFF)
            } else {
                entry
            };
            self.pool[copy].0[index] = copied;
        }
        Ok(copy)
    }

    /// Identity maps the first `gigabytes` GiB (at most 512) with writable 2 MiB
    /// pages.
    ///
    /// # Errors
    ///
    /// [`PagingError::PoolExhausted`] if the tables do not fit in the pool.
    pub fn identity_map(&mut self, gigabytes: u64) -> Result<(), PagingError> {
        let pdpt = self.child(self.root, 0)?;
        for gigabyte in 0..gigabytes.min(ENTRIES as u64) {
            let pd = self.child(pdpt, gigabyte as usize)?;
            for (entry, slot) in self.pool[pd].0.iter_mut().enumerate() {
                let phys = gigabyte * GIGABYTE + entry as u64 * HUGE_PAGE_SIZE;
                *slot = phys | PRESENT | WRITABLE | HUGE;
            }
        }
        Ok(())
    }

    /// Maps the 4 KiB page at `virt` to the physical frame at `phys`, with the
    /// given flags added to `PRESENT`. A 2 MiB mapping covering `virt` is split
    /// first, so its other pages keep their mappings.
    ///
    /// # Errors
    ///
    /// [`PagingError::PoolExhausted`] if a table is needed and none is free, and
    /// [`PagingError::Unsupported`] if the walk runs into a 1 GiB mapping or a
    /// table outside the pool.
    pub fn map_4k(&mut self, virt: u64, phys: u64, flags: u64) -> Result<(), PagingError> {
        let (p4, p3, p2, p1) = indices(virt);
        let pdpt = self.child(self.root, p4)?;
        let pd = self.child(pdpt, p3)?;
        let pd_entry = self.pool[pd].0[p2];
        let pt = if pd_entry & PRESENT != 0 && pd_entry & HUGE != 0 {
            self.split_huge(pd, p2)?
        } else {
            self.child(pd, p2)?
        };
        self.pool[pt].0[p1] = (phys & ADDRESS_MASK) | flags | PRESENT;
        Ok(())
    }

    /// Walks the tables and returns the physical address `virt` maps to, or
    /// `None` if it is not mapped.
    #[must_use]
    pub fn translate(&self, virt: u64) -> Option<u64> {
        let (p4, p3, p2, p1) = indices(virt);
        let pdpt = self.present_child(self.root, p4)?;
        let pd = self.present_child(pdpt, p3)?;
        let pd_entry = self.pool[pd].0[p2];
        if pd_entry & PRESENT == 0 {
            return None;
        }
        if pd_entry & HUGE != 0 {
            let base = pd_entry & ADDRESS_MASK & !(HUGE_PAGE_SIZE - 1);
            return Some(base + (virt & (HUGE_PAGE_SIZE - 1)));
        }
        let pt = self.present_child(pd, p2)?;
        let entry = self.pool[pt].0[p1];
        if entry & PRESENT == 0 {
            return None;
        }
        Some((entry & ADDRESS_MASK) + (virt & (PAGE_SIZE - 1)))
    }

    fn alloc(&mut self) -> Result<usize, PagingError> {
        let frame = self.frames.allocate().ok_or(PagingError::PoolExhausted)?;
        let index = self.index_of(frame.start())?;
        self.pool[index] = Table::EMPTY;
        Ok(index)
    }

    fn table_address(&self, index: usize) -> u64 {
        core::ptr::addr_of!(self.pool[index]) as u64
    }

    fn index_of(&self, address: u64) -> Result<usize, PagingError> {
        let base = self.pool.as_ptr() as u64;
        let offset = address.checked_sub(base).ok_or(PagingError::Unsupported)?;
        let index = (offset / core::mem::size_of::<Table>() as u64) as usize;
        if index < self.pool.len() && offset % core::mem::size_of::<Table>() as u64 == 0 {
            Ok(index)
        } else {
            Err(PagingError::Unsupported)
        }
    }

    /// The child table at `parent[index]`, created if the entry is absent.
    fn child(&mut self, parent: usize, index: usize) -> Result<usize, PagingError> {
        let entry = self.pool[parent].0[index];
        if entry & PRESENT != 0 {
            if entry & HUGE != 0 {
                return Err(PagingError::Unsupported);
            }
            return self.index_of(entry & ADDRESS_MASK);
        }
        let child = self.alloc()?;
        let address = self.table_address(child);
        self.pool[parent].0[index] = address | PRESENT | WRITABLE | USER;
        Ok(child)
    }

    /// The existing child table at `parent[index]`, or `None`.
    fn present_child(&self, parent: usize, index: usize) -> Option<usize> {
        let entry = self.pool[parent].0[index];
        if entry & PRESENT == 0 || entry & HUGE != 0 {
            return None;
        }
        self.index_of(entry & ADDRESS_MASK).ok()
    }

    /// Replaces the 2 MiB mapping at `pd[index]` with a table of 512 4 KiB
    /// mappings that cover the same memory with the same flags.
    fn split_huge(&mut self, pd: usize, index: usize) -> Result<usize, PagingError> {
        let entry = self.pool[pd].0[index];
        let base = entry & ADDRESS_MASK & !(HUGE_PAGE_SIZE - 1);
        let flags = entry & 0xFFF & !HUGE;
        let pt = self.alloc()?;
        for (page, slot) in self.pool[pt].0.iter_mut().enumerate() {
            *slot = (base + page as u64 * PAGE_SIZE) | flags;
        }
        let address = self.table_address(pt);
        self.pool[pd].0[index] = address | PRESENT | WRITABLE | USER;
        Ok(pt)
    }
}

/// Invalidates the TLB entry for the page at `address` in the active space.
#[cfg(target_arch = "x86_64")]
pub fn invlpg(address: u64) {
    // SAFETY: ring 0. `invlpg` only drops a cached translation; it reads no memory.
    unsafe {
        core::arch::asm!("invlpg [{}]", in(reg) address, options(nostack, preserves_flags));
    }
}

/// Reads CR3, the root table's physical address.
#[cfg(target_arch = "x86_64")]
#[must_use]
pub fn read_cr3() -> u64 {
    let value: u64;
    // SAFETY: ring 0. Reading CR3 has no side effects.
    unsafe {
        core::arch::asm!("mov {}, cr3", out(reg) value, options(nomem, nostack, preserves_flags));
    }
    value
}

/// Loads `root` into CR3, which switches the address space and flushes the TLB.
///
/// # Safety
///
/// `root` must be a valid PML4 whose mappings cover the code, the data and the
/// stack the kernel is running on, and the kernel's devices. Otherwise the next
/// memory access faults.
#[cfg(target_arch = "x86_64")]
pub unsafe fn load_root(root: u64) {
    // SAFETY: the caller guarantees `root` maps everything the kernel touches
    // next (see the function contract). Loading CR3 keeps the flags.
    unsafe {
        core::arch::asm!("mov cr3, {}", in(reg) root, options(nostack, preserves_flags));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn identity_map_translates_addresses_to_themselves() {
        let mut pool = [Table::EMPTY; 16];
        let mut bits = [0u64; 1];
        let mut tables = PageTables::new(&mut pool, &mut bits).unwrap();
        tables.identity_map(2).unwrap();
        for virt in [0x0, 0x1234, 0x20_0000, 0x7000_0000, 0x7FFF_FFFF] {
            assert_eq!(tables.translate(virt), Some(virt), "virt {virt:#x}");
        }
        assert_eq!(tables.translate(0x8000_0000), None);
    }

    #[test]
    fn remapping_a_page_splits_its_huge_mapping_and_keeps_the_neighbours() {
        let mut pool = [Table::EMPTY; 16];
        let mut bits = [0u64; 1];
        let mut tables = PageTables::new(&mut pool, &mut bits).unwrap();
        tables.identity_map(2).unwrap();
        tables.map_4k(0x7000_0000, 0x40_0000, WRITABLE).unwrap();
        assert_eq!(tables.translate(0x7000_0123), Some(0x40_0123));
        assert_eq!(tables.translate(0x7000_1000), Some(0x7000_1000));
        assert_eq!(tables.translate(0x701F_F000), Some(0x701F_F000));
        // Remapping again reuses the split table.
        tables.map_4k(0x7000_1000, 0x50_0000, WRITABLE).unwrap();
        assert_eq!(tables.translate(0x7000_1000), Some(0x50_0000));
        assert_eq!(tables.translate(0x7000_0000), Some(0x40_0000));
    }

    #[test]
    fn running_out_of_tables_is_reported() {
        let mut pool = [Table::EMPTY; 4];
        let mut bits = [0u64; 1];
        let mut tables = PageTables::new(&mut pool, &mut bits).unwrap();
        // The identity map uses the root, the PDPT and one PD. Mapping 0x7000_0000
        // then needs a PD for its gigabyte and a PT, but only one table is left.
        tables.identity_map(1).unwrap();
        assert_eq!(
            tables.map_4k(0x7000_0000, 0x40_0000, WRITABLE),
            Err(PagingError::PoolExhausted)
        );
    }

    #[test]
    fn a_forked_address_space_maps_independently() {
        let mut pool = [Table::EMPTY; 16];
        let mut bits = [0u64; 1];
        let mut tables = PageTables::new(&mut pool, &mut bits).unwrap();
        tables.identity_map(2).unwrap();
        let boot = tables.root_address();
        let process = tables.fork().unwrap();
        assert_ne!(tables.table_address(process), boot);
        tables.select(process).unwrap();
        tables.map_4k(0x7000_0000, 0x40_0000, WRITABLE).unwrap();
        assert_eq!(tables.translate(0x7000_0000), Some(0x40_0000));
        // The other space keeps the identity mapping.
        tables.select(0).unwrap();
        assert_eq!(tables.translate(0x7000_0000), Some(0x7000_0000));
        assert_eq!(tables.root_address(), boot);
    }

    #[test]
    fn selecting_an_unallocated_root_is_refused() {
        let mut pool = [Table::EMPTY; 4];
        let mut bits = [0u64; 1];
        let mut tables = PageTables::new(&mut pool, &mut bits).unwrap();
        assert_eq!(tables.select(3), Err(PagingError::Unsupported));
    }

    #[test]
    fn unmapped_addresses_do_not_translate() {
        let mut pool = [Table::EMPTY; 4];
        let mut bits = [0u64; 1];
        let tables = PageTables::new(&mut pool, &mut bits).unwrap();
        assert_eq!(tables.translate(0x1000), None);
        assert!(matches!(
            PageTables::new(&mut [], &mut []),
            Err(PagingError::PoolExhausted)
        ));
    }
}
