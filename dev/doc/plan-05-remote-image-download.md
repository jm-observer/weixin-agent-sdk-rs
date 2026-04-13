# Plan 05: 远程图片下载 (Remote Image Download)

## 目标

支持从 URL 下载远程图片到本地临时文件，用于后续通过 CDN 上传发送给用户。

## 背景

TS 版本在 `src/cdn/pic-decrypt.ts` 中实现了 `downloadRemoteImageToTemp(url, destDir)`，用于以下场景：

- Bot 需要发送一张来自外部 URL 的图片给用户
- 先下载到本地临时文件，再通过 CDN 上传流程发送

该功能根据 HTTP 响应的 `Content-Type` 头或 URL 路径推断文件扩展名。

## 实现方式

### 新建文件

`src/media/remote_download.rs`

### 核心接口

```rust
use std::path::{Path, PathBuf};
use crate::error::Result;

/// 从 URL 下载文件到指定目录
///
/// 文件名自动生成，扩展名从 Content-Type 或 URL 推断。
///
/// # Arguments
/// * `url` - 远程文件 URL
/// * `dest_dir` - 目标目录（需已存在）
///
/// # Returns
/// 下载后的本地文件路径
pub async fn download_remote_file(url: &str, dest_dir: &Path) -> Result<PathBuf>;

/// 从 URL 下载文件到系统临时目录
///
/// 便捷方法，自动使用 `std::env::temp_dir()`。
pub async fn download_remote_file_to_temp(url: &str) -> Result<PathBuf>;
```

### 实现细节

```rust
pub async fn download_remote_file(url: &str, dest_dir: &Path) -> Result<PathBuf> {
    let client = reqwest::Client::new();
    let resp = client.get(url).send().await?;

    // 从 Content-Type 或 URL 推断扩展名
    let content_type = resp.headers()
        .get(reqwest::header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok());
    let ext = get_extension_from_content_type_or_url(content_type, url);

    // 生成唯一文件名
    let filename = temp_file_name("remote", ext);
    let file_path = dest_dir.join(&filename);

    // 流式写入文件
    let bytes = resp.bytes().await?;
    tokio::fs::write(&file_path, &bytes).await?;

    Ok(file_path)
}
```

### 集成点

1. 在 `WeixinClient` 上暴露公共方法：

```rust
impl WeixinClient {
    /// 从 URL 下载文件到临时目录，然后发送给用户
    pub async fn send_remote_media(
        &self,
        to: &str,
        url: &str,
        context_token: Option<&str>,
    ) -> Result<SendResult>;
}
```

内部流程：
1. `download_remote_file_to_temp(url)`
2. `send_media(to, &local_path, context_token)`
3. 清理临时文件

2. 复用已有的 `get_extension_from_content_type_or_url` 和 `temp_file_name` 工具函数。

### 安全考虑

- 限制最大下载大小（默认 100MB，与 TS 一致）
- 设置合理的下载超时（默认 60 秒）
- 仅允许 HTTP/HTTPS 协议

```rust
const MAX_DOWNLOAD_SIZE: u64 = 100 * 1024 * 1024; // 100 MB
const DOWNLOAD_TIMEOUT: Duration = Duration::from_secs(60);
```

## 测试方式

1. **单元测试**：
   - 文件扩展名推断正确（从 Content-Type、从 URL）
   - 唯一文件名不冲突
2. **下载测试**（需要网络或 mock server）：
   - 下载一个已知图片 URL，验证本地文件内容正确
   - 验证文件扩展名与 Content-Type 匹配
3. **大小限制测试**：超过 100MB 的下载被拒绝
4. **超时测试**：慢速服务器触发超时错误
5. **集成测试**：`send_remote_media` 端到端发送远程图片

## 风险

- 低风险：标准 HTTP 下载功能，已有 reqwest 依赖
- 注意临时文件清理，避免磁盘泄漏
