use std::path::Path;

use aws_sdk_s3::{
    Client,
    config::{
        BehaviorVersion, Credentials, Region, RequestChecksumCalculation,
        ResponseChecksumValidation,
    },
    primitives::ByteStream,
};
use secrecy::ExposeSecret;
use webstack_core::config::R2Config;

use crate::BackupError;

const R2_REGION: &str = "auto";

/// Object storage operations used by backup, retention, and restore.
pub(crate) trait ObjectStore {
    /// Uploads one local file to `key`.
    async fn put(&self, key: &str, path: &Path) -> Result<(), BackupError>;

    /// Lists every object key that starts with `prefix`.
    async fn list(&self, prefix: &str) -> Result<Vec<String>, BackupError>;

    /// Deletes one object.
    async fn delete(&self, key: &str) -> Result<(), BackupError>;

    /// Downloads one object's bytes.
    async fn get(&self, key: &str) -> Result<Vec<u8>, BackupError>;
}

/// A Cloudflare R2 bucket accessed through its S3-compatible API.
pub(crate) struct R2Store {
    client: Client,
    bucket: String,
}

impl R2Store {
    /// Connects to the configured R2 account and bucket.
    pub(crate) fn from_config(config: &R2Config) -> Result<Self, BackupError> {
        let endpoint = format!("https://{}.r2.cloudflarestorage.com", config.account_id);
        Self::with_endpoint(config, &endpoint)
    }

    /// Connects to an S3-compatible endpoint with R2 settings.
    pub(crate) fn with_endpoint(config: &R2Config, endpoint: &str) -> Result<Self, BackupError> {
        let credentials = config
            .credentials()
            .ok_or(BackupError::MissingCredentials)?;
        let sdk_config = aws_sdk_s3::Config::builder()
            .behavior_version(BehaviorVersion::latest())
            .region(Region::new(R2_REGION))
            .endpoint_url(endpoint)
            .force_path_style(true)
            .request_checksum_calculation(RequestChecksumCalculation::WhenRequired)
            .response_checksum_validation(ResponseChecksumValidation::WhenRequired)
            .credentials_provider(Credentials::new(
                credentials.access_key_id.expose_secret(),
                credentials.secret_access_key.expose_secret(),
                None,
                None,
                "webstack-r2",
            ))
            .build();
        Ok(Self {
            client: Client::from_conf(sdk_config),
            bucket: config.bucket.clone(),
        })
    }
}

impl ObjectStore for R2Store {
    /// Uploads one local file to the bucket.
    async fn put(&self, key: &str, path: &Path) -> Result<(), BackupError> {
        let body = ByteStream::from_path(path)
            .await
            .map_err(|error| BackupError::Storage(error.to_string()))?;
        self.client
            .put_object()
            .bucket(&self.bucket)
            .key(key)
            .content_type("application/gzip")
            .body(body)
            .send()
            .await
            .map_err(|error| storage_error(&error))?;
        Ok(())
    }

    /// Lists every key under a prefix, following pagination.
    async fn list(&self, prefix: &str) -> Result<Vec<String>, BackupError> {
        let mut pages = self
            .client
            .list_objects_v2()
            .bucket(&self.bucket)
            .prefix(prefix)
            .into_paginator()
            .send();
        let mut keys = Vec::new();
        while let Some(page) = pages.next().await {
            let page = page.map_err(|error| storage_error(&error))?;
            keys.extend(
                page.contents()
                    .iter()
                    .filter_map(|object| object.key().map(str::to_owned)),
            );
        }
        Ok(keys)
    }

    /// Deletes one object from the bucket.
    async fn delete(&self, key: &str) -> Result<(), BackupError> {
        self.client
            .delete_object()
            .bucket(&self.bucket)
            .key(key)
            .send()
            .await
            .map_err(|error| storage_error(&error))?;
        Ok(())
    }

    /// Downloads one object into memory.
    async fn get(&self, key: &str) -> Result<Vec<u8>, BackupError> {
        let object = self
            .client
            .get_object()
            .bucket(&self.bucket)
            .key(key)
            .send()
            .await
            .map_err(|error| storage_error(&error))?;
        let bytes = object
            .body
            .collect()
            .await
            .map_err(|error| BackupError::Storage(error.to_string()))?;
        Ok(bytes.into_bytes().to_vec())
    }
}

/// Formats an SDK error with its full source chain.
fn storage_error(error: &dyn std::error::Error) -> BackupError {
    let mut message = error.to_string();
    let mut source = error.source();
    while let Some(cause) = source {
        message.push_str(": ");
        message.push_str(&cause.to_string());
        source = cause.source();
    }
    BackupError::Storage(message)
}

#[cfg(test)]
pub(crate) mod tests {
    use std::{
        collections::BTreeMap,
        fmt::Write as _,
        path::Path,
        sync::{Arc, Mutex},
    };

    use axum::{
        Router,
        body::Bytes,
        extract::{Path as UrlPath, Query, State},
        http::{HeaderMap, StatusCode},
        response::IntoResponse,
        routing::get,
    };
    use secrecy::SecretString;
    use tempfile::tempdir;
    use tokio::net::TcpListener;
    use webstack_core::config::{Config, R2Config};

    use super::{ObjectStore, R2Store};
    use crate::BackupError;

    type Objects = Arc<Mutex<BTreeMap<String, Vec<u8>>>>;

    /// An in-memory object store for backup logic tests.
    #[derive(Clone, Default)]
    pub(crate) struct MemoryStore {
        pub(crate) objects: Objects,
    }

    impl ObjectStore for MemoryStore {
        async fn put(&self, key: &str, path: &Path) -> Result<(), BackupError> {
            let bytes =
                std::fs::read(path).map_err(|error| BackupError::Storage(error.to_string()))?;
            self.objects
                .lock()
                .expect("objects lock")
                .insert(key.to_owned(), bytes);
            Ok(())
        }

