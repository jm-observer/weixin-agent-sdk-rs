# Plan 01: 版本号与协议字段同步

## 目标

将 Rust SDK 的协议版本从 `2.1.1` 升级到 `2.1.8`，同步 TS 新增/变更的协议字段，确保与最新服务端 API 完全兼容。

## 变更内容

### 1. 更新 CHANNEL_VERSION

**文件**: `src/types.rs`

```rust
// 旧
pub const CHANNEL_VERSION: &str = "2.1.1";
// 新
pub const CHANNEL_VERSION: &str = "2.1.8";
```

### 2. 同步 TS 新增的类型字段

对比 TS `src/api/types.ts` 与 Rust `src/types.rs`，确认以下字段是否缺失或需更新：

#### ImageItem

TS 新增字段（确认 Rust 是否已有）：

- `hd_size: Option<i64>` — 高清图片大小

#### VoiceItem

确认 TS 的 `encode_type` 枚举值是否有新增（PCM=1, ADPCM=2, feature=3, speex=4, AMR=5, SILK=6, MP3=7, OGG-SPEEX=8）。

#### CdnMedia

确认 `encrypt_type` 的语义：
- `0` = fileID only
- `1` = includes metadata (packed)

#### GetUpdatesResponse

TS 同时支持 `sync_buf`（旧名）和 `get_updates_buf`（新名）两个字段，做兼容性回退。确认 Rust 是否已处理。

### 3. Cargo.toml 版本号

更新 `Cargo.toml` 中的 `version` 字段以匹配新的发布版本。

## 实现方式

1. 修改 `src/types.rs` 中的 `CHANNEL_VERSION` 常量
2. 逐字段对比 TS `types.ts` 与 Rust `types.rs`，补齐缺失字段（加 `Option` 包裹，`serde` 跳过 None）
3. 确认 `GetUpdatesResponse` 的兼容性处理（`sync_buf` / `get_updates_buf` 双字段回退）
4. 更新 `Cargo.toml` 版本号

## 测试方式

1. **编译测试**：`cargo build` 通过，无类型不匹配
2. **序列化测试**：单元测试验证新增字段的序列化/反序列化，尤其是 `Option` 字段在缺失时能正确 skip
3. **兼容性测试**：用旧版 JSON 响应（无新字段）测试反序列化不报错
4. **集成测试**：连接真实服务端，确认 `CHANNEL_VERSION` 更新后 API 调用正常

## 风险

- 低风险：纯常量+可选字段更新，向后兼容
