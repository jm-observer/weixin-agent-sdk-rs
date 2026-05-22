//! Voice transcode utilities: SILK -> WAV

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
