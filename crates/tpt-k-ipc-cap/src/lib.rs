// SPDX-License-Identifier: MIT OR Apache-2.0
//! tpt-k IPC capabilities: rights, handles and unforgeable tokens.
//!
//! A capability names an object and carries the rights its holder may use. A
//! process holds a capability as a handle in its own table, and the kernel
//! checks the rights on every use. A token is the form a capability takes when
//! it has to leave the kernel, such as in a message to a user-space server. The
//! token carries a keyed tag, an HMAC-SHA256 under a server key, so a holder
//! cannot make a token or widen one without the key.
//!
//! This crate holds no key and no table. The root server owns the key, and the
//! kernel owns the tables.

#![no_std]

#[cfg(test)]
extern crate std;

use tpt_primitives::hash::sha256;

/// The rights a capability may carry. A set of bits.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Default)]
pub struct Rights(u32);

impl Rights {
    /// No rights.
    pub const NONE: Self = Self(0);
    /// Send a message on an endpoint.
    pub const SEND: Self = Self(1 << 0);
    /// Receive a message from an endpoint.
    pub const RECV: Self = Self(1 << 1);
    /// Copy the capability to another holder.
    pub const GRANT: Self = Self(1 << 2);
    /// Map a memory object into an address space.
    pub const MAP: Self = Self(1 << 3);
    /// Revoke the capability and the copies made from it.
    pub const REVOKE: Self = Self(1 << 4);
    /// Mint new root capabilities for objects, as the root server does.
    pub const MINT: Self = Self(1 << 5);
    /// Every right this crate defines.
    pub const ALL: Self = Self(0x3F);

    /// Rights from raw bits. Bits this crate does not define are cleared.
    #[must_use]
    pub const fn from_bits(bits: u32) -> Self {
        Self(bits & Self::ALL.0)
    }

    /// The raw bits.
    #[must_use]
    pub const fn bits(self) -> u32 {
        self.0
    }

    /// True if every right in `other` is also in `self`.
    #[must_use]
    pub const fn contains(self, other: Self) -> bool {
        self.0 & other.0 == other.0
    }

    /// The rights in either set.
    #[must_use]
    pub const fn union(self, other: Self) -> Self {
        Self(self.0 | other.0)
    }

    /// The rights in both sets.
    #[must_use]
    pub const fn intersection(self, other: Self) -> Self {
        Self(self.0 & other.0)
    }

    /// The rights `self` may be narrowed to. `Some` only when `wanted` is a
    /// subset of `self`: a capability can lose rights but never gain them.
    #[must_use]
    pub const fn narrow(self, wanted: Self) -> Option<Self> {
        if self.contains(wanted) {
            Some(wanted)
        } else {
            None
        }
    }
}

/// Identifies a kernel object that capabilities refer to.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct ObjectId(pub u64);

/// A capability: an object and the rights held on it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Capability {
    /// The object named.
    pub object: ObjectId,
    /// The rights held.
    pub rights: Rights,
}

impl Capability {
    /// A copy of this capability with `rights` in place of its own. Fails
    /// unless `rights` is a subset.
    #[must_use]
    pub fn attenuate(self, rights: Rights) -> Option<Self> {
        self.rights.narrow(rights).map(|rights| Self {
            object: self.object,
            rights,
        })
    }

    /// Whether the capability allows `needed`.
    #[must_use]
    pub const fn allows(self, needed: Rights) -> bool {
        self.rights.contains(needed)
    }
}

/// A process-local name for a capability. The kernel gives out handles, and
/// the holder passes them back on each call.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Handle(pub u32);

/// Length of an encoded token: object (8), rights (4), tag (32).
pub const TOKEN_LEN: usize = 44;

/// A capability in a form that can leave the kernel, with a tag under a key.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Token {
    capability: Capability,
    tag: [u8; 32],
}

/// Why a token was refused.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TokenError {
    /// The byte slice is not `TOKEN_LEN` long.
    BadLength,
    /// The tag does not match: the token was forged or altered.
    BadTag,
    /// The rights word sets bits this crate does not define. Such a word is
    /// refused rather than masked, so every wire form maps to one token.
    BadRights,
}

impl Token {
    /// Makes a token for `capability` under `key`.
    #[must_use]
    pub fn mint(key: &[u8; 32], capability: Capability) -> Self {
        Self {
            capability,
            tag: tag_for(key, capability),
        }
    }

