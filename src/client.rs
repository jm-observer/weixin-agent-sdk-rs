//! SDK entry point: [`WeixinClient`] and its builder.

use std::path::Path;
use std::sync::Arc;

use tokio_util::sync::CancellationToken;

use crate::api::client::HttpApiClient;
use crate::api::config_cache::ConfigCache;
use crate::api::session_guard::SessionGuard;
use crate::config::WeixinConfig;
use crate::error::{Error, Result};
use crate::messaging::inbound::{ContextTokenStore, SendResult};
use crate::monitor::poll_loop::MessageHandler;
use crate::qr_login::login::QrLoginApi;

/// The main SDK client.
pub struct WeixinClient {
    config: Arc<WeixinConfig>,
    handler: Arc<dyn MessageHandler>,
    debug_mode: Arc<crate::messaging::debug_mode::DebugMode>,
    api: Arc<HttpApiClient>,
    session_guard: Arc<SessionGuard>,
    config_cache: Arc<ConfigCache>,
    context_tokens: Arc<ContextTokenStore>,
    cancel: CancellationToken,
}

/// Builder for [`WeixinClient`].
#[must_use]
pub struct WeixinClientBuilder {
    config: WeixinConfig,
    handler: Option<Arc<dyn MessageHandler>>,
}

impl WeixinClient {
    /// Create a new builder.
    pub fn builder(config: WeixinConfig) -> WeixinClientBuilder {
        WeixinClientBuilder { config, handler: None }
    }

    /// Start the long-poll monitor loop. Blocks until shutdown.
    ///
    /// `initial_sync_buf` should be loaded from your persistence layer (or `None` for fresh start).
    pub async fn start(&self, initial_sync_buf: Option<String>) -> Result<()> {
        crate::monitor::poll_loop::run_monitor(
            Arc::clone(&self.api),
            self.config.cdn_base_url.clone(),
            Arc::clone(&self.handler),
            Arc::clone(&self.session_guard),
            Arc::clone(&self.config_cache),
            Arc::clone(&self.context_tokens),
            Arc::clone(&self.debug_mode),
            initial_sync_buf,
            self.config.long_poll_timeout,
            self.cancel.clone(),
        )
        .await
    }

    /// Gracefully shut down the monitor loop.
    pub fn shutdown(&self) {
        self.cancel.cancel();
    }

    /// Send a text message to a user.
    ///
    /// `client_id` supplies a stable idempotency key (the iLink server
    /// de-duplicates by it); `None` generates a fresh one.
    pub async fn send_text(
        &self,
        to: &str,
        text: &str,
        context_token: Option<&str>,
        client_id: Option<&str>,
    ) -> Result<SendResult> {
        let filtered = if self.config.filter_markdown {
            crate::messaging::markdown_filter::StreamingMarkdownFilter::filter(text)
        } else {
            text.to_string()
        };
        crate::messaging::send::send_text(&self.api, to, &filtered, context_token, client_id).await
    }

    /// Send a media file to a user.
    ///
    /// `client_id` supplies a stable idempotency key; `None` generates one.
    pub async fn send_media(
        &self,
        to: &str,
        file_path: &Path,
        context_token: Option<&str>,
        client_id: Option<&str>,
    ) -> Result<SendResult> {
        crate::messaging::send_media::send_media_file(
            &self.api,
            &self.config.cdn_base_url,
            to,
            file_path,
            "",
            context_token,
            client_id,
        )
        .await
    }

    /// Send a remote media URL: download to temp, send, then clean up.
    ///
    /// `client_id` supplies a stable idempotency key; `None` generates one.
    pub async fn send_remote_media(
        &self,
        to: &str,
        url: &str,
        context_token: Option<&str>,
        client_id: Option<&str>,
    ) -> Result<SendResult> {
        // Download to temporary location.
        let temp_path = crate::media::remote_download::download_remote_file_to_temp(url).await?;
        // Send the media.
        let result = self.send_media(to, &temp_path, context_token, client_id).await;
        // Clean up temporary file regardless of send result.
        let _ = tokio::fs::remove_file(&temp_path).await;
        result
    }

    /// Get a QR login API handle.
    pub fn qr_login(&self) -> QrLoginApi<'_> {
        QrLoginApi::new(&self.api)
    }

    /// Access the context token store (for export/import).
    pub fn context_tokens(&self) -> &ContextTokenStore {
        &self.context_tokens
    }
}

impl WeixinClientBuilder {
    /// Set the message handler.
    pub fn on_message(mut self, handler: impl MessageHandler + 'static) -> Self {
        self.handler = Some(Arc::new(handler));
        self
    }

    /// Build the client.
    pub fn build(self) -> Result<WeixinClient> {
        let handler = self
            .handler
            .ok_or_else(|| Error::Config("message handler is required".into()))?;
        let api = Arc::new(HttpApiClient::new(&self.config));
        let config_cache = Arc::new(ConfigCache::new(Arc::clone(&api)));
        Ok(WeixinClient {
            debug_mode: Arc::new(crate::messaging::debug_mode::DebugMode::new()),
            config: Arc::new(self.config),
            handler,
            api,
            session_guard: Arc::new(SessionGuard::new()),
            config_cache,
            context_tokens: Arc::new(ContextTokenStore::new()),
            cancel: CancellationToken::new(),
        })
    }
}
