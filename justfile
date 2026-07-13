set shell := ["bash", "-cu"]

check:
    cargo fmt --check
    cargo clippy --workspace --all-targets --all-features -- -D warnings
    cargo test --workspace --all-features

test:
    cargo test --workspace --all-features

clippy:
    cargo clippy --workspace --all-targets --all-features -- -D warnings

fmt:
    cargo fmt --check

cli *args:
    cargo run -p webstack-cli -- {{args}}

