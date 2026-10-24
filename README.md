# Webstack

Webstack is an early-stage, Rails-inspired framework for building self-contained
Rust web applications with Axum, embedded Turso, Askama, and htmx.

The project is being developed incrementally with test-driven development. See
[`PLAN.md`](PLAN.md) for milestone status and [`docs/README.md`](docs/README.md)
for framework documentation.

## Current Status

All planned phases are complete. Alongside the application generator, typed
configuration, templates, and embedded assets, Webstack opens one embedded
file-backed Turso instance, applies application-owned migrations before binding
HTTP, includes database readiness in `/healthz`, and provides local Argon2id
accounts, Turso sessions, CSRF enforcement, login throttling, password expiry,
role-protected routes, htmx-aware errors, and a progressively enhanced reference
CRUD feature. In-process TLS supports plain HTTP, ephemeral self-signed
certificates, and Let's Encrypt certificates via TLS-ALPN-01 or Cloudflare
DNS-01, with an optional HTTP redirect listener. Consistent snapshot backups go
to Cloudflare R2 on a schedule or on demand, with retention and startup restore.
The embedded database API is pinned to the stable `turso` 0.8.3 release. This branch changes the on-disk format: existing SurrealDB directories are
not imported, so configure a fresh `database.path` when upgrading.

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
complete runtime `migrations/` directory. Generated applications also include
an Alpine 3.24 Dockerfile and a hardened systemd service.
