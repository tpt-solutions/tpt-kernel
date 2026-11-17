// SPDX-License-Identifier: MIT OR Apache-2.0
//! Bounded exhaustive checks of token attenuation (`TOK-2`) over every pair of
//! rights masks the crate defines. The model is finite, so the result holds for
//! these masks and no others. The ID is the one in `docs/assurance/invariants.md`.

use super::*;

const KEY: [u8; 32] = [0x42; 32];
const OTHER_KEY: [u8; 32] = [0x43; 32];

/// Every mask made of the defined rights bits, from `NONE` to `ALL`.
const ALL_MASKS: u32 = Rights::ALL.bits();

#[test]
fn attenuation_is_exact_over_all_rights_pairs() {
    for have in 0..=ALL_MASKS {
        for want in 0..=ALL_MASKS {
            let held = Rights::from_bits(have);
            let wanted = Rights::from_bits(want);
            let cap = Capability {
                object: ObjectId(1),
                rights: held,
            };
            let subset = have & want == want;

            // Capability level: attenuation succeeds exactly when the wanted
            // rights are a subset, and the result carries exactly those rights.
            match cap.attenuate(wanted) {
                Some(narrowed) => {
                    assert!(subset, "widened {have:#x} to {want:#x}");
                    assert_eq!(narrowed.rights.bits(), want);
                }
                None => assert!(!subset, "refused a subset {want:#x} of {have:#x}"),
            }

            // Token level: the narrowed token verifies under the same key to
            // exactly those rights. A token with the wrong key never attenuates.
            let token = Token::mint(&KEY, cap);
            match token.attenuate(&KEY, wanted) {
                Ok(narrowed) => {
                    assert!(subset, "token widened {have:#x} to {want:#x}");
                    let verified = narrowed.verify(&KEY).expect("attenuated token verifies");
                    assert_eq!(verified.rights.bits(), want);
                }
                Err(_) => assert!(!subset, "token refused a subset {want:#x} of {have:#x}"),
            }
            assert!(
                token.attenuate(&OTHER_KEY, wanted).is_err(),
                "a token attenuated under a key it was not minted under"
            );
        }
    }
}
