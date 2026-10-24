# Backup and restore

Webstack backs up the embedded database to a Cloudflare R2 bucket. Each backup
is a consistent snapshot taken through the running application, compressed
with gzip, and uploaded under a timestamped key. Older backups beyond the
retention count are deleted after each successful upload.

## Configuration

```toml
[backup]
enabled = true
cron = "0 0 3 * * *"        # UTC; seconds field optional
retention = 14

[backup.r2]
account_id = ""             # or R2_ACCOUNT_ID
bucket = "my-app-backups"
prefix = "db-backups/"
```

R2 credentials come only from the environment:

```sh
R2_ACCESS_KEY_ID=...
R2_SECRET_ACCESS_KEY=...
```

Create an R2 API token with **Object Read & Write** permission limited to the
backup bucket. `R2_ACCOUNT_ID` overrides `backup.r2.account_id` when set.
Startup validates the cron expression, retention, bucket, account, and
credentials when backups are enabled.

## How a backup runs

1. `VACUUM INTO` writes a compacted copy of the live database into
   `.webstack-backup/` beside the database file. The copy is consistent even
   while requests are writing; the live database and WAL files are never copied
   directly.
2. The copy is gzip-compressed and the uncompressed snapshot is deleted.
3. The archive is uploaded to `{prefix}{YYYYMMDD}T{HHMMSS}Z.db.gz` and the local
   archive is deleted.
4. Objects directly under `prefix` that match the timestamped name are sorted,
   and the oldest beyond `retention` are deleted. Other objects under the prefix
   are never touched. A pruning failure is logged without failing the backup.

Only one backup runs at a time; a second request waits for the first.

## Scheduled and on-demand backups

When `backup.enabled = true`, a background task runs a backup at each `cron`
occurrence, in UTC. Failures are logged and the next occurrence still runs.

Administrators can also start a backup with an authenticated, CSRF-protected
`POST /admin/backups`. The route exists only when backups are enabled and
returns JSON:

```json
{ "status": "ok", "key": "db-backups/20261024T030000Z.db.gz" }
```

On failure it returns status 500 with `{"status": "error"}`; details are in the
server log. `/admin/backups` is reserved and cannot be registered by
applications. A button can call it with htmx, for example
`hx-post="/admin/backups"`.

## Restore

Restore runs at startup, before the database opens. Stop the application, move
the existing database files aside, and start it with `APP_RESTORE_FROM`:

```sh
mv data/app.db data/app.db.old
mv data/app.db-wal data/app.db-wal.old 2>/dev/null
APP_RESTORE_FROM=db-backups/20261024T030000Z.db.gz ./my-app
```

- If a local file exists at the given path, it is restored from disk;
  otherwise the value is treated as an R2 object key, which requires
  `backup.r2.account_id`, `backup.r2.bucket`, and R2 credentials (even when
  scheduled backups are disabled).
- Gzip archives are detected and decompressed automatically; an uncompressed
  database file also works.
- The restored file must pass `PRAGMA integrity_check` before it replaces the
  database path. Migrations then run as usual.
- Webstack refuses to restore over an existing database. Unset
  `APP_RESTORE_FROM` after a successful restore, or the next start fails.

Verify a restore against a throwaway data directory before you need it in an
emergency.
