# Generated Applications

## Ownership

A generated application is an independent Rust binary project. Its developers
own and may edit every generated file, including routes, domain code, templates,
assets, migrations, documentation, `Cargo.toml`, and `Cargo.lock`.

Framework commands do not overwrite application-owned files or dependency
declarations. An existing generation target is always rejected, even when empty.
The generated manifest includes an empty `[workspace]` table, so the application
remains an independent Cargo workspace even when generated inside another Rust
workspace directory.

## Dependencies

During Git-first development, the generated manifest references the
`framework-baseline` branch. The first developer-run Cargo command resolves that
branch to a specific commit and creates `Cargo.lock`. Commit the lockfile with
the application so local, CI, and production builds use the same dependency
graph.

Add application-specific crates normally:

```sh
cargo add serde --features derive
cargo add reqwest --features json
```

These commands update the application manifest and lockfile. Generated
applications consume only the `webstack` facade under normal use; internal
framework crates are not stable application APIs.

## Errors

Generated binaries use `anyhow` at startup so configuration, initialization, and
server failures can carry operational context. Application HTTP handlers use
Webstack's typed `AppError` rather than returning `anyhow::Error` directly.

Domain libraries inside a larger application should define concrete errors with
`thiserror` and convert them to `AppError` at the HTTP boundary. Internal error
details are logged, not exposed in browser responses.

Register application-owned error templates with `error_renderer`. The renderer
receives an `ErrorView` containing only the status, title, safe public message,
and whether the request came from htmx. Returning `None` selects Webstack's
escaped default HTML. The framework preserves the `AppError` status and does
not rewrite unrelated responses.

## Observability

`Application::run()` loads configuration and initializes Webstack tracing before
binding the HTTP listener. The generated local configuration uses readable
info-level output; set `[observability].format = "json"` for structured output.

Application code can emit supported tracing events through
`webstack::tracing`. Do not record credentials, passwords, session identifiers,
CSRF values, complete configuration values, or user-provided sensitive content.

## Configuration and Routes

Generated applications contain an ignored `webstack.toml` and a committed
`webstack.example.toml`. Webstack always loads `./webstack.toml`; a missing file
is an error. Only `R2_ACCOUNT_ID`,
`R2_ACCESS_KEY_ID`, and `R2_SECRET_ACCESS_KEY` override values.

Webstack owns only this framework configuration. Applications may load and
manage their own configuration independently.

Applications own `/` and register routes fluently. Webstack owns `/healthz`,
which reports readiness only after a trivial query succeeds against the shared
database. `/login`, `/logout`, and `/change-password` are reserved for the
authentication runtime; applications own their GET page templates through
`auth_pages`. The generated shared Items feature demonstrates authenticated
reads and `user`-role mutations. The TLS settings select plain HTTP,
self-signed HTTPS, or ACME HTTPS at runtime. Backup settings are parsed now but
must remain disabled until that runtime milestone is implemented.

## Layout

- `src/` contains the composition root and application features.
- `assets/css`, `assets/js`, and `assets/images` contain browser assets.
- `templates/` are application-owned compile-time inputs.
- `migrations/` is the application-owned runtime history deployed beside the binary.
- `docs/` describes application-specific architecture and operations.
- `tests/` verifies the application through supported facade APIs.
- `tools/` is reserved for downloaded build tools and is not an asset directory.

Create a migration from the application root with
`webstack generate migration <lowercase_snake_case_name>`. Webstack validates
the existing history and creates the next four-digit `.surql` file atomically.
Every deployment must include the complete `migrations/` directory; startup
rejects a missing directory, changed applied migration, or incomplete history.

The generated initial migration contains Webstack's local account and session
schema. Later schema changes remain normal application-owned `.surql`
migrations: generate, commit, and deploy the complete directory with the new
binary. Migrations are intentionally not embedded in the binary.
