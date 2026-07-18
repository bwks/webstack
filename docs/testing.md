# Testing

## Layers

- Unit tests cover pure parsing, validation, retries, and transformations.
- Crate integration tests cover supported public APIs and HTTP behavior.
- CLI tests invoke the compiled `webstack` executable in temporary directories.
- Demo and generated-project tests ensure applications use only facade APIs.
- Process tests cover listeners, TLS, shutdown, and isolated release binaries.
- TLS integration tests exercise disabled HTTP, self-signed HTTPS, redirect
  target preservation, coordinated shutdown, and ACME configuration without
  contacting the certificate authority.

## Isolation

Every database integration test must create a unique temporary data directory.
All SurrealDB handles must be dropped before cleanup. Tests involving environment
variables must be serialized or run in isolated child processes.

Clocks, randomness, retry delays, and external storage must be injectable where
nondeterminism would make tests unreliable.

## Current Contract

CLI integration tests invoke the compiled binary, generate applications in
temporary directories, refuse unsafe targets, and compile a generated
application through the facade. Tests may invoke external processes even though
production framework code may not.

A source-policy test scans production crate sources and rejects process-launching
APIs. A second source-policy check requires documentation on every production
and generated function and method. Test directories and `#[cfg(test)]` modules
are excluded from both production policies where appropriate.

## Release Smoke Test

Run the network-dependent release acceptance check explicitly:

```sh
just smoke-release
```

The check generates an application, downloads and verifies its configured
frontend tools, builds CSS and a release executable, and starts it from an
isolated directory containing the executable, `webstack.toml`, and the complete
runtime `migrations/` directory. It then asserts that the root page, health
response, compiled CSS, and htmx JavaScript are served successfully. It is
intentionally excluded from the default test suite because it performs network
downloads and a release build.

Typed-error tests verify stable statuses, escaped safe display messages, source
redaction, full-page and htmx rendering, application renderer fallback, and
duplicate renderer rejection.

Observability tests use scoped subscribers with captured writers so parallel
tests do not compete for global state. A dedicated integration test verifies
that a second global initialization returns a typed error.
