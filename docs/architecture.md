# Architecture

## Repository Role

This repository is the Webstack framework monorepo. It is not the source tree of
one production application. The `examples/demo` member exercises supported APIs,
while `webstack new` will generate independently owned application repositories.

## Boundaries

Generated applications consume the `webstack` facade. The facade currently
re-exports the application lifecycle type from `webstack-core`. Infrastructure
is separated into database, web, authentication, and backup crates so each can
be tested without turning a generated application into a large workspace.

The `webstack-cli` package produces the `webstack` executable. CLI scaffolds will
be embedded into that executable so generation does not depend on the framework
source checkout at runtime.

## Dependency Rules

- Generated applications depend on the facade, not internal crates.
- Internal dependencies point toward core infrastructure and remain acyclic.
- The application binary is the composition root.
- Cross-feature behavior uses narrow interfaces rather than circular imports.
- All framework crates remain private until coordinated publication is designed.

## Runtime Invariants

Each generated application is one deployable binary. It creates exactly one
embedded SurrealDB instance, and every handler receives clones of that same
thread-safe handle. Assets, templates, and migrations compile into release
binaries. Live RocksDB files are never copied for backup or opened by a second
process.

## Error Boundaries

Framework libraries expose concrete, operation-specific errors derived with
`thiserror`. They retain underlying sources where useful and do not expose
`anyhow::Error` or `Box<dyn Error>` in public APIs.

Framework and generated binaries use `anyhow` to attach process-level context at
startup and command boundaries. HTTP behavior will use Webstack's typed
`AppError`, re-exported through the facade, so status codes and htmx responses
remain explicit and testable.

Framework libraries emit structured events and spans with `tracing`. Generated
application binaries install the process-global subscriber through
`webstack::observability`; libraries never install subscribers themselves. CLI
subscriber ownership remains in the `webstack` binary.
