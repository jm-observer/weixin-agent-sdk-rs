# Rust SDK 升级总览

## 背景

Rust 版本 (`weixin-agent-sdk-rs`) 是基于 TypeScript 旧版 (`@tencent-weixin/openclaw-weixin`) 复刻的。
当前 TS 版本已升级到 **2.1.8**，Rust 仍停留在 **2.1.1**。

经过对两个代码库的完整对比，以下是 Rust 版本需要升级的功能模块，按优先级排列：

| Plan | 模块 | 优先级 | 说明 |
|------|------|--------|------|
| [01](plan-01-version-protocol-sync.md) | 版本号与协议字段同步 | P0 | 基础协议对齐，影响所有 API 调用 |
| [02](plan-02-markdown-filter.md) | Markdown 过滤器 | P1 | 出站消息的 Markdown 清洗，CJK 感知 |
| [03](plan-03-error-notice.md) | 错误通知发送 | P1 | 向用户发送错误通知消息（fire-and-forget） |
| [04](plan-04-voice-transcode.md) | 语音转码 (SILK → WAV) | P2 | 入站语音消息自动转码 |
| [05](plan-05-remote-image-download.md) | 远程图片下载 | P2 | 从 URL 下载图片到本地临时文件 |
| [06](plan-06-debug-mode.md) | 调试模式与计时统计 | P3 | 按账号开关调试模式，全链路耗时统计 |

## 当前 Rust 已实现的功能

- 核心 API：getUpdates / sendMessage / getUploadUrl / getConfig / sendTyping
- CDN 上传/下载/加解密 (AES-128-ECB)
- QR 扫码登录（含 IDC 重定向）
- 长轮询监控循环 (Monitor)
- 会话守卫 (SessionGuard) + 配置缓存 (ConfigCache)
- 消息发送：文本 + 媒体（图片/视频/文件）
- Context Token 管理
- MIME 类型映射
- 日志脱敏工具

## TS 中属于框架层（不在 Rust SDK 升级范围）的功能

以下功能属于 OpenClaw 框架层集成，Rust SDK 作为独立库不需要实现：

- 多账号管理（账号注册/注销/持久化）
- 框架配对 (Pairing / AllowFrom)
- Agent 路由 & 会话记录
- 流式文本合并 (Block Streaming Coalescing)
- Agent Prompt Hints
- 宿主版本兼容性检查 (compat.ts)
- 运行时管理 (runtime.ts)
- 通道插件元数据 (channel.ts)
- 自定义 JSON Logger（Rust 用 `tracing` 生态替代）
