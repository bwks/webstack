# Webstack

Webstack is an early-stage, Rails-inspired framework for building self-contained
Rust web applications with Axum, embedded SurrealDB, Askama, and htmx.

The project is being developed incrementally with test-driven development. See
[`PLAN.md`](PLAN.md) for milestone status and [`docs/README.md`](docs/README.md)
for framework documentation.

## Current Status

Phase 1 is complete. The framework workspace, `webstack new` application
generator, typed configuration, plain-HTTP runtime, Askama templates, and
embedded frontend assets are implemented. Generated applications own their root
route while Webstack reserves `/healthz` and `/static`.

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
isolated deployment directory containing only its binary and `webstack.toml`.
