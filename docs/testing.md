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

The first CLI integration test verifies that `webstack --help` succeeds and lists
the initial top-level commands. Later CLI milestones will test generated output
in new temporary directories and compile the resulting application.