    /// Checks the tag and returns the capability it names.
    pub fn verify(&self, key: &[u8; 32]) -> Result<Capability, TokenError> {
        let expected = tag_for(key, self.capability);
        if ct_eq(&expected, &self.tag) {
            Ok(self.capability)
        } else {
            Err(TokenError::BadTag)
        }
    }

    /// A token for a narrower copy of the capability, made under `key` after
    /// the original token checks out. Fails if the original is forged or the
    /// requested rights are not a subset.
    pub fn attenuate(&self, key: &[u8; 32], rights: Rights) -> Result<Self, TokenError> {
        let cap = self.verify(key)?;
        let narrowed = cap.attenuate(rights).ok_or(TokenError::BadTag)?;
        Ok(Self::mint(key, narrowed))
    }

    /// The fixed-size wire form.
    #[must_use]
    pub fn to_bytes(&self) -> [u8; TOKEN_LEN] {
        let mut out = [0u8; TOKEN_LEN];
        out[0..8].copy_from_slice(&self.capability.object.0.to_le_bytes());
        out[8..12].copy_from_slice(&self.capability.rights.bits().to_le_bytes());
        out[12..44].copy_from_slice(&self.tag);
        out
    }

    /// Parses the wire form. The tag is not checked here; call `verify`.
    pub fn from_bytes(bytes: &[u8]) -> Result<Self, TokenError> {
        if bytes.len() != TOKEN_LEN {
            return Err(TokenError::BadLength);
        }
        let mut object = [0u8; 8];
        object.copy_from_slice(&bytes[0..8]);
        let mut rights = [0u8; 4];
        rights.copy_from_slice(&bytes[8..12]);
        let rights_word = u32::from_le_bytes(rights);
        if rights_word & !Rights::ALL.bits() != 0 {
            return Err(TokenError::BadRights);
        }
        let mut tag = [0u8; 32];
        tag.copy_from_slice(&bytes[12..44]);
        Ok(Self {
            capability: Capability {
                object: ObjectId(u64::from_le_bytes(object)),
                rights: Rights::from_bits(rights_word),
            },
            tag,
        })
    }
}

const BLOCK: usize = 64;

/// HMAC-SHA256 over the fixed encoding of a capability.
fn tag_for(key: &[u8; 32], capability: Capability) -> [u8; 32] {
    let mut message = [0u8; 12];
    message[0..8].copy_from_slice(&capability.object.0.to_le_bytes());
    message[8..12].copy_from_slice(&capability.rights.bits().to_le_bytes());
    hmac_sha256(key, &message)
}

fn hmac_sha256(key: &[u8; 32], message: &[u8]) -> [u8; 32] {
    let mut block = [0u8; BLOCK];
    block[..32].copy_from_slice(key);

    let mut inner = [0u8; BLOCK + 12];
    for (dst, k) in inner.iter_mut().zip(block.iter()) {
        *dst = k ^ 0x36;
    }
    inner[BLOCK..BLOCK + message.len()].copy_from_slice(message);
    let inner_digest = sha256(&inner[..BLOCK + message.len()]);

    let mut outer = [0u8; BLOCK + 32];
    for (dst, k) in outer.iter_mut().zip(block.iter()) {
        *dst = k ^ 0x5c;
    }
    outer[BLOCK..].copy_from_slice(&inner_digest);
    sha256(&outer)
}

/// Compares two tags without an early exit on the first difference.
fn ct_eq(a: &[u8; 32], b: &[u8; 32]) -> bool {
    a.iter()
        .zip(b.iter())
        .fold(0u8, |acc, (x, y)| acc | (x ^ y))
        == 0
}

#[cfg(test)]
mod tests {
    use super::*;

    const KEY: [u8; 32] = [7; 32];

    fn cap(rights: Rights) -> Capability {
        Capability {
            object: ObjectId(0x1234),
            rights,
        }
    }

    #[test]
    fn rights_narrow_but_never_widen() {
        let rw = Rights::SEND.union(Rights::RECV);
        assert!(rw.contains(Rights::SEND));
        assert_eq!(rw.narrow(Rights::SEND), Some(Rights::SEND));
        assert_eq!(Rights::SEND.narrow(rw), None);
        assert_eq!(Rights::from_bits(u32::MAX), Rights::ALL);
        assert_eq!(rw.intersection(Rights::GRANT), Rights::NONE);
    }

