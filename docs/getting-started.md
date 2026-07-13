# Getting Started

## Install During Git-First Development

Until Webstack crates are published, install the CLI from Git:

```sh
cargo install --git https://github.com/bwks/webstack.git \
  --branch framework-baseline webstack-cli
```

The installed executable is named `webstack`.

## Generate an Application

Application names must use lowercase kebab-case:

```sh
webstack new inventory-app
cd inventory-app
cargo test
```

Generation creates `./inventory-app`, resolves `Cargo.lock`, and initializes a
Git repository on `main`. The target must not already exist.

For offline or nested-repository workflows:

```sh
webstack new inventory-app --no-lock --no-git
```

If lockfile resolution fails, the generated source is preserved. Enter the new
directory and retry with `cargo generate-lockfile`.

HTTP serving is introduced in the next framework milestone; the current
application verifies composition through the Webstack facade.

