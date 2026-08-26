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

- One shared file-backed Turso handle in application state
- Runtime `./migrations` validation, checksums, ledger tracking, and atomic application
- Bounded retries for optimistic write conflicts
- Database readiness reporting through `/healthz`
- Migration generation through `webstack generate migration`

### Phase 4: Authentication and Authorisation

- Local Argon2id accounts behind `axum-login` backend traits
- Turso-backed sessions with absolute lifetime checks and daily cleanup
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

### Phase 3: TLS

- Added disabled HTTP, ephemeral self-signed HTTPS, and ACME HTTPS modes.
- Added optional HTTP-to-HTTPS redirects with path and query preservation.
- Added coordinated listener shutdown and secure session cookies in TLS modes.

### Phase 6: Runtime Hardening

- Configurable bounded graceful shutdown for listener and background tasks
- Trusted request IDs, structured request tracing, and gzip compression
- Strict same-origin browser security headers with TLS-only HSTS
- Optimized releases, Alpine container deployment, hardened systemd units, and CI

## Next

### Phase 7: Backup to R2

Add consistent Turso snapshot backup, R2 retention, and restore as the final phase.

### Schema Snapshot Generation

- Add `webstack generate schema` to apply the complete migration history to an
  isolated temporary Turso database through Webstack's migration runner.
- Export tables, indexes, triggers, and views from `sqlite_schema` in a
  deterministic order to an application-owned `docs/schema.sql` file.
- Mark the generated snapshot as read-only documentation; immutable migrations
  remain the authoritative runtime schema history.
- Add `webstack generate schema --check` to detect a stale snapshot without
  rewriting it.
- Add `schema` and `check-schema` recipes to newly generated application
  `justfile`s.
- Cover deterministic output, altered-schema representation, stale snapshot
  detection, and isolation from the application's runtime database in tests.
- Keep schema generation in-process; do not introduce an external `sqlite3`
  dependency or spawn another process from production Rust.
