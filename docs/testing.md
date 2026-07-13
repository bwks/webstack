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

Clocks, randomness, retry delays, external storage, and process supervision must
be injectable where nondeterminism would make tests unreliable.

## Current Contract

CLI integration tests verify Clap behavior, generate applications in temporary
directories, refuse unsafe targets, initialize Git, and compile a generated
application through the facade. Process execution is injected where Cargo or Git
failure behavior must be deterministic.

Typed-error tests verify both stable display messages and retained source chains.
HTTP milestones will test every `AppError` variant's status, safe response body,
and htmx rendering behavior.

Observability tests use scoped subscribers with captured writers so parallel
tests do not compete for global state. A dedicated integration test verifies
that a second global initialization returns a typed error.
