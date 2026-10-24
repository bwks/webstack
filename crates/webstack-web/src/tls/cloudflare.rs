use reqwest::{Client, RequestBuilder, StatusCode};
use secrecy::{ExposeSecret, SecretString};
use serde::{Deserialize, de::DeserializeOwned};
use serde_json::json;
use thiserror::Error;

const CLOUDFLARE_API_BASE: &str = "https://api.cloudflare.com/client/v4";
const TXT_RECORD_TTL_SECONDS: u32 = 60;

/// A typed Cloudflare DNS API failure.
#[derive(Debug, Error)]
pub enum CloudflareError {
    #[error("cannot reach the Cloudflare API")]
    Request(#[source] reqwest::Error),
    #[error("Cloudflare API returned {status}: {message}")]
    Api { status: StatusCode, message: String },
    #[error("no Cloudflare zone accessible to the API token contains {domain}")]
    ZoneNotFound { domain: String },
}

/// One TXT record created for an ACME DNS-01 challenge.
#[derive(Debug, Clone)]
pub(super) struct TxtRecord {
    zone_id: String,
    record_id: String,
}

#[derive(Deserialize)]
struct Envelope<T> {
    success: bool,
    #[serde(default)]
    errors: Vec<ApiMessage>,
    result: Option<T>,
}

#[derive(Deserialize)]
struct ApiMessage {
    code: i64,
    message: String,
}

#[derive(Deserialize)]
struct Identified {
    id: String,
}

/// A minimal Cloudflare client that manages ACME challenge TXT records.
pub(super) struct CloudflareDns {
    client: Client,
    api_base: String,
    token: SecretString,
}

impl CloudflareDns {
    /// Creates a client for the public Cloudflare API.
    pub(super) fn new(token: SecretString) -> Self {
        Self::with_api_base(token, CLOUDFLARE_API_BASE.to_owned())
    }

    /// Creates a client for a specific Cloudflare-compatible API base URL.
    pub(super) fn with_api_base(token: SecretString, api_base: String) -> Self {
        Self {
            client: Client::new(),
            api_base,
            token,
        }
    }

    /// Creates one short-lived TXT record in the zone that contains `name`.
    pub(super) async fn create_txt(
        &self,
        name: &str,
        content: &str,
    ) -> Result<TxtRecord, CloudflareError> {
        let zone_id = self.zone_id(name).await?;
        let record: Identified = self
            .send(
                self.client
                    .post(format!("{}/zones/{zone_id}/dns_records", self.api_base))
                    .json(&json!({
                        "type": "TXT",
                        "name": name,
                        "content": content,
                        "ttl": TXT_RECORD_TTL_SECONDS,
                    })),
            )
            .await?;
        Ok(TxtRecord {
            zone_id,
            record_id: record.id,
        })
    }

    /// Deletes one previously created challenge TXT record.
    pub(super) async fn delete_txt(&self, record: &TxtRecord) -> Result<(), CloudflareError> {
        let _deleted: Identified = self
            .send(self.client.delete(format!(
                "{}/zones/{}/dns_records/{}",
                self.api_base, record.zone_id, record.record_id
            )))
            .await?;
        Ok(())
    }

    /// Finds the most specific accessible zone that contains a DNS name.
    async fn zone_id(&self, name: &str) -> Result<String, CloudflareError> {
        for candidate in zone_candidates(name) {
            let zones: Vec<Identified> = self
                .send(
                    self.client
                        .get(format!("{}/zones", self.api_base))
                        .query(&[("name", candidate)]),
                )
                .await?;
            if let Some(zone) = zones.into_iter().next() {
                return Ok(zone.id);
            }
        }
        Err(CloudflareError::ZoneNotFound {
            domain: name.to_owned(),
        })
    }

    /// Sends one authenticated request and unwraps Cloudflare's response envelope.
    async fn send<T: DeserializeOwned>(
        &self,
        request: RequestBuilder,
    ) -> Result<T, CloudflareError> {
        let response = request
            .bearer_auth(self.token.expose_secret())
            .send()
            .await
            .map_err(CloudflareError::Request)?;
        let status = response.status();
        let envelope =
            response
                .json::<Envelope<T>>()
                .await
                .map_err(|error| CloudflareError::Api {
                    status,
                    message: format!("unreadable response: {error}"),
                })?;
        match envelope.result {
            Some(result) if envelope.success && status.is_success() => Ok(result),
            _ => Err(CloudflareError::Api {
                status,
                message: api_message(&envelope.errors),
            }),
        }
    }
}

/// Lists the DNS name and each parent domain that could be a Cloudflare zone.
fn zone_candidates(name: &str) -> Vec<&str> {
    let name = name.trim_end_matches('.');
    let mut candidates = vec![name];
    candidates.extend(
        name.match_indices('.')
            .map(|(index, _)| &name[index + 1..])
            .filter(|parent| parent.contains('.')),
    );
    candidates
}

/// Joins Cloudflare error messages for diagnostics.
fn api_message(errors: &[ApiMessage]) -> String {
    if errors.is_empty() {
        return "request was not successful".to_owned();
    }
    errors
        .iter()
        .map(|error| format!("{} (code {})", error.message, error.code))
        .collect::<Vec<_>>()
        .join("; ")
}

#[cfg(test)]
mod tests {
    use std::sync::{Arc, Mutex};

