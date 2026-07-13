# Webstack Framework Plan

## Product Goal

Webstack is a Rails-inspired Rust framework for building independent,
single-binary web applications. The framework monorepo supplies reusable crates,
conventions, a CLI, a test harness, and a complete demo application.

Generated applications own their routes, domain code, Askama templates, assets,
migrations, configuration, documentation, and tests. They normally depend only
on the `webstack` facade crate.

## Working Agreement

Development is incremental and test-driven:

1. Agree on the next milestone's behavior and design choices.
2. Write a failing test for the next observable behavior.
3. Implement the minimum behavior needed to pass.
4. Refactor while keeping tests green.
5. Run the workspace quality gate and update documentation.
6. Stop for user review before starting another milestone.

Every bug fix starts with a regression test. Framework upgrades never overwrite
application-owned files silently.

## Framework Workspace

- `webstack`: supported facade and prelude for generated applications.
- `webstack-core`: application builder, configuration, lifecycle, and shutdown.
- `webstack-db`: embedded SurrealDB, migrations, and conflict retries.
- `webstack-web`: Axum, TLS, htmx, Askama, assets, errors, and middleware.
- `webstack-auth`: users, sessions, RBAC, rate limiting, and CSRF.
- `webstack-backup`: logical export/restore, R2, retention, and scheduling.
- `webstack-cli`: the `webstack` command and embedded application scaffold.
- `webstack-test`: supported integration harness for generated applications.
- `examples/demo`: an application built exclusively through supported APIs.

All packages are private during Git-first development. The facade becomes the
primary compatibility boundary before coordinated crates.io publication.

## Generated Application Convention

A generated application contains `src/`, `templates/`, `migrations/`, `docs/`,
and `tests/`, plus:

```text
assets/
├── css/
│   ├── input.css
│   └── app.css
├── js/
│   └── htmx.min.js
└── images/
```

The standalone Tailwind executable lives at `tools/tailwindcss`. Static URLs
preserve the asset hierarchy: `/static/css/app.css`,
`/static/js/htmx.min.js`, and `/static/images/<path>`.

## Delivery Milestones

### 0. Workspace Baseline (complete)

- Stable toolchain, Cargo workspace, private crate boundaries, CLI shell, demo,
  test conventions, initial documentation, and quality commands.
- **Gate:** workspace format, Clippy, and tests pass.

### 1. Application Generation (in review)

- Test and implement `webstack new <name>` in fresh temporary directories.
- Generate an independently compiling application with Git framework dependency,
  nested assets, docs, configuration example, and optional Git initialization.
- Use Clap for typed commands, generated help, and argument validation. Resolve
  and commit application lockfiles by default, with an offline opt-out.
- Establish structured tracing, typed observability configuration, CLI verbosity,
  and generated-application startup logging before runtime features are added.
- **Gate:** generated project compiles without relying on monorepo path state.

### 2. Configuration and HTTP Runtime

- Test and implement typed TOML configuration, path precedence, the three R2
  secret overrides, tracing, `/`, and database-free `/healthz`.
- **Gate:** generated application boots and responds through the facade API.

### 3. Assets, Templates, and Development Command

- Test and implement Askama pages, embedded nested assets, ETags, caching,
  Tailwind/daisyUI tooling, and `webstack dev` child supervision.
- **Gate:** isolated release binary serves CSS, htmx, images, and templates.

### 4. TLS

- Test and implement disabled, ephemeral self-signed, redirect, and ACME modes.
- **Gate:** local TLS tests pass and ACME staging procedure is documented.

### 5. Embedded Database

- Test and implement one SurrealDB handle, embedded immutable migrations,
  checksums, health query, and three-attempt jittered conflict retry.
- **Gate:** fresh boot and restarts apply migrations safely and idempotently.

### 6. Authentication and Request Security

- Test and implement users, SurrealDB sessions, Argon2id, bootstrap admin, login,
  logout, RBAC, login backoff, htmx redirects, and double-submit CSRF.
- **Gate:** auth, role, session, rate-limit, and CSRF integration tests pass.

### 7. htmx Conventions and Reference Feature

- Test and implement full/partial rendering, typed errors, validation, and a
  complete application-owned `items` CRUD feature.
- **Gate:** normal navigation and htmx flows share business logic and pass tests.

### 8. Backup and Restore

- Test and implement logical export, gzip, R2, retention, overlap prevention,
  scheduling, admin trigger, and startup-only `--restore-from`.
- **Gate:** mock S3 tests and export/import round trip pass.

### 9. Hardening and Distribution

- Test and implement request IDs, compression, security headers, shutdown, CI,
  Docker, systemd, documentation, and publication preparation.
- **Gate:** a generated release binary passes the isolated-directory test.

Model/controller/scaffold generators are deferred until the reference feature
establishes stable application conventions.

## Initial CLI Contract

- `webstack new <name>`
- `webstack dev`
- `webstack assets setup`
- `webstack assets build`
- `webstack generate migration <name>`
- `webstack doctor`

Until its milestone is implemented, a command must fail clearly rather than
pretend to perform work.

Generated applications own their `Cargo.toml` and `Cargo.lock`. Developers add
dependencies with normal Cargo workflows; framework tooling does not overwrite
application dependency declarations.

## Invariants

- Generated applications are independent projects and single release binaries.
- One process owns one embedded SurrealDB instance; no other tool opens or copies
  its live RocksDB directory.
- Backups are logical exports.
- Templates, migrations, CSS, JavaScript, and images embed into release binaries.
- `APP_CONFIG` selects configuration. Only `R2_ACCOUNT_ID`,
  `R2_ACCESS_KEY_ID`, and `R2_SECRET_ACCESS_KEY` override config values.
- Restore is a startup-only CLI operation and requires a fresh data directory.
- Node.js, `cargo-watch`, Figment, and the `config` crate are out of scope.
- Library APIs return concrete errors derived with `thiserror`; `anyhow` is
  reserved for binary entrypoints and process-level context.
- Generated application startup uses `anyhow`, while HTTP handlers use the
  facade's typed `AppError` once the response layer is introduced.

## Quality Gate

```sh
cargo fmt --check
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo test --workspace --all-features
```

Applicable milestones also run generated-project, HTTP, TLS, documentation-link,
and isolated-release tests. Database tests receive unique temporary directories
and drop all handles before cleanup.
