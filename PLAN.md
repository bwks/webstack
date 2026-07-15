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

## Next

### Phase 4: Authentication and Authorisation

Add local accounts, SurrealDB-backed sessions, RBAC, CSRF protection, login and
logout routes, failed-login throttling, and first-run administrator bootstrap.

## Later

1. Phase 5: reusable htmx full-page/partial patterns and a reference CRUD feature
2. Phase 3: in-process disabled, self-signed, and ACME TLS modes with HTTP redirects
3. Phase 6: logical SurrealDB backup, R2 retention, and restore
4. Phase 7: runtime hardening, broader integration coverage, and deployment artifacts
