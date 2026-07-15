set shell := ["bash", "-cu"]

demo_tailwind := if os() == "windows" { "tools/tailwindcss.exe" } else { "tools/tailwindcss" }

check:
    cargo fmt --check
    cargo clippy --workspace --all-targets --all-features -- -D warnings
    cargo test --workspace --all-features

test:
    cargo test --workspace --all-features

smoke-release:
    cargo test -p webstack-cli --test release_smoke -- --ignored --nocapture

clippy:
    cargo clippy --workspace --all-targets --all-features -- -D warnings

fmt:
    cargo fmt --check

demo-setup:
    cd examples/demo && cargo run -p webstack-cli -- assets setup
    just demo-css

demo-css:
    cd examples/demo && {{demo_tailwind}} -i assets/css/input.css -o assets/css/app.css --minify

demo-css-watch:
    cd examples/demo && {{demo_tailwind}} -i assets/css/input.css -o assets/css/app.css --watch

demo-run:
    cd examples/demo && cargo run -p webstack-demo

demo-dev:
    just --parallel demo-css-watch demo-run

cli *args:
    cargo run -p webstack-cli -- {{args}}
