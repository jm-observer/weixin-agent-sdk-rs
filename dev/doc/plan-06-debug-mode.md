# Plan 06: 调试模式与计时统计 (Debug Mode & Timing)

## 目标

实现按会话的调试模式开关和全链路耗时统计，帮助开发者定位性能问题。

## 背景

TS 版本实现了两个相关功能：

### 1. Debug Mode (`src/messaging/debug-mode.ts`)

- 通过 `/toggle-debug` 斜杠命令开关
- 按账号维度控制
- 开启后在 Bot 回复后附加全链路耗时统计

### 2. Slash Commands (`src/messaging/slash-commands.ts`)

- `/echo <message>` — 直接回显消息（跳过 AI），附带通道耗时
- `/toggle-debug` — 切换调试模式

### 3. 耗时统计维度

```
Platform→Plugin: Xms          // 从微信服务器到 SDK 的延迟
Inbound processing: Xms       // 入站处理耗时（鉴权、路由、媒体下载）
AI generation: Xms             // AI 生成耗时
Reply delivery: Xms            // 回复发送耗时
Total: Xms                     // 全链路总耗时
```

## 实现方式

### 新建文件

- `src/messaging/debug_mode.rs` — 调试模式状态管理
- `src/messaging/slash_commands.rs` — 斜杠命令处理

### 1. 调试模式状态

```rust
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

/// 调试模式管理器
pub struct DebugMode {
    enabled: AtomicBool,
}

impl DebugMode {
    pub fn new() -> Self {
        Self { enabled: AtomicBool::new(false) }
    }

    pub fn toggle(&self) -> bool {
        let prev = self.enabled.fetch_xor(true, Ordering::Relaxed);
        !prev // 返回切换后的状态
    }

    pub fn is_enabled(&self) -> bool {
        self.enabled.load(Ordering::Relaxed)
    }
}
```

### 2. 计时统计

```rust
/// 消息处理的计时记录
#[derive(Debug, Clone)]
pub struct MessageTiming {
    /// 消息创建时间（服务端）
    pub event_timestamp_ms: i64,
    /// SDK 收到消息的时间
    pub received_at_ms: u64,
    /// 入站处理完成时间
    pub inbound_done_ms: Option<u64>,
    /// 回复发送完成时间
    pub reply_done_ms: Option<u64>,
}

impl MessageTiming {
    pub fn platform_to_plugin_ms(&self) -> u64 {
        self.received_at_ms.saturating_sub(self.event_timestamp_ms as u64)
    }

    pub fn inbound_processing_ms(&self) -> Option<u64> {
        self.inbound_done_ms.map(|t| t.saturating_sub(self.received_at_ms))
    }

    pub fn total_ms(&self) -> Option<u64> {
        self.reply_done_ms.map(|t| t.saturating_sub(self.received_at_ms))
    }

    /// 格式化为可读的计时报告
    pub fn format_report(&self) -> String;
}
```

### 3. 斜杠命令处理

```rust
/// 斜杠命令处理结果
pub enum SlashCommandResult {
    /// 命令已处理，不需要继续走正常消息流程
    Handled,
    /// 不是斜杠命令，继续正常处理
    NotACommand,
}

/// 处理斜杠命令
pub async fn handle_slash_command(
    text: &str,
    ctx: &MessageContext,
    debug_mode: &DebugMode,
    timing: &MessageTiming,
) -> SlashCommandResult {
    let text = text.trim();

    if text.starts_with("/echo ") {
        let echo_text = &text[6..];
        let timing_info = format!(
            "\n\n---\nPlatform→SDK: {}ms",
            timing.platform_to_plugin_ms()
        );
        let _ = ctx.reply_text(&format!("{echo_text}{timing_info}")).await;
        return SlashCommandResult::Handled;
    }

    if text == "/toggle-debug" {
        let new_state = debug_mode.toggle();
        let status = if new_state { "ON" } else { "OFF" };
        let _ = ctx.reply_text(&format!("[Debug mode: {status}]")).await;
        return SlashCommandResult::Handled;
    }

    SlashCommandResult::NotACommand
}
```

### 4. 集成到 MessageHandler

在 `MessageContext` 中添加 timing 和 debug_mode 引用：

```rust
pub struct MessageContext {
    // ... 现有字段
    pub timing: MessageTiming,
}
```

在 `run_monitor` 中：
1. 收到消息时记录 `received_at_ms`
2. 传递 `event_timestamp_ms`（从 `create_time_ms`）
3. 调用 `handle_slash_command` 判断是否斜杠命令
4. 如果不是斜杠命令，调用 `handler.on_message`
5. 调用后记录 `reply_done_ms`
6. 如果 debug_mode 开启，额外发送 timing report

### 5. 配置项

```rust
pub struct WeixinConfigBuilder {
    // ...
    /// 是否启用斜杠命令处理，默认 true
    pub fn enable_slash_commands(mut self, enabled: bool) -> Self;
}
```

## 测试方式

1. **单元测试**：
   - `DebugMode::toggle` 正确切换状态
   - `MessageTiming::format_report` 输出格式正确
   - `handle_slash_command` 正确识别 `/echo` 和 `/toggle-debug`
   - 非斜杠命令返回 `NotACommand`
2. **计时测试**：
   - 各时间段计算正确
   - `saturating_sub` 处理时钟偏差不 panic
3. **集成测试**：
   - 发送 `/echo hello`，验证收到回显 + 耗时信息
   - 发送 `/toggle-debug`，验证状态切换
   - 开启 debug 后发送普通消息，验证回复后附带计时报告
   - 关闭 debug 后发送普通消息，验证无额外报告

## 风险

- 低风险：独立功能模块，可通过配置关闭
- 斜杠命令需注意与用户正常消息的冲突（`/` 开头的普通文本）
- 计时依赖系统时钟，跨时区场景可能有偏差（`platform_to_plugin_ms`）
