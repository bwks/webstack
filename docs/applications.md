# Generated Applications

## Ownership

A generated application is an independent Rust binary project. Its developers
own and may edit every generated file, including routes, domain code, templates,
assets, migrations, documentation, `Cargo.toml`, and `Cargo.lock`.

Framework commands do not overwrite application-owned files or dependency
declarations. An existing generation target is always rejected, even when empty.

## Dependencies

During Git-first development, the generated manifest references the
`framework-baseline` branch. Cargo resolves that branch to a specific commit and
records the result in `Cargo.lock`. Commit the lockfile with the application so
local, CI, and production builds use the same dependency graph.

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

## Observability

Generated binaries initialize Webstack tracing before application composition.
The current scaffold uses readable info-level output. Typed configuration will
allow explicit JSON production output in the configuration/HTTP milestone.

Application code can emit supported tracing events through
`webstack::tracing`. Do not record credentials, passwords, session identifiers,
CSRF values, complete configuration values, or user-provided sensitive content.

## Layout

- `src/` contains the composition root and application features.
- `assets/css`, `assets/js`, and `assets/images` contain browser assets.
- `templates/` and `migrations/` are application-owned compile-time inputs.
- `docs/` describes application-specific architecture and operations.
- `tests/` verifies the application through supported facade APIs.
- `tools/` is reserved for downloaded build tools and is not an asset directory.