        async fn list(&self, prefix: &str) -> Result<Vec<String>, BackupError> {
            Ok(self
                .objects
                .lock()
                .expect("objects lock")
                .keys()
                .filter(|key| key.starts_with(prefix))
                .cloned()
                .collect())
        }

        async fn delete(&self, key: &str) -> Result<(), BackupError> {
            self.objects.lock().expect("objects lock").remove(key);
            Ok(())
        }

        async fn get(&self, key: &str) -> Result<Vec<u8>, BackupError> {
            self.objects
                .lock()
                .expect("objects lock")
                .get(key)
                .cloned()
                .ok_or_else(|| BackupError::Storage(format!("no such key {key}")))
        }
    }

    async fn put_object(
        State(objects): State<Objects>,
        UrlPath((_bucket, key)): UrlPath<(String, String)>,
        headers: HeaderMap,
        body: Bytes,
    ) -> StatusCode {
        if !headers.contains_key("authorization") {
            return StatusCode::FORBIDDEN;
        }
        objects
            .lock()
            .expect("objects lock")
            .insert(key, body.to_vec());
        StatusCode::OK
    }

    async fn get_object(
        State(objects): State<Objects>,
        UrlPath((_bucket, key)): UrlPath<(String, String)>,
    ) -> impl IntoResponse {
        match objects.lock().expect("objects lock").get(&key) {
            Some(bytes) => (StatusCode::OK, bytes.clone()).into_response(),
            None => StatusCode::NOT_FOUND.into_response(),
        }
    }

    async fn delete_object(
        State(objects): State<Objects>,
        UrlPath((_bucket, key)): UrlPath<(String, String)>,
    ) -> StatusCode {
        objects.lock().expect("objects lock").remove(&key);
        StatusCode::NO_CONTENT
    }

    async fn list_objects(
        State(objects): State<Objects>,
        UrlPath(bucket): UrlPath<String>,
        Query(query): Query<BTreeMap<String, String>>,
    ) -> impl IntoResponse {
        let prefix = query.get("prefix").cloned().unwrap_or_default();
        let mut contents = String::new();
        for (key, bytes) in objects
            .lock()
            .expect("objects lock")
            .iter()
            .filter(|(key, _)| key.starts_with(&prefix))
        {
            write!(
                contents,
                "<Contents><Key>{key}</Key><Size>{}</Size></Contents>",
                bytes.len()
            )
            .expect("write listing");
        }
        (
            [("content-type", "application/xml")],
            format!(
                "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\
                 <ListBucketResult xmlns=\"http://s3.amazonaws.com/doc/2006-03-01/\">\
                 <Name>{bucket}</Name><Prefix>{prefix}</Prefix><MaxKeys>1000</MaxKeys>\
                 <IsTruncated>false</IsTruncated>{contents}</ListBucketResult>"
            ),
        )
    }

    /// Starts a minimal path-style S3 server and returns its endpoint.
    pub(crate) async fn mock_s3() -> (String, Objects) {
        let objects = Objects::default();
        let router = Router::new()
            .route("/{bucket}", get(list_objects))
            .route("/{bucket}/", get(list_objects))
            .route(
                "/{bucket}/{*key}",
                get(get_object).put(put_object).delete(delete_object),
            )
            .with_state(Arc::clone(&objects));
        let listener = TcpListener::bind("127.0.0.1:0")
            .await
            .expect("mock listener");
        let address = listener.local_addr().expect("mock address");
        tokio::spawn(async move {
            axum::serve(listener, router).await.expect("mock S3 server");
        });
        (format!("http://{address}"), objects)
    }

    /// Returns R2 settings with test credentials for one bucket.
    pub(crate) fn r2_config(bucket: &str) -> R2Config {
        let mut config = Config::default();
        config.backup.r2.account_id = "account".to_owned();
        config.backup.r2.bucket = bucket.to_owned();
        config.backup.r2.set_credentials(
            SecretString::from("access".to_owned()),
            SecretString::from("secret".to_owned()),
        );
        config.backup.r2
    }

    #[tokio::test]
    async fn r2_store_puts_lists_gets_and_deletes_objects() {
        let (endpoint, objects) = mock_s3().await;
        let store = R2Store::with_endpoint(&r2_config("backups"), &endpoint).expect("R2 store");
        let file = tempdir().expect("temporary directory");
        let path = file.path().join("snapshot.db.gz");
        std::fs::write(&path, b"compressed snapshot").expect("snapshot file");

        store
            .put("db-backups/20261024T030000Z.db.gz", &path)
            .await
            .expect("uploaded object");
        store
            .put("other/ignored.txt", &path)
            .await
            .expect("uploaded unrelated object");

        assert_eq!(
            store.list("db-backups/").await.expect("listed objects"),
            vec!["db-backups/20261024T030000Z.db.gz"]
        );
        assert_eq!(
            store
                .get("db-backups/20261024T030000Z.db.gz")
                .await
                .expect("downloaded object"),
            b"compressed snapshot"
        );
        store
            .delete("db-backups/20261024T030000Z.db.gz")
            .await
            .expect("deleted object");
        assert_eq!(
            objects
                .lock()
                .expect("objects lock")
                .keys()
                .collect::<Vec<_>>(),
            vec!["other/ignored.txt"]
        );
    }

    #[test]
    fn r2_store_requires_credentials() {
        let mut config = Config::default();
        config.backup.r2.bucket = "backups".to_owned();
        assert!(matches!(
            R2Store::from_config(&config.backup.r2),
            Err(BackupError::MissingCredentials)
        ));
    }
}
