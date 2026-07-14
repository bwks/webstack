# Project Plan: Rust Web App (Axum + SurrealDB embedded + htmx, single binary)

## Overview
A fully self-contained, single-binary Rust web application. Server-rendered HTML
with htmx for reactivity. Embedded SurrealDB (RocksDB backend) accessed only by
this app, in-process. HTTPS terminated in-process with rustls (auto-ACME).
Templates, CSS, and JS are all compiled into the binary. The only external state
is the data directory. Periodic consistent DB exports are pushed to Cloudflare R2.

## Stack
- **Web framework:** Axum (latest stable, 0.8+)
- **HTTPS:** `axum-server` (rustls) + `rustls-acme` — no reverse proxy, no Caddy
- **Templates:** Askama (compile-time, typed)
- **Reactivity:** htmx v4 (beta accepted until stable; vendored and compiled into the binary)
- **CSS:** Tailwind CSS v4 + daisyUI via standalone Tailwind CLI (no Node), output
  compiled into the binary
- **Static assets:** `rust-embed` — disk reads in debug (hot reload), embedded in release
- **Database:** SurrealDB embedded, `surrealdb` crate with `kv-rocksdb` feature
- **Auth:** `axum-login` + `tower-sessions` + `argon2` (local accounts, RBAC; no OIDC)
- **Config:** `webstack.toml` parsed with `toml` + serde and validated with Garde
- **Backup:** SurrealDB logical export → gzip → Cloudflare R2 via `aws-sdk-s3`
- **Scheduler:** `tokio-cron-scheduler` (fallback: plain `tokio::time` loop)
- **Observability:** `tracing` + `tracing-subscriber`
- **Dev tooling:** `just` (task runner) + `bacon` (NOT cargo-watch — it is unmaintained)

## Architecture Decisions (locked in — do not revisit unless blocked)

1. **Single SurrealDB instance in app state.** Create `Surreal<Db>` once at startup,
   store in Axum `State`. Handlers clone the handle (Arc internally, thread-safe).
   Concurrent access within the app is expected and safe. Implement a small
   write-retry helper for optimistic transaction conflicts (max 3 retries,
   jittered backoff).
2. **No second process may ever open the RocksDB data dir.** All admin/backup
   operations go through the app or its export mechanism.
3. **Backups are logical exports, never file copies.** RocksDB files cannot be
   safely copied while live. Use SurrealDB export → `.surql` → gzip → R2.
4. **Everything ships in the binary.** Askama templates compile in by nature;
   CSS and htmx JS embed via `rust-embed`. Release builds MUST build Tailwind CSS
   first (enforced by `just release` ordering and a `build.rs` check that fails a
   release compile if `static/app.css` is missing).
5. **TLS in-process, three modes:** `acme` (prod), `self_signed` (dev, ephemeral
   rcgen cert generated in memory at startup), `disabled` (plain HTTP).
   Default ports: HTTPS 8443, HTTP 8080 (no elevated privileges needed).
6. **Auth is sessions, not JWT.** Server-rendered htmx app → cookie sessions via
   tower-sessions, backed by SurrealDB (custom session store — no extra infra).
   Passwords hashed with argon2id. Authorisation is role-based: `roles: [string]`
   on the user record, enforced with axum-login's permission/role layers on
   route groups. No OIDC/SSO — but keep all auth behind axum-login's
   `AuthnBackend` trait so a different backend can be swapped in later.
7. **CSRF:** double-submit cookie pattern on all state-changing routes,
   token delivered via `hx-headers` attribute on `<body>` in the base template.
8. **htmx pattern:** full-page templates + partial templates. An extractor reads
   the `HX-Request` header to decide partial vs full render. Auth failures for
   htmx requests respond with `HX-Redirect` to `/login` (a plain 302 would be
   swallowed into a partial swap).
9. **Config is boring on purpose.** One `webstack.toml`, `toml` + serde + Garde with
   `#[serde(default)]` everywhere. Exactly three env overrides, applied after
   parsing, for secrets only: `R2_ACCOUNT_ID`, `R2_ACCESS_KEY_ID`,
   `R2_SECRET_ACCESS_KEY`. Configuration always loads from `./webstack.toml`.
   Garde validation runs at startup
   with clear errors (e.g. acme mode requires domain + email).

