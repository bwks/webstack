# Testing

## Layers

- Unit tests cover pure parsing, validation, retries, and transformations.
- Crate integration tests cover supported public APIs and HTTP behavior.
- CLI tests invoke the compiled `webstack` executable in temporary directories.
- Demo and generated-project tests ensure applications use only facade APIs.
- Process tests cover listeners, TLS, shutdown, and isolated release binaries.

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
APIs. Test directories are excluded from that restriction.

Typed-error tests verify both stable display messages and retained source chains.
HTTP milestones will test every `AppError` variant's status, safe response body,
and htmx rendering behavior.

Observability tests use scoped subscribers with captured writers so parallel
tests do not compete for global state. A dedicated integration test verifies
that a second global initialization returns a typed error.