    use axum::{
        Json, Router,
        extract::{Path, Query, State},
        http::{HeaderMap, StatusCode},
        routing::{delete, get},
    };
    use secrecy::SecretString;
    use serde_json::{Value, json};
    use tokio::net::TcpListener;

    use super::{CloudflareDns, CloudflareError, zone_candidates};

    #[derive(Clone, Default)]
    struct MockState {
        records: Arc<Mutex<Vec<(String, Value)>>>,
        authorizations: Arc<Mutex<Vec<String>>>,
    }

    async fn zones(
        State(state): State<MockState>,
        headers: HeaderMap,
        Query(query): Query<Vec<(String, String)>>,
    ) -> Json<Value> {
        record_authorization(&state, &headers);
        let name = query
            .into_iter()
            .find(|(key, _)| key == "name")
            .map(|(_, value)| value)
            .unwrap_or_default();
        let result = if name == "example.com" {
            json!([{ "id": "zone-1" }])
        } else {
            json!([])
        };
        Json(json!({ "success": true, "errors": [], "result": result }))
    }

    async fn create_record(
        State(state): State<MockState>,
        headers: HeaderMap,
        Path(zone): Path<String>,
        Json(body): Json<Value>,
    ) -> (StatusCode, Json<Value>) {
        record_authorization(&state, &headers);
        if body["name"] == "_acme-challenge.denied.example.com" {
            return (
                StatusCode::FORBIDDEN,
                Json(json!({
                    "success": false,
                    "errors": [{ "code": 10000, "message": "Authentication error" }],
                    "result": null,
                })),
            );
        }
        state
            .records
            .lock()
            .expect("records lock")
            .push((zone, body));
        (
            StatusCode::OK,
            Json(json!({ "success": true, "errors": [], "result": { "id": "record-1" } })),
        )
    }

    async fn delete_record(
        State(state): State<MockState>,
        headers: HeaderMap,
        Path((zone, record)): Path<(String, String)>,
    ) -> Json<Value> {
        record_authorization(&state, &headers);
        state
            .records
            .lock()
            .expect("records lock")
            .retain(|(stored_zone, _)| !(stored_zone == &zone && record == "record-1"));
        Json(json!({ "success": true, "errors": [], "result": { "id": record } }))
    }

    fn record_authorization(state: &MockState, headers: &HeaderMap) {
        let value = headers
            .get("authorization")
            .and_then(|value| value.to_str().ok())
            .unwrap_or_default()
            .to_owned();
        state
            .authorizations
            .lock()
            .expect("authorizations lock")
            .push(value);
    }

    async fn mock_api() -> (String, MockState) {
        let state = MockState::default();
        let router = Router::new()
            .route("/zones", get(zones))
            .route(
                "/zones/{zone}/dns_records",
                axum::routing::post(create_record),
            )
            .route("/zones/{zone}/dns_records/{record}", delete(delete_record))
            .with_state(state.clone());
        let listener = TcpListener::bind("127.0.0.1:0")
            .await
            .expect("mock listener");
        let address = listener.local_addr().expect("mock address");
        tokio::spawn(async move {
            axum::serve(listener, router).await.expect("mock server");
        });
        (format!("http://{address}"), state)
    }

    #[test]
    fn zone_candidates_walk_from_most_to_least_specific() {
        assert_eq!(
            zone_candidates("_acme-challenge.app.example.com"),
            vec![
                "_acme-challenge.app.example.com",
                "app.example.com",
                "example.com"
            ]
        );
        assert_eq!(zone_candidates("example.com."), vec!["example.com"]);
    }

    #[tokio::test]
    async fn challenge_records_are_created_in_the_parent_zone_and_deleted() {
        let (api_base, state) = mock_api().await;
        let client =
            CloudflareDns::with_api_base(SecretString::from("token-value".to_owned()), api_base);

        let record = client
            .create_txt("_acme-challenge.app.example.com", "digest-value")
            .await
            .expect("created record");
        {
            let records = state.records.lock().expect("records lock");
            assert_eq!(records.len(), 1);
            assert_eq!(records[0].0, "zone-1");
            assert_eq!(records[0].1["type"], "TXT");
            assert_eq!(records[0].1["name"], "_acme-challenge.app.example.com");
            assert_eq!(records[0].1["content"], "digest-value");
        }

        client.delete_txt(&record).await.expect("deleted record");
        assert!(state.records.lock().expect("records lock").is_empty());
        assert!(
            state
                .authorizations
                .lock()
                .expect("authorizations lock")
                .iter()
                .all(|value| value == "Bearer token-value")
        );
    }

    #[tokio::test]
    async fn api_failures_and_missing_zones_are_reported() {
        let (api_base, _state) = mock_api().await;
        let client = CloudflareDns::with_api_base(SecretString::from("token".to_owned()), api_base);

        let error = client
            .create_txt("_acme-challenge.denied.example.com", "value")
            .await
            .expect_err("forbidden record");
        assert!(matches!(
            &error,
            CloudflareError::Api { status, message }
                if *status == 403 && message.contains("Authentication error")
        ));

        let error = client
            .create_txt("_acme-challenge.app.other.org", "value")
            .await
            .expect_err("unknown zone");
        assert!(matches!(error, CloudflareError::ZoneNotFound { .. }));
    }
}