## Crate Notes (checked July 2026)
- Do NOT use `figment` (dormant) or `cargo-watch` (unmaintained) or `config`
  (overkill here). Use `toml` + serde and `bacon`.
- `rustls-acme` is active and has an `axum-server` integration feature — use it.
  Fallbacks if blocked: `tokio-rustls-acme` fork, or `instant-acme` (lower level).
- `axum-login` 0.18+ supports axum 0.8; slow release cadence, single maintainer.
  Keep our code behind its traits; auth lives in one module.
- `tokio-cron-scheduler` is active. If it causes friction, a plain tokio sleep
  loop for the single daily backup job is an acceptable replacement.

## Rust Coding Conventions

- Declare imports only in the import section at the top of a Rust source file.
  Do not place `use` declarations inside functions or other production-code
  blocks. Imports inside `#[cfg(test)] mod tests` blocks are the only exception.
- Declare every `struct` and `enum` at module scope after imports and module
  constants, and before free functions. Never declare a `struct` or `enum`
  inside a function or other block. Place every inherent and trait `impl`
  immediately after the `struct` or `enum` it implements. Keep multiple `impl`
  blocks for the same type contiguous, with no unrelated items between the type
  and its implementations. Apply the same ordering inside test modules.
- Production Rust code must never spawn shell commands or external processes.
  Do not use `std::process::Command`, `tokio::process`, process wrapper crates,
  or equivalent APIs. Invoke developer tools from `just`, CI, or another
  developer-facing task runner instead. Test-only black-box orchestration may
  launch processes from test code.
- The repository test that scans production Rust for process-launching APIs is
  a required architecture guard. Do not weaken it, exclude crates from it, or
  bypass it to make an implementation pass.
- Add a meaningful `///` doc comment to every production function and method,
  including private functions and trait implementations. Apply the same rule
  to Rust functions emitted by the application generator. Test functions and
  test-only helpers are exempt. The source-policy test enforcing this rule is
  required and must not be weakened or bypassed.

## Project Layout
```
.
├── Cargo.toml
├── PLAN.md
├── webstack.toml             # ignored local file (example committed as webstack.example.toml)
├── justfile                  # dev / css / release / certs / backup tasks
├── bacon.toml                # bacon jobs: run, clippy, test
├── build.rs                  # rerun-if-changed static/ + templates/; fail release if app.css missing
├── tailwind/
│   ├── input.css             # tailwind + daisyui config
│   └── tailwindcss           # standalone CLI binary (gitignored, fetched by `just setup`)
├── static/
│   ├── htmx.min.js           # vendored, committed
│   └── app.css               # tailwind output (gitignored, built pre-compile)
├── templates/
│   ├── base.html             # layout: head, css/js refs, csrf hx-headers, nav
│   ├── pages/                # full-page templates (incl. login.html)
│   └── partials/             # htmx fragments
├── src/
│   ├── main.rs               # config → tracing → db → auth → router → scheduler → serve
│   ├── config.rs             # toml load, env secret overrides, validate()
│   ├── db.rs                 # Surreal init, migration runner, write-retry helper
│   ├── auth/
│   │   ├── mod.rs            # axum-login AuthnBackend impl over SurrealDB users
│   │   ├── session_store.rs  # tower-sessions SessionStore impl over SurrealDB
│   │   └── routes.rs         # login/logout handlers + templates wiring
│   ├── assets.rs             # rust-embed Asset + GET /static/{*path} handler
│   ├── tls.rs                # mode switch: acme (rustls-acme) | self_signed (rcgen) | disabled
│   ├── error.rs              # AppError -> IntoResponse (htmx-friendly fragments)
│   ├── state.rs              # AppState { db, config }
│   ├── routes/
│   │   ├── mod.rs            # router assembly, layers (sessions, auth, csrf, trace)
│   │   ├── health.rs         # GET /healthz (includes trivial DB query)
│   │   └── ...               # feature routes
│   ├── views/                # Askama template structs
│   ├── backup.rs             # export -> gzip -> R2 upload, retention pruning
│   └── middleware/
│       ├── csrf.rs
│       └── request_id.rs
├── migrations/
│   ├── 0001_init.surql       # includes user table + roles + session table
│   └── ...                   # ordered, applied at startup, tracked in _migrations table
└── tests/
    └── integration.rs        # temp data dir per test, full request cycle
```

