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

Generation creates `./inventory-app` using deterministic filesystem operations.
The target must not already exist. Webstack then prints the explicit commands to
initialize version control and compile the application:

```sh
cd inventory-app
git init -b main
cargo check
```

The first Cargo build, check, or test creates `Cargo.lock`. Commit that file with
the application for reproducible builds.

HTTP serving is introduced in the next framework milestone; the current
application verifies composition through the Webstack facade.
