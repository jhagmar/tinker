#!/bin/sh
# GitHub workflow ci.yml. Run via: docker compose run --rm ci
set -eu
cd /src/tinker-backend
cargo fmt --all -- --check
cargo fmt --manifest-path catalog/Cargo.toml -- --check
cargo clippy --workspace --locked --all-targets -- -D warnings
cargo clippy --manifest-path catalog/Cargo.toml --locked --all-targets -- -D warnings
python3 scripts/layering.py
cargo deny check
cargo test --workspace --locked
python3 -m unittest discover -s languages/python -v
node --test scripts/test-tinker-http.mjs
cargo llvm-cov --workspace --locked --fail-under-lines 100 --ignore-filename-regex '/main\.rs$'