## Key Dependencies (verify latest compatible versions at implementation time)
```toml
[dependencies]
axum = "0.8"
axum-server = { version = "0.8", features = ["tls-rustls"] }
rustls-acme = { version = "0.15", features = ["axum"] }   # check exact feature name
rcgen = "0.14"
tokio = { version = "1", features = ["full"] }
tower-http = { version = "0.6", features = ["trace", "compression-gzip"] }
axum-login = "0.18"
tower-sessions = "0.14"
argon2 = "0.5"
askama = "0.14"
rust-embed = "8"
mime_guess = "2"
surrealdb = { version = "2", features = ["kv-rocksdb"] }
serde = { version = "1", features = ["derive"] }
toml = "0.9"
tracing = "0.1"
tracing-subscriber = { version = "0.3", features = ["env-filter"] }
tokio-cron-scheduler = "0.15"
aws-sdk-s3 = "1"
aws-config = "1"
flate2 = "1"
anyhow = "1"
thiserror = "2"
```

## webstack.toml Schema
```toml
[server]
bind_addr = "0.0.0.0"
https_port = 8443
http_port = 8080
http_redirect = true        # false disables the port-8080 listener entirely

[tls]
mode = "acme"               # "acme" | "self_signed" | "disabled"
domain = "app.example.com"  # required for acme
acme_email = "you@example.com"
acme_cache_dir = "./data/acme"   # must persist across restarts (LE rate limits)
acme_staging = false        # true = Let's Encrypt staging endpoint

[database]
data_dir = "./data/surreal"
namespace = "app"
database = "app"

[auth]
session_ttl_hours = 168
# initial admin bootstrap: on first run, if no users exist, create admin
# with a generated password printed once to stdout/logs
bootstrap_admin = true

[backup]
enabled = true
cron = "0 0 3 * * *"        # UTC
retention = 14

[backup.r2]
account_id = ""             # override: env R2_ACCOUNT_ID
bucket = ""
prefix = "db-backups/"
# access_key_id / secret_access_key come ONLY from env:
# R2_ACCESS_KEY_ID, R2_SECRET_ACCESS_KEY (never stored in this file)
```
R2 endpoint: `https://{account_id}.r2.cloudflarestorage.com`, region `auto`.

## Implementation Phases

### Phase 1: Skeleton + embedded assets
- Cargo project, config module (toml + serde defaults + 3 env secret overrides
  + validate()), tracing init
- Axum router with `/healthz`; `assets.rs` with
  `#[derive(RustEmbed)] #[folder = "static/"]` and a `GET /static/{*path}`
  handler: Content-Type via `mime_guess`, ETag from embedded hash,
  `Cache-Control: public, max-age=31536000, immutable`. rust-embed handles
  debug-disk / release-embed automatically — no custom cfg branching.
- `build.rs`: rerun-if-changed on `static/` and `templates/`; fail RELEASE
  builds with a clear message if `static/app.css` is absent ("run `just css`")
- Base Askama layout referencing `/static/app.css` + `/static/htmx.min.js`
- Tooling: `just setup` (fetch tailwind standalone CLI), `just css`,
  `just dev` (bacon run + `tailwindcss --watch` concurrently),
  `just release` (css → `cargo build --release`); `bacon.toml` with run/clippy/test jobs
- **Done when:** `just dev` serves a styled page over plain HTTP, and
  `just smoke-release` proves a release binary serves its compiled templates,
  CSS, and htmx from an isolated directory containing the binary, the required
  `webstack.toml`, and the complete runtime `migrations/` history.

### Phase 3: TLS
- `tls.rs` with the three-mode switch from config:
  - `self_signed`: `rcgen::generate_simple_self_signed(["localhost"])` at startup,
    cert/key fed to rustls config in memory — no files
  - `acme`: rustls-acme with TLS-ALPN-01, DirCache at `acme_cache_dir`,
    staging flag honoured, wired into axum-server
  - `disabled`: plain HTTP on https_port (naming stays consistent)
- Optional port-8080 listener that 301s everything to https (skipped when
  `http_redirect = false` or mode = disabled)
- **Done when:** dev serves https://localhost:8443 with self-signed cert;
  acme mode verified against LE staging (document the DNS prerequisite);
  redirect listener works.

