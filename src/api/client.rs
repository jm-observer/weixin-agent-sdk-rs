//! HTTP API client for the Weixin iLink Bot API.

use std::time::Duration;

use crate::config::WeixinConfig;
use crate::error::{Error, Result};
use crate::types::{
    CHANNEL_VERSION, DEFAULT_CONFIG_TIMEOUT_MS, GetConfigRequest, GetConfigResponse, GetUpdatesRequest,
    GetUpdatesResponse, GetUploadUrlRequest, GetUploadUrlResponse, ILINK_APP_ID, SendMessageRequest, SendTypingRequest,
    build_base_info,
};
use crate::util::redact;

/// Encode version string as `(major<<16)|(minor<<8)|patch`.
fn build_client_version(version: &str) -> u32 {
    let parts: Vec<u32> = version.split('.').filter_map(|p| p.parse().ok()).collect();
    let major = parts.first().copied().unwrap_or(0) & 0xff;
    let minor = parts.get(1).copied().unwrap_or(0) & 0xff;
    let patch = parts.get(2).copied().unwrap_or(0) & 0xff;
    (major << 16) | (minor << 8) | patch
}

/// Generate a random `X-WECHAT-UIN` header value.
fn random_wechat_uin() -> String {
    use base64::Engine;
    use rand::Rng;
    let n: u32 = rand::rng().random();
    base64::engine::general_purpose::STANDARD.encode(n.to_string().as_bytes())
}

fn ensure_trailing_slash(url: &str) -> String {
    if url.ends_with('/') {
        url.to_owned()
    } else {
        format!("{url}/")
    }
}

/// Retry budget for transient transport errors in `post_json`.
const POST_MAX_RETRIES: u32 = 2;
/// Fixed backoff between `post_json` retries.
const POST_RETRY_DELAY: Duration = Duration::from_millis(500);

/// True for transport errors worth retrying — client-side timeouts and
/// connection failures. API errors (non-2xx) and decode errors are not retried.
fn is_retryable_transport(err: &Error) -> bool {
    matches!(err, Error::Http(e) if e.is_timeout() || e.is_connect())
}

/// Business-level status fields carried in an otherwise HTTP-200 response.
const BUSINESS_CODE_FIELDS: [&str; 2] = ["ret", "errcode"];

/// The iLink Bot API signals business failures with **HTTP 200** — the real
/// status lives in the body's `ret` / `errcode`, with `errmsg` giving the
/// reason. Judging by HTTP status alone turns an explicit server-side refusal
/// ("this message was not delivered") into a silent success: the caller never
/// learns the message vanished.
///
/// A body without these fields is treated as success — some endpoints return
/// bare payloads on the happy path. A body that is not JSON at all is left to
/// the caller's own deserialization, which reports the mismatch with context.
fn check_business_code(raw: &str) -> Result<()> {
    let Ok(value) = serde_json::from_str::<serde_json::Value>(raw) else {
        return Ok(());
    };
    for field in BUSINESS_CODE_FIELDS {
        let Some(code) = value.get(field).and_then(serde_json::Value::as_i64) else {
            continue;
        };
        if code == 0 {
            continue;
        }
        let errmsg = value
            .get("errmsg")
            .and_then(serde_json::Value::as_str)
            .unwrap_or_default()
            .to_owned();
        return Err(Error::Api {
            errcode: i32::try_from(code).unwrap_or(i32::MAX),
            errmsg,
        });
    }
    Ok(())
}

/// Low-level HTTP client for all iLink Bot API endpoints.
pub struct HttpApiClient {
    base_url: String,
    token: String,
    route_tag: Option<u32>,
    api_timeout: Duration,
    http: reqwest::Client,
}

impl HttpApiClient {
    /// Create a new API client from config.
    pub fn new(config: &WeixinConfig) -> Self {
        Self {
            base_url: ensure_trailing_slash(&config.base_url),
            token: config.token.clone(),
            route_tag: config.route_tag,
            api_timeout: config.api_timeout,
            http: reqwest::Client::new(),
        }
    }

    fn common_headers(&self) -> Vec<(&'static str, String)> {
        let mut h = vec![
            ("iLink-App-Id", ILINK_APP_ID.to_owned()),
            (
                "iLink-App-ClientVersion",
                build_client_version(CHANNEL_VERSION).to_string(),
            ),
        ];
        if let Some(tag) = self.route_tag {
            h.push(("SKRouteTag", tag.to_string()));
        }
        h
    }

    fn post_headers(&self) -> Vec<(&'static str, String)> {
        let mut h = vec![
            ("Content-Type", "application/json".to_owned()),
            ("AuthorizationType", "ilink_bot_token".to_owned()),
            ("X-WECHAT-UIN", random_wechat_uin()),
        ];
        if !self.token.is_empty() {
            h.push(("Authorization", format!("Bearer {}", self.token.trim())));
        }
        h.extend(self.common_headers());
        h
    }

    /// POST JSON with bounded retries on transient transport errors
    /// (timeout / connection failures). API and decode errors are returned
    /// immediately without retry.
    async fn post_json<T: serde::de::DeserializeOwned>(
        &self,
        endpoint: &str,
        body: &impl serde::Serialize,
        timeout: Duration,
    ) -> Result<T> {
        let mut attempt = 0u32;
        loop {
            match self.post_json_once(endpoint, body, timeout).await {
                Ok(value) => return Ok(value),
                Err(err) if attempt < POST_MAX_RETRIES && is_retryable_transport(&err) => {
                    attempt += 1;
                    tracing::warn!(
                        endpoint,
                        attempt,
                        error = %err,
                        "post_json transient failure; retrying after backoff"
                    );
                    tokio::time::sleep(POST_RETRY_DELAY).await;
                }
                Err(err) => return Err(err),
            }
        }
    }

