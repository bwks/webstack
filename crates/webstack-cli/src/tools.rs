use reqwest::Client;

const USER_AGENT: &str = concat!("webstack/", env!("CARGO_PKG_VERSION"));

pub(crate) struct HttpClient {
    client: Client,
}

impl HttpClient {
    pub(crate) fn new() -> Result<Self, reqwest::Error> {
        Ok(Self {
            client: Client::builder().user_agent(USER_AGENT).build()?,
        })
    }

    pub(crate) async fn get(&self, url: &str) -> Result<Vec<u8>, reqwest::Error> {
        Ok(self
            .client
            .get(url)
            .send()
            .await?
            .error_for_status()?
            .bytes()
            .await?
            .to_vec())
    }
}
