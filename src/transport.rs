use crate::config::Config;
use crate::record::Record;
use bytes::Bytes;
use http_body_util::{BodyExt, Full, Limited};
use hyper::{Method, Request};
use hyper_util::client::legacy::Client;
use hyper_util::client::legacy::connect::HttpConnector;
use hyper_util::rt::TokioExecutor;
use serde::Deserialize;
use std::error::Error;
use std::time::Duration;
use tokio::time;

type Result<T> = std::result::Result<T, Box<dyn Error + Send + Sync>>;

pub(crate) struct Transport {
    client: Client<HttpConnector, Full<Bytes>>,
    timeout: Duration,
}

impl Transport {
    pub(crate) fn new(config: &Config) -> Self {
        let timeout = Duration::from_millis(config.timeout_ms);
        let mut connector = HttpConnector::new();
        connector.set_connect_timeout(Some(timeout));
        let client = Client::builder(TokioExecutor::new())
            .pool_max_idle_per_host(0)
            .retry_canceled_requests(false)
            .build(connector);
        Self { client, timeout }
    }

    pub(crate) async fn get_text(&self, url: &str, limit: usize) -> Result<String> {
        let request = Request::builder()
            .method(Method::GET)
            .uri(url)
            .body(Full::new(Bytes::new()))?;
        let bytes = self.request(request, limit).await?;
        Ok(String::from_utf8(bytes.to_vec())?)
    }

    pub(crate) async fn upload(
        &self,
        primary: &str,
        fallback: &str,
        records: &[Record],
    ) -> Result<()> {
        let payload = Bytes::from(serde_json::to_vec(records)?);
        match self.post(primary, payload.clone()).await {
            Ok(()) => Ok(()),
            Err(primary_error) if primary != fallback => self
                .post(fallback, payload)
                .await
                .map_err(|error| format!("primary: {primary_error}; fallback: {error}").into()),
            Err(error) => Err(error),
        }
    }

    async fn post(&self, url: &str, payload: Bytes) -> Result<()> {
        let request = Request::builder()
            .method(Method::POST)
            .uri(url)
            .header("Content-Type", "application/json")
            .body(Full::new(payload))?;
        let bytes = self.request(request, 65_536).await?;
        let reply: Reply = serde_json::from_slice(&bytes)?;
        let code = match reply.code {
            ReturnCode::Number(code) => code,
            ReturnCode::Text(code) => code.parse()?,
        };
        if code != 0 {
            return Err(format!("receiver returnCode={code}").into());
        }
        Ok(())
    }

    async fn request(&self, request: Request<Full<Bytes>>, limit: usize) -> Result<Bytes> {
        time::timeout(self.timeout, async {
            let response = self.client.request(request).await?;
            if !response.status().is_success() {
                return Err(format!("receiver HTTP {}", response.status()).into());
            }
            let body = Limited::new(response.into_body(), limit).collect().await?;
            Ok(body.to_bytes())
        })
        .await?
    }
}

#[derive(Deserialize)]
struct Reply {
    #[serde(rename = "returnCode")]
    code: ReturnCode,
}

#[derive(Deserialize)]
#[serde(untagged)]
enum ReturnCode {
    Number(i64),
    Text(String),
}
