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

## Commands

```sh
cargo test --workspace --all-features
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo fmt --check
```

The CLI can currently display its planned command surface:

```sh
cargo run -p webstack-cli -- --help
```

Commands other than help intentionally fail until their milestones are complete.

