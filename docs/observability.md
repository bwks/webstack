# Observability

## Architecture

Webstack uses `tracing` for structured events and spans. Libraries emit events;
the application or CLI binary owns subscriber initialization. Logs are written
to stderr so command results and HTTP response bodies remain separate.

Generated applications initialize tracing through the supported facade:

```rust
webstack::observability::init(&ObservabilityConfig::default())?;
```

Application code can emit events without depending on an internal crate:

```rust
webstack::tracing::info!(item_id = %item.id, "item created");
```

## Filtering and Formats

The current default is an `info` filter with compact, human-readable output and
terminal-aware ANSI styling. JSON output is newline-delimited, disables ANSI,
and includes event fields plus current span context.

The configuration milestone will expose:

```toml
[observability]
filter = "info"
format = "pretty" # "pretty" or "json"
```

`RUST_LOG` is intentionally unsupported. Filters come from typed TOML so the
project preserves its rule that only the three R2 secrets override configuration
values through environment variables.

## CLI Diagnostics

- No verbosity flag: warning and error events only.
- `-v`: debug diagnostics.
- `-vv`: trace diagnostics.

Normal results remain on stdout. Diagnostics and contextual errors use stderr.

## Field Conventions

- Use stable snake_case field names such as `request_id`, `item_id`, and
  `migration_name`.
- Put identifiers and measurable values in fields; keep messages short and
  stable.
- Use spans for operations with duration or nested work.
- Never log passwords, credential material, session or CSRF tokens, complete
  configuration structures, or sensitive user content.
- Log internal error source chains at operational boundaries; return safe error
  messages to users.

Log persistence, rotation, shipping, and retention belong to the deployment
environment rather than the Webstack process.