    #[test]
    fn capability_attenuation_follows_rights() {
        let c = cap(Rights::SEND.union(Rights::GRANT));
        let narrow = c.attenuate(Rights::SEND).unwrap();
        assert_eq!(narrow.object, c.object);
        assert!(narrow.allows(Rights::SEND));
        assert!(!narrow.allows(Rights::GRANT));
        assert_eq!(c.attenuate(Rights::MAP), None);
    }

    #[test]
    fn token_round_trips_and_verifies() {
        let t = Token::mint(&KEY, cap(Rights::MAP));
        let bytes = t.to_bytes();
        let back = Token::from_bytes(&bytes).unwrap();
        assert_eq!(back.verify(&KEY), Ok(cap(Rights::MAP)));
        assert_eq!(Token::from_bytes(&bytes[..43]), Err(TokenError::BadLength));
    }

    #[test]
    fn altered_rights_or_object_fail_verification() {
        let t = Token::mint(&KEY, cap(Rights::SEND));
        // Widen the rights in the wire form, keeping the old tag.
        let mut bytes = t.to_bytes();
        bytes[8..12].copy_from_slice(&Rights::ALL.bits().to_le_bytes());
        let forged = Token::from_bytes(&bytes).unwrap();
        assert_eq!(forged.verify(&KEY), Err(TokenError::BadTag));
        // Change the object.
        let mut bytes = t.to_bytes();
        bytes[0] ^= 1;
        assert_eq!(
            Token::from_bytes(&bytes).unwrap().verify(&KEY),
            Err(TokenError::BadTag)
        );
    }

    #[test]
    fn wrong_key_fails_verification() {
        let t = Token::mint(&KEY, cap(Rights::SEND));
        assert_eq!(t.verify(&[8; 32]), Err(TokenError::BadTag));
    }

    #[test]
    fn attenuated_token_is_valid_and_narrower() {
        let t = Token::mint(&KEY, cap(Rights::SEND.union(Rights::RECV)));
        let child = t.attenuate(&KEY, Rights::RECV).unwrap();
        assert_eq!(child.verify(&KEY), Ok(cap(Rights::RECV)));
        // Asking for more than the token holds is refused.
        assert_eq!(t.attenuate(&KEY, Rights::GRANT), Err(TokenError::BadTag));
        // A forged token cannot be attenuated either.
        let mut bytes = t.to_bytes();
        bytes[12] ^= 1;
        let forged = Token::from_bytes(&bytes).unwrap();
        assert_eq!(
            forged.attenuate(&KEY, Rights::RECV),
            Err(TokenError::BadTag)
        );
    }

    #[test]
    fn hmac_matches_rfc4231_test_case_1() {
        // RFC 4231 test case 1: a 20-byte key of 0x0b. HMAC pads keys with
        // zeros to the block size, so the same key zero-extended to 32 bytes
        // gives the same tag.
        let mut key = [0u8; 32];
        key[..20].fill(0x0b);
        let tag = hmac_sha256(&key, b"Hi There");
        let expected: [u8; 32] = [
            0xb0, 0x34, 0x4c, 0x61, 0xd8, 0xdb, 0x38, 0x53, 0x5c, 0xa8, 0xaf, 0xce, 0xaf, 0x0b,
            0xf1, 0x2b, 0x88, 0x1d, 0xc2, 0x00, 0xc9, 0x83, 0x3d, 0xa7, 0x26, 0xe9, 0x37, 0x6c,
            0x2e, 0x32, 0xcf, 0xf7,
        ];
        assert_eq!(tag, expected);
    }
}

#[cfg(test)]
mod prop_tests {
    use super::*;

    /// Deterministic xorshift64 generator. Seeded with a constant, so every run
    /// checks the same cases. This is property-style testing, not
    /// coverage-guided fuzzing.
    struct Rng(u64);

    impl Rng {
        fn next(&mut self) -> u64 {
            let mut x = self.0;
            x ^= x << 13;
            x ^= x >> 7;
            x ^= x << 17;
            self.0 = x;
            x
        }

        fn u32(&mut self) -> u32 {
            self.next() as u32
        }

        /// A value in `0..n`, for `n > 0`.
        fn below(&mut self, n: usize) -> usize {
            (self.next() % n as u64) as usize
        }

        fn bytes(&mut self, len: usize) -> std::vec::Vec<u8> {
            (0..len).map(|_| self.next() as u8).collect()
        }
    }

    /// Cases per property. Enough to cover the input space in practice, and
    /// small enough that the suite stays fast in debug builds.
    const CASES: usize = 512;

    fn rights(rng: &mut Rng) -> Rights {
        Rights::from_bits(rng.u32())
    }

