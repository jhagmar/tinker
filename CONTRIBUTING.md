# Contributing

Thank you for contributing to Tinker.

## Bootstrap

Rust 1.98. The Cargo workspace is `tinker-backend/` so this tree can also hold the Mixtrapi submodule and a later SPA.

```bash
git submodule update --init --recursive
cd tinker-backend
cargo fmt --all -- --check
cargo fmt --manifest-path catalog/Cargo.toml -- --check
cargo clippy --workspace --locked --all-targets -- -D warnings
cargo clippy --manifest-path catalog/Cargo.toml --locked --all-targets -- -D warnings
cargo test --workspace --locked
python3 -m unittest discover -s languages/python -v
python3 scripts/layering.py
cargo deny check
cargo llvm-cov --workspace --locked --fail-under-lines 100 --ignore-filename-regex '/main\.rs$'
docker compose -f compose/docker-compose.yml config
cargo run -p tinker -- verify all 100
```

CI runs those commands. Line coverage on measured crates is 100%.

`crates/tinker/src/main.rs` is the process entry: environment, stdout/stderr, and `tinker::run`. Codecov omits that file; review it by reading it. Domain crates (`tinker-protocol`, `tinker-catalog`, `tinker-agent`) have no crates.io dependencies.

The Mixtrapi contract is the `mixtrapi/` submodule.

## Pull requests

1. Keep domain crates free of third-party crates and of host IO (`std::fs`, `std::net`, threads, `SystemTime`, `Instant`).
2. Add a table-driven test for each new error code a parser can emit.
3. Do not hand-edit files under `tinker-backend/generated/`.
4. Fill in the pull request template.

## Code of conduct

See [CODE_OF_CONDUCT.md](CODE_OF_CONDUCT.md).
