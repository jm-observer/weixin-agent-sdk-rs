//! Voice transcode utilities: SILK ↔ WAV
//!
//! - `silk_to_wav`：接收方向。微信入站语音消息是 SILK V3 编码，转 16 kHz mono
//!   16-bit WAV 后给 ASR 引擎用。
//! - `wav_to_silk`：发送方向（**新加，2026-05-31**）。微信客户端的 `voice_item`
//!   语音气泡**实际只识别 SILK 编码**（npm 包 README 明示「Voice (SILK
//!   encoded)」；`encode_type` 协议字段虽允许 1=PCM / 7=MP3 等多种值，但
//!   实测 mp3/wav 不显示气泡）。发送方上传必须是 SILK。
//!
//! 两个函数都走 `voice-transcode` feature gate，避免不发语音的 SDK 用户白付
//! `silk-rs` / `hound` 依赖成本。

/// Result of a transcode operation.
pub struct TranscodeResult {
    /// Transcoded audio data (WAV format).
    pub data: Vec<u8>,
    /// Output format string, e.g., "wav".
    pub format: String,
}

/// Detect whether the provided data is a SILK V3 format.
pub fn is_silk_format(data: &[u8]) -> bool {
    // SILK file header: 0x02 '#''!' 'S' 'I' 'L' 'K' '_' 'V' '3'
    data.len() > 10 && &data[1..10] == b"#!SILK_V3"
}

/// Convert SILK data to WAV. Returns None if transcoding is unavailable or fails.
#[cfg(feature = "voice-transcode")]
pub fn silk_to_wav(silk_data: &[u8]) -> Option<TranscodeResult> {
    // Decode SILK to PCM using the silk-rs crate. Output at 16 kHz: that is
    // what general ASR engines (Whisper etc.) expect, and `download_media`'s
    // documented purpose is to yield ASR-ready WAV.
    let pcm = match silk_rs::decode_silk(silk_data, 16000) {
        Ok(p) => p,
        Err(_) => return None,
    };
    // Build a simple WAV header (mono, 16-bit, 16000 Hz).
    let wav = pcm_to_wav(&pcm, 16000, 16, 1);
    Some(TranscodeResult {
        data: wav,
        format: "wav".to_string(),
    })
}

/// Stub when feature is disabled.
#[cfg(not(feature = "voice-transcode"))]
pub fn silk_to_wav(_silk_data: &[u8]) -> Option<TranscodeResult> {
    None
}

/// Convert WAV (16-bit PCM, mono/stereo, any sample rate in SILK's range) to
/// SILK V3 bytes suitable for 微信 send_voice (encode_type=6). Returns None on
/// unsupported wav format or encode failure.
///
/// 实现细节：
/// - 用 `hound` 解析 WAV header 拿 PCM samples + sample_rate
/// - 多声道时简单做均值 mono 化（语音 TTS 通常已是 mono）
/// - 调 `silk_rs::encode_silk(pcm, sample_rate, bit_rate=24000, tencent=true)`
///   —— tencent=true 走微信兼容 SILK 变种
///
/// 调用方典型场景：TTS 出 24 kHz mono 16-bit WAV → 本函数 → SILK → CDN upload。
#[cfg(feature = "voice-transcode")]
pub fn wav_to_silk(wav_data: &[u8]) -> Option<Vec<u8>> {
    let mut reader = match hound::WavReader::new(std::io::Cursor::new(wav_data)) {
        Ok(r) => r,
        Err(e) => {
            tracing::warn!("wav_to_silk: 解析 WAV header 失败: {e}");
            return None;
        }
    };
    let spec = reader.spec();
    if spec.bits_per_sample != 16 || spec.sample_format != hound::SampleFormat::Int {
        tracing::warn!(
            "wav_to_silk: 仅支持 16-bit Int PCM WAV (got bits={}, fmt={:?})",
            spec.bits_per_sample,
            spec.sample_format
        );
        return None;
    }
    // 读 i16 samples → mono 化
    let interleaved: Vec<i16> = match reader.samples::<i16>().collect::<Result<Vec<_>, _>>() {
        Ok(v) => v,
        Err(e) => {
            tracing::warn!("wav_to_silk: 读 samples 失败: {e}");
            return None;
        }
    };
    let mono_samples: Vec<i16> = if spec.channels == 1 {
        interleaved
    } else {
        let ch = spec.channels as usize;
        interleaved
            .chunks_exact(ch)
            .map(|c| {
                let sum: i32 = c.iter().map(|s| i32::from(*s)).sum();
                #[allow(clippy::cast_possible_truncation)]
                let avg = (sum / ch as i32) as i16;
                avg
            })
            .collect()
    };
    // i16 → little-endian bytes
    let mut pcm_bytes = Vec::with_capacity(mono_samples.len() * 2);
    for s in mono_samples {
        pcm_bytes.extend_from_slice(&s.to_le_bytes());
    }
    let sample_rate = i32::try_from(spec.sample_rate).unwrap_or(16000);
    // 24 kbps 是 SILK 语音常用码率（覆盖 6-40 kbps）
    match silk_rs::encode_silk(&pcm_bytes, sample_rate, 24000, true) {
        Ok(silk) => Some(silk),
        Err(e) => {
            tracing::warn!("wav_to_silk: encode_silk 失败: {e:?}");
            None
        }
    }
}

/// Stub when feature is disabled.
#[cfg(not(feature = "voice-transcode"))]
pub fn wav_to_silk(_wav_data: &[u8]) -> Option<Vec<u8>> {
    None
}

// Helper: convert raw PCM (little-endian i16) to a WAV container.
#[allow(dead_code)]
fn pcm_to_wav(pcm: &[u8], sample_rate: i32, bits_per_sample: i32, channels: i16) -> Vec<u8> {
    let data_len = u32::try_from(pcm.len()).unwrap_or(0);
    // Use u32 for calculations to avoid overflow
    let sr = u32::try_from(sample_rate).unwrap_or(0);
    let ch = u32::try_from(channels).unwrap_or(0);
    let bps = u32::try_from(bits_per_sample).unwrap_or(0);

    let byte_rate = sr.checked_mul(ch).and_then(|v| v.checked_mul(bps / 8)).unwrap_or(0);

    let block_align = u16::try_from(ch)
        .unwrap_or(0)
        .checked_mul(u16::try_from(bps / 8).unwrap_or(0))
        .unwrap_or(0);

    let mut wav = Vec::with_capacity(44 + pcm.len());
    // RIFF header
    wav.extend_from_slice(b"RIFF");
    wav.extend_from_slice(&(36u32 + data_len).to_le_bytes());
    wav.extend_from_slice(b"WAVE");
    // fmt subchunk
    wav.extend_from_slice(b"fmt ");
    wav.extend_from_slice(&16u32.to_le_bytes()); // Subchunk1Size
    wav.extend_from_slice(&1u16.to_le_bytes()); // AudioFormat PCM
    wav.extend_from_slice(&channels.to_le_bytes());
    wav.extend_from_slice(&sample_rate.to_le_bytes());
    wav.extend_from_slice(&byte_rate.to_le_bytes());
    wav.extend_from_slice(&block_align.to_le_bytes());
    wav.extend_from_slice(&u16::try_from(bps).unwrap_or(0).to_le_bytes());
    // data subchunk
    wav.extend_from_slice(b"data");
    wav.extend_from_slice(&data_len.to_le_bytes());
    wav.extend_from_slice(pcm);
    wav
}