    async fn post_json_once<T: serde::de::DeserializeOwned>(
        &self,
        endpoint: &str,
        body: &impl serde::Serialize,
        timeout: Duration,
    ) -> Result<T> {
        let url = format!("{}{endpoint}", self.base_url);
        let body_str = serde_json::to_string(body)?;
        tracing::debug!(
            url = redact::redact_url(&url),
            body = redact::redact_body_default(&body_str),
            "POST"
        );

        let mut builder = self.http.post(&url).timeout(timeout).body(body_str);
        for (k, v) in self.post_headers() {
            builder = builder.header(k, v);
        }

        let response = builder.send().await?;
        let status = response.status();
        let raw = response.text().await?;
        tracing::debug!(
            status = %status,
            body = redact::redact_body_default(&raw),
            "response"
        );
        if !status.is_success() {
            return Err(Error::Api {
                errcode: i32::from(status.as_u16()),
                errmsg: raw,
            });
        }
        check_business_code(&raw)?;
        Ok(serde_json::from_str(&raw)?)
    }

    /// Long-poll `getUpdates`. On client-side timeout, returns an empty response.
    pub async fn get_updates(&self, request: &GetUpdatesRequest, timeout: Duration) -> Result<GetUpdatesResponse> {
        let url = format!("{}ilink/bot/getupdates", self.base_url);
        let body_str = serde_json::to_string(request)?;

        let mut builder = self.http.post(&url).timeout(timeout).body(body_str);
        for (k, v) in self.post_headers() {
            builder = builder.header(k, v);
        }

        match builder.send().await {
            Ok(response) => {
                let raw = response.text().await?;
                Ok(serde_json::from_str(&raw)?)
            }
            Err(e) if e.is_timeout() => {
                tracing::debug!("getUpdates: client-side timeout, returning empty response");
                Ok(GetUpdatesResponse {
                    ret: Some(0),
                    msgs: Some(Vec::new()),
                    get_updates_buf: Some(request.get_updates_buf.clone()),
                    ..Default::default()
                })
            }
            Err(e) => Err(e.into()),
        }
    }

    /// Send a message.
    pub async fn send_message(&self, request: &SendMessageRequest) -> Result<()> {
        let _: serde_json::Value = self
            .post_json("ilink/bot/sendmessage", request, self.api_timeout)
            .await?;
        Ok(())
    }

    /// Get a pre-signed CDN upload URL.
    pub async fn get_upload_url(&self, request: &GetUploadUrlRequest) -> Result<GetUploadUrlResponse> {
        self.post_json("ilink/bot/getuploadurl", request, self.api_timeout)
            .await
    }

    /// Fetch bot config (`typing_ticket`).
    pub async fn get_config(&self, user_id: &str, context_token: Option<&str>) -> Result<GetConfigResponse> {
        let body = GetConfigRequest {
            ilink_user_id: user_id.to_owned(),
            context_token: context_token.map(String::from),
            base_info: build_base_info(),
        };
        self.post_json(
            "ilink/bot/getconfig",
            &body,
            Duration::from_millis(DEFAULT_CONFIG_TIMEOUT_MS),
        )
        .await
    }

    /// Send a typing indicator.
    pub async fn send_typing(&self, request: &SendTypingRequest) -> Result<()> {
        let _: serde_json::Value = self
            .post_json(
                "ilink/bot/sendtyping",
                request,
                Duration::from_millis(DEFAULT_CONFIG_TIMEOUT_MS),
            )
            .await?;
        Ok(())
    }

    /// GET request for QR code endpoints.
    pub async fn api_get(&self, endpoint: &str, timeout: Duration) -> Result<String> {
        let url = format!("{}{endpoint}", self.base_url);
        tracing::debug!(url = redact::redact_url(&url), "GET");

        let mut builder = self.http.get(&url).timeout(timeout);
        for (k, v) in self.common_headers() {
            builder = builder.header(k, v);
        }

        let response = builder.send().await?;
        let status = response.status();
        let raw = response.text().await?;
        if !status.is_success() {
            return Err(Error::Api {
                errcode: i32::from(status.as_u16()),
                errmsg: raw,
            });
        }
        Ok(raw)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn build_client_version_encoding() {
        assert_eq!(build_client_version("2.1.1"), (2 << 16) | (1 << 8) | 1);
        assert_eq!(build_client_version("1.0.0"), 1 << 16);
        assert_eq!(build_client_version("0.0.1"), 1);
        assert_eq!(build_client_version(""), 0);
    }

    #[test]
    fn ensure_trailing_slash_adds() {
        assert_eq!(ensure_trailing_slash("https://example.com"), "https://example.com/");
    }

    #[test]
    fn ensure_trailing_slash_noop() {
        assert_eq!(ensure_trailing_slash("https://example.com/"), "https://example.com/");
    }

    #[test]
    fn random_wechat_uin_format() {
        use base64::Engine;
        let uin = random_wechat_uin();
        let decoded = base64::engine::general_purpose::STANDARD.decode(&uin).unwrap();
        let s = std::str::from_utf8(&decoded).unwrap();
        assert!(s.parse::<u32>().is_ok());
    }

    #[test]
    fn is_retryable_transport_rejects_non_transport_errors() {
        assert!(!is_retryable_transport(&Error::Api {
            errcode: 500,
            errmsg: "boom".into(),
        }));
        assert!(!is_retryable_transport(&Error::Crypto("bad".into())));
        assert!(!is_retryable_transport(&Error::Config("bad".into())));
        assert!(!is_retryable_transport(&Error::Timeout("slow".into())));
    }
}
