# Development

## Toolchain

The workspace uses the stable Rust channel selected by `rust-toolchain.toml` and
requires Rust 1.97 or newer. Clippy and rustfmt are required components.

## TDD Workflow

For observable behavior:

1. Add a focused test through the narrowest public boundary.
2. Run it and confirm that it fails for the expected reason.
3. Add the minimum implementation required to pass.
4. Refactor with the focused test and workspace suite green.
5. Update documentation in the same change.

Bug fixes begin with a reproducing test. Avoid tests coupled to private structure
when a public behavior can express the contract.

## Errors

- Library crates define typed errors with `thiserror`.
- Public library functions do not return `anyhow::Error` or `Box<dyn Error>`.
- Binaries use `anyhow::Result` or an internal `anyhow` boundary for contextual
  startup and orchestration failures.
- Preserve source errors and add variants that describe the failed operation.
- Request-handling errors use `AppError` after the HTTP layer is introduced.

## Commands

```sh
cargo test --workspace --all-features
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo fmt --check
```

The CLI uses Clap and can generate an independent application:

```sh
cargo run -p webstack-cli -- --help
cargo run -p webstack-cli -- new my-app
```

Because the CLI is the workspace's default member, the shorter forms also work
from the repository root:

```sh
cargo run -- --help
cargo run -- new my-app
```

Add `-v` for debug diagnostics or `-vv` for trace diagnostics. Without a
verbosity flag, the CLI emits only warnings and errors on stderr while keeping
normal command results on stdout.

Generation does not invoke Cargo or Git. Run the printed commands explicitly to
initialize version control and compile the application. Other advertised
commands intentionally fail until their milestones are complete.

Production Webstack code does not launch external processes. Generated
`justfile` recipes may describe Cargo, Tailwind, or Bacon commands that a
developer invokes directly. Tests may launch executables for black-box checks.
