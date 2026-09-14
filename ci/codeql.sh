#!/bin/sh
# GitHub workflow codeql.yml. Run via: docker compose run --rm codeql
# SARIF is written to ci/out/codeql.sarif on the host.
set -eu
cd /src
mkdir -p /out
rm -rf /out/codeql-db
codeql database create /opt/codeql-db \
    --language=rust \
    --source-root=/src \
    --overwrite \
    --command='cargo build --manifest-path tinker-backend/Cargo.toml --workspace --locked'
codeql database analyze /opt/codeql-db \
    --format=sarifv2.1.0 \
    --output=/out/codeql.sarif \
    rust-code-scanning.qls
chown -R "$(stat -c '%u:%g' /src)" /out
