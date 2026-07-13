# Webstack

Webstack is an early-stage, Rails-inspired framework for building self-contained
Rust web applications with Axum, embedded SurrealDB, Askama, and htmx.

The project is being developed incrementally with test-driven development. See
[`PLAN.md`](PLAN.md) for milestone status and [`docs/README.md`](docs/README.md)
for framework documentation.

## Current Status

The framework workspace, `webstack new` application generator, typed
configuration, and initial plain-HTTP runtime exist. Generated applications own
their root route while Webstack reserves `/healthz`.

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
