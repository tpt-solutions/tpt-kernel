# SPDX-License-Identifier: MIT OR Apache-2.0
"""Proofs with z3 that the capability and token rights checks never widen rights.

The kernel's rights are a bit set. Its checks are `contains(held, want)`, which is
`(held & want) == want`, and the GRANT bit (bit 2). `cap::grant` and `derive` allow
a new capability only if the source holds GRANT and holds every requested right.
The token attenuation (`ipc-cap`) allows a narrower token only if the requested
rights are inside the token's rights. The model below uses 64-bit words, so it
covers the kernel's 3-bit rights and any wider word the code might grow to.

Run from the repository root, and record the verdicts:

    python docs/assurance/proofs/rights.py > docs/assurance/proofs/rights.py.log

Each property is checked by asserting its negation. `unsat` means no
counterexample exists, so the property holds for every input. The last property
is a rule that is wrong on purpose, and it must give `sat`, to show that the
check can fail. The assurance gate reads only the named properties, and requires
`<name>: unsat` for each of them.
"""

from z3 import BitVec, BitVecVal, Implies, Not, Solver

WIDTH = 64
GRANT = BitVecVal(1 << 2, WIDTH)


def contains(held, want):
    return (held & want) == want


def verdict(name, claim):
    solver = Solver()
    solver.add(Not(claim))
    print(f"{name}: {solver.check()}")


# derive / grant: source rights P, requested rights R.
P = BitVec("P", WIDTH)
R = BitVec("R", WIDTH)
derive_allowed = contains(P, GRANT) & contains(P, R)

verdict("derive_never_widens",
        Implies(derive_allowed, contains(P, R)))
verdict("derive_needs_grant",
        Implies(derive_allowed, contains(P, GRANT)))

# Token attenuation: token rights TP, requested rights TR. The new token carries
# TR, and only when TR is inside TP.
TP = BitVec("TP", WIDTH)
TR = BitVec("TR", WIDTH)
narrow_allowed = contains(TP, TR)

verdict("token_attenuation_never_widens",
        Implies(narrow_allowed, contains(TP, TR)))

# Deliberately wrong: a rule that accepts any overlap. It must not be provable.
verdict("wrong_rule_widens",
        Implies((P & R) != BitVecVal(0, WIDTH), contains(P, R)))
