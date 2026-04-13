# Plan 03: 错误通知发送 (Error Notice)

## 目标

实现向用户发送错误通知消息的能力，采用 fire-and-forget 模式（不阻塞主流程，失败仅记录日志）。

## 背景

TS 版本在 `src/messaging/error-notice.ts` 中实现了 `sendWeixinErrorNotice()`，当以下场景出现时向用户发送友好的错误提示：

- 媒体下载/解密失败
- CDN 上传失败
- 通用处理错误

这些通知采用 fire-and-forget 模式：即使发送失败也不抛异常，仅记录日志。目的是在出错时给用户一个反馈，避免用户等待无响应。

## 实现方式

### 新建文件

`src/messaging/error_notice.rs`

### 核心接口

```rust
use crate::api::client::HttpApiClient;
use std::sync::Arc;

/// 错误通知类型
pub enum ErrorNoticeKind {
    /// 媒体下载失败
    MediaDownload,
    /// CDN 上传失败
    CdnUpload,
    /// 通用错误
    General,
}

/// 向用户发送错误通知（fire-and-forget）
///
/// 发送失败仅记录日志，不返回错误。
pub async fn send_error_notice(
    api: &Arc<HttpApiClient>,
    to: &str,
    kind: ErrorNoticeKind,
    detail: &str,
    context_token: Option<&str>,
) {
    let message = match kind {
        ErrorNoticeKind::MediaDownload => {
            format!("[系统提示] 媒体文件处理失败: {detail}")
        }
        ErrorNoticeKind::CdnUpload => {
            format!("[系统提示] 文件上传失败: {detail}")
        }
        ErrorNoticeKind::General => {
            format!("[系统提示] 处理出错: {detail}")
        }
    };

    if let Err(e) = send_text(api, to, &message, context_token).await {
        tracing::warn!(to, error = %e, "failed to send error notice");
    }
}
```

### 集成点

1. 在 `MessageContext` 上添加便捷方法：

```rust
impl MessageContext {
    pub async fn send_error_notice(&self, kind: ErrorNoticeKind, detail: &str) {
        error_notice::send_error_notice(
            &self.sender.api,
            &self.from,
            kind,
            detail,
            self.context_token.as_deref(),
        ).await;
    }
}
```

2. 在 `download_media` 失败时自动发送通知（可选，通过配置控制）
3. 在 `send_media` 的 CDN 上传失败时自动发送通知

### 配置项

```rust
pub struct WeixinConfigBuilder {
    // ...
    /// 是否在出错时自动向用户发送错误通知，默认 true
    pub fn send_error_notices(mut self, enabled: bool) -> Self;
}
```

## 测试方式

1. **单元测试**：验证各 `ErrorNoticeKind` 生成的消息格式正确
2. **Fire-and-forget 测试**：mock API 返回错误时，`send_error_notice` 不 panic、不返回 Err
3. **集成测试**：模拟媒体下载失败场景，验证用户收到错误通知消息
4. **日志测试**：确认失败时有 warn 级别日志输出

## 风险

- 低风险：fire-and-forget 模式，不影响主流程
- 注意避免递归：错误通知发送失败时不应再次触发错误通知
