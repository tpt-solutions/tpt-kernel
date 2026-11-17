// SPDX-License-Identifier: MIT OR Apache-2.0
//! A fixed-capacity ARP cache: IPv4 address to MAC, with entries that expire.
//!
//! The caller drives time: [`ArpCache::advance`] adds seconds to every entry's
//! age, and drops those that reach the time to live. When the cache is full,
//! inserting evicts the oldest entry.

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Entry {
    ip: [u8; 4],
    mac: [u8; 6],
    age: u32,
}

/// An ARP cache with room for `N` entries.
#[derive(Clone, Copy, Debug)]
pub struct ArpCache<const N: usize> {
    entries: [Option<Entry>; N],
    ttl: u32,
}

impl<const N: usize> ArpCache<N> {
    /// An empty cache whose entries expire after `ttl_seconds` of age.
    #[must_use]
    pub const fn new(ttl_seconds: u32) -> Self {
        Self {
            entries: [None; N],
            ttl: ttl_seconds,
        }
    }

    /// The MAC address for `ip`, if a live entry exists.
    #[must_use]
    pub fn lookup(&self, ip: [u8; 4]) -> Option<[u8; 6]> {
        self.entries
            .iter()
            .flatten()
            .find(|e| e.ip == ip)
            .map(|e| e.mac)
    }

    /// Records `ip` at `mac`. An existing entry is replaced and its age reset.
    /// Otherwise a free slot is used, or the oldest entry is evicted. Returns
    /// false only for a zero-capacity cache.
    pub fn insert(&mut self, ip: [u8; 4], mac: [u8; 6]) -> bool {
        if N == 0 {
            return false;
        }
        let index = self
            .entries
            .iter()
            .position(|slot| slot.is_some_and(|e| e.ip == ip))
            .or_else(|| self.entries.iter().position(Option::is_none))
            .or_else(|| (0..N).max_by_key(|&i| self.entries[i].map_or(0, |e| e.age)));
        match index {
            Some(i) => {
                self.entries[i] = Some(Entry { ip, mac, age: 0 });
                true
            }
            None => false,
        }
    }

    /// Adds `seconds` to every entry's age and drops entries at or past the
    /// time to live.
    pub fn advance(&mut self, seconds: u32) {
        let ttl = self.ttl;
        for slot in self.entries.iter_mut() {
            if let Some(e) = slot {
                e.age = e.age.saturating_add(seconds);
                if e.age >= ttl {
                    *slot = None;
                }
            }
        }
    }

    /// The number of live entries.
    #[must_use]
    pub fn len(&self) -> usize {
        self.entries.iter().flatten().count()
    }

    /// True if the cache holds no live entries.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const A: [u8; 4] = [10, 0, 0, 1];
    const B: [u8; 4] = [10, 0, 0, 2];
    const C: [u8; 4] = [10, 0, 0, 3];
    const MAC_A: [u8; 6] = [2, 0, 0, 0, 0, 1];
    const MAC_B: [u8; 6] = [2, 0, 0, 0, 0, 2];

    #[test]
    fn insert_lookup_and_replace() {
        let mut cache: ArpCache<4> = ArpCache::new(60);
        assert!(cache.insert(A, MAC_A));
        assert_eq!(cache.lookup(A), Some(MAC_A));
        assert_eq!(cache.lookup(B), None);
        let moved = [9; 6];
        cache.insert(A, moved);
        assert_eq!(cache.lookup(A), Some(moved));
        assert_eq!(cache.len(), 1);
    }

    #[test]
    fn entries_expire_by_age() {
        let mut cache: ArpCache<4> = ArpCache::new(60);
        cache.insert(A, MAC_A);
        cache.advance(59);
        assert_eq!(cache.lookup(A), Some(MAC_A));
        cache.advance(1);
        assert_eq!(cache.lookup(A), None);
        assert!(cache.is_empty());
    }

    #[test]
    fn refresh_resets_age() {
        let mut cache: ArpCache<4> = ArpCache::new(60);
        cache.insert(A, MAC_A);
        cache.advance(50);
        cache.insert(A, MAC_A);
        cache.advance(50);
        assert_eq!(cache.lookup(A), Some(MAC_A));
    }

    #[test]
    fn full_cache_evicts_the_oldest() {
        let mut cache: ArpCache<2> = ArpCache::new(600);
        cache.insert(A, MAC_A);
        cache.advance(10);
        cache.insert(B, MAC_B);
        assert!(cache.insert(C, [3; 6]));
        assert_eq!(cache.lookup(A), None);
        assert_eq!(cache.lookup(B), Some(MAC_B));
        assert_eq!(cache.len(), 2);
    }

    #[test]
    fn zero_capacity_refuses_inserts() {
        let mut cache: ArpCache<0> = ArpCache::new(60);
        assert!(!cache.insert(A, MAC_A));
        assert!(cache.is_empty());
    }
}
