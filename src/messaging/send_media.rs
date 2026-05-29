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
/// The encode type is inferred from the file extension — `audio/mpeg` maps to
/// `VoiceEncodeType::Mp3`, which 微信 accepts natively (no `SILK` transcode needed).
pub(crate) async fn send_voice_file(
    api: &Arc<HttpApiClient>,
    cdn_base_url: &str,
    to: &str,
    file_path: &Path,
    duration_ms: Option<i64>,
    context_token: Option<&str>,
    client_id: Option<&str>,
) -> Result<SendResult> {
    let filename = file_path.file_name().and_then(|n| n.to_str()).unwrap_or("voice.mp3");
    let encode_type = voice_encode_type_from_filename(filename);

    let uploaded = upload_file(api, cdn_base_url, file_path, UploadMediaType::Voice, to).await?;
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

/// Map an audio filename to its 微信 voice `encode_type`. Defaults to
/// `VoiceEncodeType::Mp3` for unknown audio formats since 微信 accepts mp3 natively.
fn voice_encode_type_from_filename(filename: &str) -> VoiceEncodeType {
    match get_mime_from_filename(filename) {
        "audio/amr" => VoiceEncodeType::Amr,
        "audio/ogg" => VoiceEncodeType::OggSpeex,
        "audio/wav" => VoiceEncodeType::Pcm,
        // audio/mpeg 与未知格式都用 Mp3（微信原生接受 mp3）
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
