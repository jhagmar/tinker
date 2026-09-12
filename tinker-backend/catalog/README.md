# Catalog

Language-agnostic problem sources compiled into the catalog binary at host start.

`tinker verify` compiles this package (`cargo build --manifest-path catalog/Cargo.toml --offline --locked`) and then Monte-Carlo-samples the in-process `tinker-catalog` registry.

## Problems

| Id | Statement |
| --- | --- |
| `int-sum` | Given an array of integers `v`, return their sum. |

## Add a problem

1. Add a module under `crates/tinker-catalog/src/problems/` that implements `Problem` and exports `ENTRY`.
2. Append that `ENTRY` to the slice in `entries()`.
3. Recompile. `tinker verify all 100` samples the new id.
