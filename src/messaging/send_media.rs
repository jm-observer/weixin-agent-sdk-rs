//! Media message upload and sending, routed by MIME type.

use std::path::Path;
use std::sync::Arc;

use crate::api::client::HttpApiClient;
use crate::cdn::upload::{CdnUploadResult, upload_file};
use crate::error::Result;
use crate::media::mime::get_mime_from_filename;
use crate::messaging::inbound::SendResult;
use crate::messaging::send::generate_client_id;
use crate::types::{
    CdnMedia, FileItem, ImageItem, MessageItem, MessageItemType, MessageState, MessageType, SendMessageRequest,
    UploadMediaType, VideoItem, VoiceEncodeType, VoiceItem, WeixinMessage, build_base_info,
};

/// Upload a file and send it as a message, routing by MIME type.
pub(crate) async fn send_media_file(
    api: &Arc<HttpApiClient>,
    cdn_base_url: &str,
    to: &str,
    file_path: &Path,
    text: &str,
    context_token: Option<&str>,
    client_id: Option<&str>,
) -> Result<SendResult> {
    let filename = file_path.file_name().and_then(|n| n.to_str()).unwrap_or("file.bin");
    let mime = get_mime_from_filename(filename);

    let (media_type, build_item): (UploadMediaType, fn(&str, &CdnUploadResult) -> MessageItem) =
        if mime.starts_with("video/") {
            (UploadMediaType::Video, build_video_item)
        } else if mime.starts_with("image/") {
            (UploadMediaType::Image, build_image_item)
        } else {
            (UploadMediaType::File, |fname, u| build_file_item(fname, u))
        };

    let uploaded = upload_file(api, cdn_base_url, file_path, media_type, to).await?;
    let media_item = build_item(filename, &uploaded);

    // Send text and media as separate requests
    if !text.is_empty() {
        let text_req = crate::messaging::send::build_text_message(to, text, context_token, None);
        api.send_message(&text_req).await?;
    }

    let client_id = client_id.map_or_else(generate_client_id, String::from);
    let req = SendMessageRequest {
        msg: WeixinMessage {
            from_user_id: Some(String::new()),
            to_user_id: Some(to.to_owned()),
            client_id: Some(client_id.clone()),
            message_type: Some(MessageType::Bot),
            message_state: Some(MessageState::Finish),
            item_list: Some(vec![media_item]),
            context_token: context_token.map(String::from),
            ..Default::default()
        },
        base_info: build_base_info(),
    };
    api.send_message(&req).await?;

    Ok(SendResult { message_id: client_id })
}

/// Upload an audio file and send it as a **voice** message (语音气泡), as opposed
/// to `send_media_file` which routes audio MIME to a generic File attachment.
///
/// `duration_ms` is the playback length in milliseconds. 微信语音气泡
/// display this duration; pass `None` if unknown (the bubble may then show 0s).
///
/// **格式约束**（2026-05-31 修正）：微信客户端的 `voice_item` 语音气泡**实际
/// 只识别 SILK 编码**（npm 包 README 明示「Voice (SILK encoded)」；mp3 / wav
/// 即便 `encode_type` 字段填对也不显示气泡——实测）。本函数：
///   - 检测输入字节是否已经是 SILK V3 → 直接上传
///   - 否则尝试 WAV → SILK 转码（feature `voice-transcode` 启用时；
///     `silk_rs::encode_silk(tencent=true)` + `hound` 解 WAV header）→ 上传
///     SILK 字节、encode_type = Silk
///   - 转码不可用（feature 关 / 非 WAV / 编码失败）→ 按原扩展名映射
///     encode_type 上传（向后兼容老调用，但微信端可能不显示气泡，仅作 fallback）
///
/// 调用方建议直接传 WAV 或 SILK 文件，不要传 mp3——SDK 无 mp3 解码能力。
pub(crate) async fn send_voice_file(
    api: &Arc<HttpApiClient>,
    cdn_base_url: &str,
    to: &str,
    file_path: &Path,
    duration_ms: Option<i64>,
    context_token: Option<&str>,
    client_id: Option<&str>,
) -> Result<SendResult> {
    let filename = file_path.file_name().and_then(|n| n.to_str()).unwrap_or("voice.silk");
    let original_bytes = tokio::fs::read(file_path).await?;

    // 决定最终上传路径与 encode_type
    let (upload_path_buf, tmp_to_clean, encode_type) =
        prepare_voice_upload(file_path, filename, &original_bytes).await?;

    let upload_result = upload_file(
        api,
        cdn_base_url,
        upload_path_buf.as_path(),
        UploadMediaType::Voice,
        to,
    )
    .await;
    // 不论 upload 成败，先清理临时文件（如果有）
    if let Some(tmp) = tmp_to_clean {
        let _ = tokio::fs::remove_file(&tmp).await;
    }
    let uploaded = upload_result?;

    let media_item = build_voice_item(&uploaded, encode_type, duration_ms);

    let client_id = client_id.map_or_else(generate_client_id, String::from);
    let req = SendMessageRequest {
        msg: WeixinMessage {
            from_user_id: Some(String::new()),
            to_user_id: Some(to.to_owned()),
            client_id: Some(client_id.clone()),
            message_type: Some(MessageType::Bot),
            message_state: Some(MessageState::Finish),
            item_list: Some(vec![media_item]),
            context_token: context_token.map(String::from),
            ..Default::default()
        },
        base_info: build_base_info(),
    };
    api.send_message(&req).await?;

    Ok(SendResult { message_id: client_id })
}