    fn key(rng: &mut Rng) -> [u8; 32] {
        let mut k = [0u8; 32];
        k.copy_from_slice(&rng.bytes(32));
        k
    }

    /// Invariant: capabilities only narrow (Phase 5, property "capability check
    /// cannot be bypassed"). Attenuation never yields a right the source did not
    /// hold.
    #[test]
    fn attenuate_never_yields_rights_outside_source() {
        let mut rng = Rng(0x9E37_79B9_7F4A_7C15);
        for _ in 0..CASES {
            let src = rights(&mut rng);
            let want = rights(&mut rng);
            let cap = Capability {
                object: ObjectId(7),
                rights: src,
            };
            match cap.attenuate(want) {
                Some(c) => {
                    assert!(src.contains(c.rights));
                    assert_eq!(c.rights, want);
                }
                None => assert!(!src.contains(want)),
            }
        }
    }

    /// Invariant: a token is bound to its key, so a holder without the key
    /// cannot re-tag it. A key differing in one byte must not verify.
    #[test]
    fn token_does_not_verify_under_a_key_differing_in_one_byte() {
        let mut rng = Rng(0xD1B5_4A32_D192_ED03);
        for _ in 0..CASES {
            let k = key(&mut rng);
            let object = rng.next();
            let r = rights(&mut rng);
            let pos = rng.below(32);
            let delta = 1 + rng.below(255) as u8;
            let t = Token::mint(
                &k,
                Capability {
                    object: ObjectId(object),
                    rights: r,
                },
            );
            let mut other = k;
            other[pos] ^= delta;
            assert_eq!(t.verify(&other), Err(TokenError::BadTag));
            assert!(t.verify(&k).is_ok());
        }
    }

    /// Invariant: tampering with a minted token is detected. Any changed wire
    /// byte either fails to parse or fails verification.
    #[test]
    fn any_single_byte_change_to_a_token_fails_verification() {
        let mut rng = Rng(0x2545_F491_4F6C_DD1D);
        for _ in 0..CASES {
            let k = key(&mut rng);
            let object = rng.next();
            let r = rights(&mut rng);
            let pos = rng.below(TOKEN_LEN);
            let delta = 1 + rng.below(255) as u8;
            let t = Token::mint(
                &k,
                Capability {
                    object: ObjectId(object),
                    rights: r,
                },
            );
            let mut bytes = t.to_bytes();
            bytes[pos] ^= delta;
            // A parse refusal is also a detection.
            if let Ok(forged) = Token::from_bytes(&bytes) {
                assert!(forged.verify(&k).is_err());
            }
        }
    }

    /// Invariant: attenuating a verified token yields a token that verifies and
    /// carries exactly the narrower rights.
    #[test]
    fn attenuated_token_verifies_and_is_a_subset() {
        let mut rng = Rng(0x6A09_E667_F3BC_C908);
        for _ in 0..CASES {
            let k = key(&mut rng);
            let object = rng.next();
            let r = rights(&mut rng);
            let mask = rng.u32();
            let t = Token::mint(
                &k,
                Capability {
                    object: ObjectId(object),
                    rights: r,
                },
            );
            let want = Rights::from_bits(r.bits() & mask);
            let child = t.attenuate(&k, want).unwrap();
            let cap = child.verify(&k).unwrap();
            assert!(r.contains(cap.rights));
            assert_eq!(cap.rights, want);
            assert_eq!(cap.object, ObjectId(object));
        }
    }

    /// Regression for a bug the property tests found. `Token::from_bytes` used
    /// to mask undefined rights bits, so a wire form with an extra bit set
    /// still verified as the same token. The fix refuses such a word. Bit 6
    /// (0x40) is undefined, since `Rights::ALL` covers bits 0 to 5.
    #[test]
    fn token_with_undefined_rights_bit_is_refused() {
        let k = [0x11u8; 32];
        let t = Token::mint(
            &k,
            Capability {
                object: ObjectId(9),
                rights: Rights::SEND,
            },
        );
        let clean = t.to_bytes();
        assert!(Token::from_bytes(&clean).unwrap().verify(&k).is_ok());

        let mut tampered = clean;
        let word = u32::from_le_bytes([tampered[8], tampered[9], tampered[10], tampered[11]]);
        tampered[8..12].copy_from_slice(&(word | 0x40).to_le_bytes());
        assert_eq!(Token::from_bytes(&tampered), Err(TokenError::BadRights));
    }
}
