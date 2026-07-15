-- Add Webstack local accounts and durable sessions.
DEFINE TABLE _webstack_user SCHEMAFULL;
DEFINE FIELD username ON _webstack_user TYPE string;
DEFINE FIELD password_hash ON _webstack_user TYPE string;
DEFINE FIELD roles ON _webstack_user TYPE array<string>;
DEFINE FIELD disabled ON _webstack_user TYPE bool;
DEFINE FIELD created_at ON _webstack_user TYPE int;
DEFINE FIELD password_expires_at ON _webstack_user TYPE int;
DEFINE INDEX webstack_user_username ON _webstack_user FIELDS username UNIQUE;

DEFINE TABLE _webstack_session SCHEMAFULL;
DEFINE FIELD payload ON _webstack_session TYPE string;
DEFINE FIELD expires_at ON _webstack_session TYPE int;
DEFINE INDEX webstack_session_expiry ON _webstack_session FIELDS expires_at;
