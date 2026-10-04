# Tinker

Tinker is an operator-run site for working through coding problems in the browser. One Rust binary serves the public SPA, a separate admin SPA, Mixtrapi session channels, and Docker sandboxes.

The wire contract is the [Mixtrapi](mixtrapi/README.md) submodule (`mixtrapi/1`). This repository is the host: `tinker-backend` (binary `tinker`) and, later, `tinker-spa`.

[![CI](https://github.com/jhagmar/tinker/actions/workflows/ci.yml/badge.svg)](https://github.com/jhagmar/tinker/actions/workflows/ci.yml)
[![codecov](https://codecov.io/gh/jhagmar/tinker/graph/badge.svg)](https://codecov.io/gh/jhagmar/tinker)
[![OpenSSF Scorecard](https://api.scorecard.dev/projects/github.com/jhagmar/tinker/badge)](https://scorecard.dev/viewer/?uri=github.com/jhagmar/tinker)
[![REUSE status](https://api.reuse.software/badge/github.com/jhagmar/tinker)](https://api.reuse.software/info/github.com/jhagmar/tinker)
[![License: MIT](https://img.shields.io/badge/License-MIT-blue.svg)](LICENSE)
[![Contributor Covenant](https://img.shields.io/badge/Contributor%20Covenant-2.1-4baaaa.svg)](CODE_OF_CONDUCT.md)

## Bootstrap

Rust 1.98. From a clone:

```bash
git submodule update --init --recursive
cd tinker-backend
cargo test --workspace --locked
python3 -m unittest discover -s languages/python -v
node --test scripts/test-tinker-http.mjs
docker compose -f compose/docker-compose.yml config
```

`cargo test --workspace --locked` is the default Rust test command. Line coverage on measured crates is 100%. Python 3.12 or later: `python3 -m unittest discover -s languages/python -v` from `tinker-backend/`. Node 18 or later: `node --test scripts/test-tinker-http.mjs` from `tinker-backend/` imports `generated/tinker-http.js`.

With Docker running, from this directory: `docker compose run --rm ci` and `docker compose run --rm codeql`. CodeQL writes `ci/out/codeql.sarif`.

From `tinker-backend/`, `cargo run -p tinker -- verify all 100` compiles `catalog/` and samples every problem. Set `TINKER_CATALOG_DIR` when the process working directory is not the workspace root. `cargo run -p tinker -- codegen generated/tinker-http.js` writes the HTTP JavaScript helpers.

`tinker hash-password` prints an argon2id hash (`$argon2id$v=19$…`) from a TTY prompt or `--password-file`. `tinker orchestrate` binds the public and admin listeners. It refuses to start without `TINKER_ADMIN_PASSWORD_HASH` (or `_FILE`, or TOML `admin_password_hash`) and `TINKER_JWT_HS256_SECRET` (or `_FILE`, or TOML `jwt_hs256_secret`). Listen addresses default to `0.0.0.0:8080` and `127.0.0.1:8081`. Pass `--config path.toml` or set `TINKER_PUBLIC_LISTEN` / `TINKER_ADMIN_LISTEN`. A plaintext admin password field is a config error.

After start, `POST /v1/access-requests` on the public port, `POST /v1/login` then `POST /v1/access-requests/{id}/approve` on the admin port, and the waiter receives a JWT with `session_id`, `user_id`, and `workspace_id`.

## Layout

| Path | Role |
| --- | --- |
| `mixtrapi/` | Protocol spec, schemas, and JavaScript helpers (git submodule) |
| `tinker-backend/` | Cargo workspace: host binary `tinker`, protocol, catalog, agent |
| `tinker-spa/` | Visitor and operator SPAs (placeholders until that work starts) |

The Cargo workspace lives under `tinker-backend` so Mixtrapi and a later SPA keep their own package roots.

## Security

See [SECURITY.md](SECURITY.md).

## License

MIT. Copyright (c) 2026 Jonas Hagmar.