/// 决定 send_voice_file 实际要上传的字节路径 + encode_type。
///
/// 返回 (upload_path, tmp_path_to_clean_after, encode_type)：
/// - 已是 SILK → 原文件路径，无 tmp
/// - WAV 转码成功 → tmp 文件路径（caller 清理），encode_type=Silk
/// - 否则 → 原文件路径，按扩展名映射的 encode_type（fallback、可能不显示）
async fn prepare_voice_upload(
    original_path: &Path,
    filename: &str,
    bytes: &[u8],
) -> Result<(std::path::PathBuf, Option<std::path::PathBuf>, VoiceEncodeType)> {
    use crate::media::voice_transcode;

    // 1) 已是 SILK V3 → 直接传
    if voice_transcode::is_silk_format(bytes) {
        return Ok((original_path.to_path_buf(), None, VoiceEncodeType::Silk));
    }

    // 2) 非 SILK → 尝试 wav→silk（仅 voice-transcode feature 启用）
    if let Some(silk_bytes) = voice_transcode::wav_to_silk(bytes) {
        // 写临时文件
        let mut tmp_name = String::from("weixin-voice-");
        let n: u64 = rand::random();
        use std::fmt::Write;
        let _ = write!(tmp_name, "{n:016x}.silk");
        let tmp = std::env::temp_dir().join(tmp_name);
        if let Err(e) = tokio::fs::write(&tmp, &silk_bytes).await {
            tracing::warn!(
                "send_voice_file: 写 silk 临时文件失败 ({}): {e}",
                tmp.display()
            );
            // 回退：按扩展名映射 encode_type 上传原文件
            return Ok((
                original_path.to_path_buf(),
                None,
                voice_encode_type_from_filename(filename),
            ));
        }
        tracing::debug!(
            "send_voice_file: wav→silk 转码成功 ({} bytes → {} bytes, tmp={})",
            bytes.len(),
            silk_bytes.len(),
            tmp.display()
        );
        return Ok((tmp.clone(), Some(tmp), VoiceEncodeType::Silk));
    }

    // 3) 转码不可用（feature 关 / 非 wav / encode 失败）→ 按扩展名映射 encode_type
    tracing::warn!(
        "send_voice_file: 输入既非 SILK 也无法 wav→silk 转码，按扩展名 encode_type 上传 {filename}（微信端可能不显示语音气泡）"
    );
    Ok((
        original_path.to_path_buf(),
        None,
        voice_encode_type_from_filename(filename),
    ))
}

/// Map an audio filename to its 微信 voice `encode_type`. **仅作 fallback**——
/// 实测微信只识别 Silk (6) 的语音气泡；本映射保留只为兼容老调用（例如
/// 已经是 silk 的文件用 .silk 扩展名上传时跳过转码路径，但实际 silk 检测
/// 是看 `is_silk_format` 字节头不是扩展名）。
fn voice_encode_type_from_filename(filename: &str) -> VoiceEncodeType {
    match get_mime_from_filename(filename) {
        "audio/amr" => VoiceEncodeType::Amr,
        "audio/ogg" => VoiceEncodeType::OggSpeex,
        "audio/wav" => VoiceEncodeType::Pcm,
        _ => VoiceEncodeType::Mp3,
    }
}

fn build_voice_item(uploaded: &CdnUploadResult, encode_type: VoiceEncodeType, duration_ms: Option<i64>) -> MessageItem {
    use base64::Engine;
    MessageItem {
        item_type: Some(MessageItemType::Voice),
        voice_item: Some(VoiceItem {
            media: Some(CdnMedia {
                encrypt_query_param: Some(uploaded.encrypt_query_param.clone()),
                aes_key: Some(base64::engine::general_purpose::STANDARD.encode(uploaded.aes_key_hex.as_bytes())),
                encrypt_type: Some(1),
                ..Default::default()
            }),
            encode_type: Some(encode_type as i32),
            playtime: duration_ms,
            ..Default::default()
        }),
        ..Default::default()
    }
}

