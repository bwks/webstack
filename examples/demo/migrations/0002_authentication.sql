-- Add Webstack local accounts and durable sessions.
CREATE TABLE _webstack_user (
    username TEXT PRIMARY KEY,
    password_hash TEXT NOT NULL,
    roles TEXT NOT NULL,
    disabled INTEGER NOT NULL DEFAULT 0 CHECK (disabled IN (0, 1)),
    created_at INTEGER NOT NULL,
    password_expires_at INTEGER NOT NULL
) STRICT;

CREATE TABLE _webstack_session (
    id TEXT PRIMARY KEY,
    payload TEXT NOT NULL,
    expires_at INTEGER NOT NULL
) STRICT;
CREATE INDEX webstack_session_expiry ON _webstack_session (expires_at);
