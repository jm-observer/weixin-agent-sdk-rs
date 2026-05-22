//! Fire-and-forget error notice sending.

use crate::api::client::HttpApiClient;
use crate::messaging::send::send_text;
use std::sync::Arc;

/// Types of error notices.
#[derive(Debug, Clone, Copy)]
pub enum ErrorNoticeKind {
    MediaDownload,
    CdnUpload,
    General,
}

/// Send an error notice without blocking the main flow.
/// Errors are logged via `tracing::warn` and otherwise ignored.
pub async fn send_error_notice(
    api: &Arc<HttpApiClient>,
    to: &str,
    kind: ErrorNoticeKind,
    detail: &str,
    context_token: Option<&str>,
) {
    let message = match kind {
        ErrorNoticeKind::MediaDownload => format!("[系统提示] 媒体文件处理失败: {detail}"),
        ErrorNoticeKind::CdnUpload => format!("[系统提示] 文件上传失败: {detail}"),
        ErrorNoticeKind::General => format!("[系统提示] 处理出错: {detail}"),
    };
    // fire-and-forget: ignore errors, just log.
    if let Err(e) = send_text(api, to, &message, context_token, None).await {
        tracing::warn!(to, error = %e, "failed to send error notice");
    }
}
