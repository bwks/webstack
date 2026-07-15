use std::collections::HashMap;

use axum_login::AuthnBackend;
use surrealdb::{Surreal, engine::local::Mem};
use time::{Duration, OffsetDateTime};
use tower_sessions::{
    ExpiredDeletion, SessionStore,
    session::{Id, Record},
};
use webstack_auth::{
    AuthBackend, Credentials, SurrealSessionStore, bootstrap_admin, hash_password,
};
use webstack_core::config::{AuthConfig, Environment};
use webstack_db::Database;

const AUTH_SCHEMA: &str = r"
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
";

async fn database() -> Database {
    let database = Surreal::new::<Mem>(()).await.expect("memory database");
    database
        .use_ns("auth_test")
        .use_db("auth_test")
        .await
        .expect("namespace and database");
    database
        .query(AUTH_SCHEMA)
        .await
        .expect("schema query")
        .check()
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
async fn surreal_session_store_round_trips_and_deletes_records() {
    let database = database().await;
    let store = SurrealSessionStore::new(database);
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
    let store = SurrealSessionStore::new(database);
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
