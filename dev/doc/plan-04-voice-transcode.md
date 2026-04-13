# Plan 04: 语音转码 (SILK → WAV)

## 目标

支持将微信入站语音消息（SILK 格式）自动转码为 WAV 格式，方便下游业务处理（如语音识别、播放等）。

## 背景

微信语音消息使用 SILK 编码格式（encode_type=6），这是一种不常用的音频格式，大多数音频处理库不直接支持。TS 版本在 `src/media/silk-transcode.ts` 中使用 `silk-wasm` 库实现了 SILK → WAV 的转码。

### 微信语音 encode_type 枚举

| 值 | 编码 |
|----|------|
| 1 | PCM |
| 2 | ADPCM |
| 3 | Feature |
| 4 | Speex |
| 5 | AMR |
| 6 | SILK |
| 7 | MP3 |
| 8 | OGG-SPEEX |

## 实现方式

### 新建文件

`src/media/voice_transcode.rs`

### 方案选型

Rust 生态中有几种处理 SILK 的方式：

**方案 A：使用 silk-rs 或类似 crate（推荐）**

查找 crates.io 上是否有可用的 SILK 解码 crate。如果有稳定的实现，直接依赖。

**方案 B：通过 FFI 调用 silk-decoder C 库**

SILK 解码器有开源 C 实现 (https://github.com/nicedayzhu/silk-v3-decoder)，可通过 `cc` crate 编译链接。

**方案 C：作为可选 feature，调用外部命令行工具**

通过 `tokio::process::Command` 调用系统安装的 `silk-decoder` 或 `ffmpeg`（需用户自行安装）。

### 核心接口

```rust
/// 语音转码结果
pub struct TranscodeResult {
    /// 转码后的音频数据
    pub data: Vec<u8>,
    /// 输出格式（如 "wav"）
    pub format: String,
}

/// 将 SILK 格式音频转码为 WAV
///
/// 如果转码功能不可用，返回 None（优雅降级）。
pub async fn silk_to_wav(silk_data: &[u8]) -> Option<TranscodeResult>;

/// 检测是否为 SILK 格式（通过文件头魔数）
pub fn is_silk_format(data: &[u8]) -> bool {
    // SILK 文件头: 0x02 '#' '!' 'S' 'I' 'L' 'K' '_' 'V' '3'
    data.len() > 10 && &data[1..10] == b"#!SILK_V3"
}
```

### 集成点

1. 在 `MessageContext::download_media` 中，当媒体类型为 `Voice` 且 `encode_type == 6 (SILK)` 时，自动尝试转码：

```rust
// download_media 内部
if media.media_type == MediaType::Voice {
    if is_silk_format(&decrypted_data) {
        if let Some(transcoded) = silk_to_wav(&decrypted_data).await {
            // 保存为 .wav
        }
        // 转码失败时保留原始 .silk 文件
    }
}
```

2. 通过 Cargo feature 控制：

```toml
[features]
default = []
voice-transcode = ["dep:silk-decoder"]  # 可选依赖
```

### VoiceItem encode_type 枚举（Rust 侧补齐）

在 `src/types.rs` 中新增：

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize_repr, Deserialize_repr)]
#[repr(i32)]
pub enum VoiceEncodeType {
    Pcm = 1,
    Adpcm = 2,
    Feature = 3,
    Speex = 4,
    Amr = 5,
    Silk = 6,
    Mp3 = 7,
    OggSpeex = 8,
}
```

## 测试方式

1. **单元测试**：
   - `is_silk_format` 对 SILK 文件头检测准确
   - 非 SILK 数据返回 false
2. **转码测试**：使用真实 SILK 样本文件测试转码输出
   - 验证输出为有效 WAV 格式（检查 RIFF 头）
   - 验证音频参数（采样率、位深等）合理
3. **降级测试**：当 feature 未启用时，`silk_to_wav` 返回 None，不 panic
4. **集成测试**：通过 `download_media` 下载语音消息，验证自动转码生效

## 风险

- 中等风险：SILK 解码的 Rust 生态不成熟，可能需要 FFI
- 通过 feature flag 降级：不启用 feature 时完全不影响编译和运行
- 跨平台：C FFI 方案需注意 Windows/Linux/macOS 兼容性
