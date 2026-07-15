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

The generated application includes safe local configuration and serves plain
HTTP on `127.0.0.1:8080`. Its application-owned `/` route and framework-owned
`/healthz` endpoint are composed through the Webstack facade. Stop the server
with SIGINT or SIGTERM for graceful shutdown. Startup opens the configured
RocksDB directory and applies the runtime `./migrations` history before the HTTP
listener binds. Run the application from a directory containing
`webstack.toml` and the complete generated `migrations/` directory.

On the first start, development configuration creates `admin` with temporary
password `changeme`. Sign in at `/login` and replace that password before using
a production configuration. See [Authentication and
Authorisation](authentication.md) for the environment-specific behavior.
