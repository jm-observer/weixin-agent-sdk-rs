//! iLink Bot API 在业务失败时仍返回 HTTP 200，把错误放在响应体的
//! `ret` / `errcode` / `errmsg` 里。SDK 若只看 HTTP 状态码，就会把
//! 「服务端明确拒绝投递」当成发送成功——调用方无从感知，消息静默消失。
//!
//! 这组测试用一个手写的最小 HTTP mock server 复现该场景：不引入
//! wiremock/httpmock 依赖，只用已有的 tokio。

use std::time::Duration;

use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;
use weixin_agent::api::client::HttpApiClient;
use weixin_agent::config::WeixinConfig;
use weixin_agent::error::Error;
use weixin_agent::messaging::send::build_text_message;

/// 起一个只回固定响应体的 HTTP/1.1 mock server，返回它的 base_url。
///
/// 一直循环 accept，避免 `post_json` 的瞬时错误重试撞上已关闭的监听。
async fn spawn_mock(status_line: &'static str, body: &'static str) -> String {
    let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind mock listener");
    let addr = listener.local_addr().expect("mock listener addr");

    tokio::spawn(async move {
        loop {
            let Ok((mut stream, _)) = listener.accept().await else {
                return;
            };
            tokio::spawn(async move {
                // 读一次就够：只需把请求从内核缓冲区取走，不解析内容。
                let mut buf = [0u8; 4096];
                let _ = stream.read(&mut buf).await;
                let resp = format!(
                    "{status_line}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                    body.len()
                );
                let _ = stream.write_all(resp.as_bytes()).await;
                let _ = stream.flush().await;
            });
        }
    });

    format!("http://{addr}/")
}

fn client_for(base_url: &str) -> HttpApiClient {
    let config = WeixinConfig::builder()
        .base_url(base_url)
        .token("test-token")
        .api_timeout(Duration::from_secs(5))
        .build()
        .expect("build config");
    HttpApiClient::new(&config)
}

/// 复现：服务端 HTTP 200 + `ret` 非 0（明确拒绝），`send_message` 必须报错。
#[tokio::test]
async fn send_message_rejects_nonzero_ret() {
    let base_url = spawn_mock("HTTP/1.1 200 OK", r#"{"ret":-3,"errmsg":"user not in active session"}"#).await;
    let api = client_for(&base_url);

    let req = build_text_message("o9cq80xy@im.wechat", "天气预警", None, Some("cid-1"));
    let result = api.send_message(&req).await;

    let err = result.expect_err("ret 非 0 表示服务端拒绝投递，必须返回 Err");
    match err {
        Error::Api { errcode, errmsg } => {
            assert_eq!(errcode, -3, "errcode 应透传服务端的 ret");
            assert!(
                errmsg.contains("user not in active session"),
                "errmsg 应保留服务端原因，实际: {errmsg}"
            );
        }
        other => panic!("应为 Error::Api，实际: {other:?}"),
    }
}

/// 同一场景的另一种字段形态：`errcode` 非 0。
#[tokio::test]
async fn send_message_rejects_nonzero_errcode() {
    let base_url = spawn_mock(
        "HTTP/1.1 200 OK",
        r#"{"errcode":45015,"errmsg":"response out of time limit"}"#,
    )
    .await;
    let api = client_for(&base_url);

    let req = build_text_message("o9cq80xy@im.wechat", "天气预警", None, Some("cid-2"));
    let result = api.send_message(&req).await;

    let err = result.expect_err("errcode 非 0 必须返回 Err");
    match err {
        Error::Api { errcode, errmsg } => {
            assert_eq!(errcode, 45015);
            assert!(errmsg.contains("out of time limit"), "实际: {errmsg}");
        }
        other => panic!("应为 Error::Api，实际: {other:?}"),
    }
}

/// 回归护栏：正常成功响应不能被误判成失败。
#[tokio::test]
async fn send_message_accepts_success() {
    let base_url = spawn_mock("HTTP/1.1 200 OK", r#"{"ret":0,"errcode":0,"msg_id":"m1"}"#).await;
    let api = client_for(&base_url);

    let req = build_text_message("o9cq80xy@im.wechat", "hi", None, Some("cid-3"));
    api.send_message(&req)
        .await
        .expect("ret=0 / errcode=0 是成功，不应报错");
}

/// 回归护栏：响应体不带业务码字段（部分端点如此）时视为成功。
#[tokio::test]
async fn send_message_accepts_response_without_code_fields() {
    let base_url = spawn_mock("HTTP/1.1 200 OK", r#"{"msg_id":"m2"}"#).await;
    let api = client_for(&base_url);

    let req = build_text_message("o9cq80xy@im.wechat", "hi", None, Some("cid-4"));
    api.send_message(&req).await.expect("无业务码字段时不应误判为失败");
}
