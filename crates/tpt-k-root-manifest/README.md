# tpt-k-root-manifest

Policy as a manifest. An application manifest (TOML) names the objects it may use, the most rights any process may hold on each, and the grants each process asks for. `compile` turns the manifest into the grants each process would receive, refusing any grant above an object's ceiling. `lint` flags a grant that includes `grant`, and a declared object that no process is granted.

Part of tpt-k, a permissively licensed, capability-based microkernel ecosystem by TPT Solutions.

## Status

Implemented, host-tested: parsing, the ceiling check, the rights names (`read`, `write`, `grant`), duplicate and undeclared-name errors, and two lints. The lints flag a grant that includes `grant` (delegation), and an object that no process is granted. The ceiling property is checked over every pair of rights sets (`property_grants_never_exceed_the_ceiling`), and it is claim `MAN-1` in the assurance graph.

Not done: the root server does not read manifests yet, so the grants are only computed, not made. The two lints are not a full least-privilege analysis. `tpt-k-build lint-manifest --manifest <file>` runs parse, compile and lint.

## License

Dual-licensed under [MIT](../../LICENSE-MIT) OR [Apache-2.0](../../LICENSE-APACHE). See [LICENSE-POLICY.md](../../LICENSE-POLICY.md) for dependency rules and [CONTRIBUTING.md](../../CONTRIBUTING.md) for how to contribute.
