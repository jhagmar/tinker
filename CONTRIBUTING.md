# Contributing

Thank you for contributing to Tinker.

## Bootstrap

Rust 1.98. The Cargo workspace is `tinker-backend/` so this tree can also hold the Mixtrapi submodule and a later SPA.

```bash
git submodule update --init --recursive
cd tinker-backend
cargo fmt --all -- --check
cargo clippy --workspace --locked --all-targets -- -D warnings
cargo test --workspace --locked
python3 scripts/layering.py
cargo deny check
cargo llvm-cov --workspace --locked --fail-under-lines 100
docker compose -f compose/docker-compose.yml config
```

CI runs those commands. Line coverage on measured crates is 100%.

`crates/tinker/src/main.rs` is the process entry: it prints the lib version and exits. Codecov omits that file; review it by reading it. Domain crates (`tinker-protocol`, `tinker-catalog`, `tinker-agent`) have no crates.io dependencies.

The Mixtrapi contract is the `mixtrapi/` submodule.

## Pull requests

1. Keep domain crates free of third-party crates and of host IO (`std::fs`, `std::net`, threads, `SystemTime`, `Instant`).
2. Add a table-driven test for each new error code a parser can emit.
3. Do not hand-edit files under `tinker-backend/generated/`.
4. Fill in the pull request template.

## GitHub

On github.com/jhagmar/tinker, enable:

- Branch protection on `master` with required check `test`
- CodeQL and Scorecard workflow permissions (read contents, write `security-events` and `id-token` as in the workflow files)
- Codecov for the repository so the coverage badge resolves
- Private vulnerability reporting

## Code of conduct

See [CODE_OF_CONDUCT.md](CODE_OF_CONDUCT.md).