fn build_image_item(_filename: &str, uploaded: &CdnUploadResult) -> MessageItem {
    use base64::Engine;
    #[allow(clippy::cast_possible_wrap)] // file sizes won't exceed i64::MAX
    let mid_size = uploaded.file_size_ciphertext as i64;
    MessageItem {
        item_type: Some(MessageItemType::Image),
        image_item: Some(ImageItem {
            media: Some(CdnMedia {
                encrypt_query_param: Some(uploaded.encrypt_query_param.clone()),
                aes_key: Some(base64::engine::general_purpose::STANDARD.encode(uploaded.aes_key_hex.as_bytes())),
                encrypt_type: Some(1),
                ..Default::default()
            }),
            mid_size: Some(mid_size),
            ..Default::default()
        }),
        ..Default::default()
    }
}

fn build_video_item(_filename: &str, uploaded: &CdnUploadResult) -> MessageItem {
    use base64::Engine;
    #[allow(clippy::cast_possible_wrap)] // file sizes won't exceed i64::MAX
    let video_size = uploaded.file_size_ciphertext as i64;
    MessageItem {
        item_type: Some(MessageItemType::Video),
        video_item: Some(VideoItem {
            media: Some(CdnMedia {
                encrypt_query_param: Some(uploaded.encrypt_query_param.clone()),
                aes_key: Some(base64::engine::general_purpose::STANDARD.encode(uploaded.aes_key_hex.as_bytes())),
                encrypt_type: Some(1),
                ..Default::default()
            }),
            video_size: Some(video_size),
            ..Default::default()
        }),
        ..Default::default()
    }
}

fn build_file_item(filename: &str, uploaded: &CdnUploadResult) -> MessageItem {
    use base64::Engine;
    MessageItem {
        item_type: Some(MessageItemType::File),
        file_item: Some(FileItem {
            media: Some(CdnMedia {
                encrypt_query_param: Some(uploaded.encrypt_query_param.clone()),
                aes_key: Some(base64::engine::general_purpose::STANDARD.encode(uploaded.aes_key_hex.as_bytes())),
                encrypt_type: Some(1),
                ..Default::default()
            }),
            file_name: Some(filename.to_owned()),
            len: Some(uploaded.file_size.to_string()),
            ..Default::default()
        }),
        ..Default::default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fake_upload() -> CdnUploadResult {
        CdnUploadResult {
            encrypt_query_param: "q".to_string(),
            aes_key_base64: "k".to_string(),
            aes_key_hex: "6b6579".to_string(),
            file_size: 1234,
            file_size_ciphertext: 1248,
            filekey: "fk".to_string(),
        }
    }

    #[test]
    fn voice_encode_type_inferred_from_extension() {
        // 仅测 mime.rs 实际支持的音频扩展名（mp3 / ogg / wav）
        assert_eq!(voice_encode_type_from_filename("clip.mp3"), VoiceEncodeType::Mp3);
        assert_eq!(voice_encode_type_from_filename("clip.ogg"), VoiceEncodeType::OggSpeex);
        assert_eq!(voice_encode_type_from_filename("clip.wav"), VoiceEncodeType::Pcm);
        // 未知 / mime.rs 未覆盖的扩展名默认 Mp3（微信原生接受 mp3）
        assert_eq!(voice_encode_type_from_filename("clip.xyz"), VoiceEncodeType::Mp3);
        assert_eq!(voice_encode_type_from_filename("clip.amr"), VoiceEncodeType::Mp3);
    }

    #[test]
    fn build_voice_item_sets_voice_type_and_fields() {
        let item = build_voice_item(&fake_upload(), VoiceEncodeType::Mp3, Some(3500));
        assert_eq!(item.item_type, Some(MessageItemType::Voice));
        let voice = item.voice_item.expect("voice_item present");
        assert_eq!(voice.encode_type, Some(VoiceEncodeType::Mp3 as i32));
        assert_eq!(voice.playtime, Some(3500));
        assert!(voice.media.is_some());
        // 非 voice 字段不应被设置
        assert!(item.image_item.is_none());
        assert!(item.file_item.is_none());
    }

    #[test]
    fn build_voice_item_allows_unknown_duration() {
        let item = build_voice_item(&fake_upload(), VoiceEncodeType::Mp3, None);
        let voice = item.voice_item.expect("voice_item present");
        assert_eq!(voice.playtime, None);
    }
}
