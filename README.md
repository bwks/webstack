# Webstack

Webstack is an early-stage, Rails-inspired framework for building self-contained
Rust web applications with Axum, embedded SurrealDB, Askama, and htmx.

The project is being developed incrementally with test-driven development. See
[`PLAN.md`](PLAN.md) for milestone status and [`docs/README.md`](docs/README.md)
for framework documentation.

## Current Status

Phases 1, 2, 4, and 5 are complete. Alongside the application generator, typed
configuration, plain-HTTP runtime, templates, and embedded assets, Webstack now
opens one embedded RocksDB-backed SurrealDB instance, applies application-owned
migrations before binding HTTP, includes database readiness in `/healthz`, and
provides local Argon2id accounts, SurrealDB sessions, CSRF enforcement, login
throttling, password expiry, role-protected routes, htmx-aware errors, and a
progressively enhanced reference CRUD feature. In-process TLS supports plain
HTTP, ephemeral self-signed development certificates, and ACME certificates
with an optional HTTP redirect listener. The remaining implementation order is
logical backup to R2, then hardening.

Generate an application from a local build with:

```sh
cargo run -p webstack-cli -- new my-app
```

## Checks

```sh
cargo fmt --check
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo test --workspace --all-features
```

The opt-in `just smoke-release` check downloads the configured frontend tools,
builds a generated application in release mode, and verifies it from an
isolated deployment directory containing its binary, `webstack.toml`, and the
complete runtime `migrations/` directory.