### Phase 4: Auth (authentication + authorisation)
- `0001_init.surql`: user table (username unique, argon2id password_hash,
  roles array, created_at, disabled flag) + session table
- `session_store.rs`: implement tower-sessions `SessionStore` against SurrealDB;
  expired-session cleanup piggybacks on the backup cron (or its own daily job)
- `auth/mod.rs`: axum-login `AuthnBackend` over the user table; argon2id verify
- Login page (Askama) + login/logout routes; failed-login rate limiting
  (simple in-memory per-IP/username backoff is sufficient)
- RBAC: protect route groups with role requirements
  (`login_required!` / permission layers); admin-only example route
- htmx-aware auth errors: `HX-Redirect: /login` on 401 for htmx requests
- Bootstrap: if `auth.bootstrap_admin` and zero users exist, create `admin`
  with a random password logged once
- CSRF middleware (double-submit cookie) + `hx-headers` in base template
- **Done when:** login/logout work, protected routes reject anonymous +
  wrong-role users, CSRF enforced on unsafe methods, tests cover all three.

### Phase 5: htmx patterns + example feature
- `HxRequest` extractor (reads HX-Request header); partial-vs-full rendering
- `AppError` renders htmx-friendly error fragments with correct status codes
- One reference CRUD feature (e.g. `items`): list page, create/edit/delete via
  partials, role-gated mutations — the template for all future features
- **Done when:** CRUD works with no full page reloads; pattern documented in
  a short CONVENTIONS.md.

### Phase 2: Database
- `db.rs`: open embedded Surreal at `data_dir`, select ns/db
- Migration runner: apply `migrations/*.surql` in filename order, record applied
  names in `_migrations` table, idempotent across restarts
- Write-retry helper for optimistic conflicts
- `/healthz` extended with a trivial DB query
- **Done when:** boot applies migrations idempotently; restart-safe.

### Phase 6: Backup to R2
- `backup.rs`: SurrealDB export (Rust SDK export on the embedded instance) to a
  temp file → gzip → upload to R2 key `{prefix}{iso8601-utc}.surql.gz`
- Retention: list keys under prefix, delete oldest beyond `retention`
- Scheduled via tokio-cron-scheduler from `backup.cron`; also an admin-role-only
  route to trigger an on-demand backup
- Restore path: `APP_RESTORE_FROM=<r2-key-or-local-path>` env at startup imports
  into a fresh data dir before serving; document the procedure in README
- **Done when:** scheduled backup lands in R2, retention pruning works,
  restore verified end-to-end against a throwaway bucket + fresh data dir.

### Phase 7: Hardening
- tower-http compression + trace layers, request IDs
- Graceful shutdown: SIGTERM → stop accepting, finish in-flight, close Surreal
  cleanly (scheduler shutdown too)
- Security headers middleware (HSTS when TLS active, X-Content-Type-Options,
  frame-ancestors, a sane CSP that permits inline htmx attributes)
- Integration tests: unique temp data dir per test (embedded DB cannot be shared)
- Release profile: `lto = true`, `codegen-units = 1`, `strip = true`
- systemd unit example in README (plain user service — ports 8443/8080 need no
  capabilities; note firewall NAT 443→8443 as an option)
- CI check for the single-binary claim: build release, copy the binary,
  `webstack.toml`, and `migrations/` to an empty dir, run with self_signed mode,
  assert /static/app.css and
  /static/htmx.min.js return 200
- Dockerfile (debian-slim or distroless): image = binary only + mounted data volume

## Testing Requirements
- Every integration test uses its own temp data dir
- Auth tests: anonymous rejected, wrong role rejected, CSRF-less POST rejected (403)
- Backup module tested against an S3-compatible mock or feature-gated
- TLS self_signed mode smoke-tested (client with cert verification disabled)

## Non-Goals
- OIDC / SSO (auth stays behind AuthnBackend trait for later)
- Horizontal scaling (embedded DB pins to one instance — accepted)
- WebSockets / SSE (htmx polling suffices initially)
- Node.js tooling of any kind

## Notes for LLMs
- Small compiling increments; run `cargo check` frequently; keep `bacon clippy` clean
- Askama template errors are compile errors — expect them, fix them
- Never write a script that copies the RocksDB data directory — exports only
- Do not add cargo-watch, figment, or the config crate (see Crate Notes)
- If a pinned version conflicts, prefer the latest compatible and note the change
