# Webstack Implementation Plan

Webstack is implemented in small, compiling milestones. The detailed target
architecture and locked decisions remain in [`AGENTS.md`](AGENTS.md); this file
records delivery status.

## Complete

### Phase 1: Skeleton and Embedded Assets

- Workspace crates and the supported `webstack` facade
- `webstack new` with application-owned source, templates, and configuration
- Typed `webstack.toml` loading, Garde validation, and three R2 overrides
- Tracing initialization, plain-HTTP serving, graceful shutdown, and `/healthz`
- Askama pages and application-owned assets served through `rust-embed`
- Verified Tailwind, daisyUI, and htmx v4 beta provisioning
- Generated `just` and Bacon development workflows
- Opt-in isolated release smoke coverage through `just smoke-release`

### Phase 2: Embedded Database

- One shared RocksDB-backed SurrealDB handle in application state
- Runtime `./migrations` validation, checksums, ledger tracking, and atomic application
- Bounded retries for optimistic write conflicts
- Database readiness reporting through `/healthz`
- Migration generation through `webstack generate migration`

### Phase 4: Authentication and Authorisation

- Local Argon2id accounts behind `axum-login` backend traits
- SurrealDB-backed sessions with absolute lifetime checks and daily cleanup
- Application-owned login and password-change templates
- Login/logout, mandatory password rotation, failed-login throttling, and CSRF
- Authenticated and arbitrary lowercase snake-case role route guards
- Environment-aware first-run administrator bootstrap

### Phase 5: htmx Patterns and Example Feature

- Infallible `HxRequest` extraction for full-page and partial rendering
- Typed `AppError` statuses with safe full-page and htmx fallback HTML
- Application-owned error templates through `ApplicationBuilder::error_renderer`
- Progressive-enhancement CSRF support for htmx headers and ordinary forms
- Role-gated shared Items CRUD reference feature in generated applications and the demo
- Stable region, validation-target, write-retry, and side-effect conventions

## Next

### Phase 3: TLS

Add in-process disabled, self-signed, and ACME TLS modes with HTTP redirects.

## Later

1. Phase 6: logical SurrealDB backup, R2 retention, and restore
2. Phase 7: runtime hardening, broader integration coverage, and deployment artifacts
