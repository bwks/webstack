use std::collections::HashMap;

use axum_login::AuthnBackend;

use time::{Duration, OffsetDateTime};
use tower_sessions::{
    ExpiredDeletion, SessionStore,
    session::{Id, Record},
};
use webstack_auth::{AuthBackend, Credentials, TursoSessionStore, bootstrap_admin, hash_password};
use webstack_core::config::{AuthConfig, Environment};
use webstack_db::Database;

const AUTH_SCHEMA: &str = r"
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
";

async fn database() -> Database {
    let database = webstack_db::connect(std::path::Path::new(":memory:"))
        .await
        .expect("memory database");
    database
        .connection()
        .await
        .expect("connection")
        .execute_batch(AUTH_SCHEMA)
        .await
        .expect("authentication schema");
    database
}

#[tokio::test]
async fn development_bootstrap_is_idempotent_and_authenticates() {
    let database = database().await;
    let config = AuthConfig::default();

    bootstrap_admin(&database, &config, Environment::Development)
        .await
        .expect("first bootstrap");
    bootstrap_admin(&database, &config, Environment::Development)
        .await
        .expect("second bootstrap");

    let backend = AuthBackend::new(database);
    let user = backend
        .authenticate(Credentials {
            username: " ADMIN ".to_owned(),
            password: "changeme".to_owned(),
        })
        .await
        .expect("authentication")
        .expect("bootstrap user");
    assert_eq!(user.username(), "admin");
    assert!(user.has_role("admin"));
    assert!(user.has_role("user"));
    assert_eq!(
        user.password_expires_at(),
        OffsetDateTime::from_unix_timestamp(32_535_129_600).expect("development expiry")
    );
    assert!(
        backend
            .authenticate(Credentials {
                username: "admin".to_owned(),
                password: "wrong".to_owned(),
            })
            .await
            .expect("rejected authentication")
            .is_none()
    );
}

#[tokio::test]
async fn production_bootstrap_requires_an_immediate_password_change() {
    let database = database().await;
    let before = OffsetDateTime::now_utc();
    bootstrap_admin(&database, &AuthConfig::default(), Environment::Production)
        .await
        .expect("production bootstrap");
    let user = AuthBackend::new(database)
        .find_user("admin")
        .await
        .expect("user query")
        .expect("bootstrap user");
    assert!(user.password_expires_at() >= before - Duration::seconds(1));
    assert!(user.password_expired());
}

#[tokio::test]
async fn password_replacement_invalidates_the_old_authentication_hash() {
    let database = database().await;
    bootstrap_admin(&database, &AuthConfig::default(), Environment::Development)
        .await
        .expect("bootstrap");
    let backend = AuthBackend::new(database);
    let original = backend
        .find_user("admin")
        .await
        .expect("original user")
        .expect("user");
    let replacement = hash_password("a-long-new-password")
        .await
        .expect("replacement hash");
    let updated = backend
        .replace_password("admin", &replacement, 90)
        .await
        .expect("password replacement");
    assert_ne!(
        axum_login::AuthUser::session_auth_hash(&original),
        axum_login::AuthUser::session_auth_hash(&updated)
    );
    assert!(updated.password_expires_at() > OffsetDateTime::now_utc());
}

#[tokio::test]
async fn turso_session_store_round_trips_and_deletes_records() {
    let database = database().await;
    let store = TursoSessionStore::new(database);
    let mut record = Record {
        id: Id::default(),
        data: HashMap::new(),
        expiry_date: OffsetDateTime::now_utc() + Duration::hours(1),
    };
    record
        .data
        .insert("key".to_owned(), serde_json::json!("value"));

    store.create(&mut record).await.expect("create session");
    assert_eq!(
        store.load(&record.id).await.expect("load session"),
        Some(record.clone())
    );
    store.delete(&record.id).await.expect("delete session");
    assert!(
        store
            .load(&record.id)
            .await
            .expect("missing session")
            .is_none()
    );
}

#[tokio::test]
async fn expired_sessions_are_removed() {
    let database = database().await;
    let store = TursoSessionStore::new(database);
    let mut record = Record {
        id: Id::default(),
        data: HashMap::new(),
        expiry_date: OffsetDateTime::now_utc() - Duration::seconds(1),
    };
    store
        .create(&mut record)
        .await
        .expect("create expired session");
    store.delete_expired().await.expect("expired cleanup");
    assert!(
        store
            .load(&record.id)
            .await
            .expect("load expired")
            .is_none()
    );
}
