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

## Next

### Phase 2: In-Process TLS

Implement disabled, ephemeral self-signed, and ACME TLS modes. Add the optional
HTTP-to-HTTPS redirect listener, focused process tests, and deployment guidance.

## Later

1. Embedded SurrealDB, migrations, and write-conflict retries
2. Session authentication, RBAC, CSRF, and admin bootstrap
3. Reusable htmx full-page/partial patterns and a reference CRUD feature
4. Logical SurrealDB backup, R2 retention, and restore
5. Runtime hardening, broader integration coverage, and deployment artifacts
