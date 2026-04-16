//! QR code login demo – displays the QR code for scanning.
//! Run with:
//!   cargo run --example qr_demo
//!
//! This example shows how to perform a standalone QR login using the
//! `StandaloneQrLogin` API and prints the QR code image content. The QR code
//! is displayed both as the raw image URL/contents and as an ASCII QR in the
//! terminal (using the `qr2term` crate). After a successful login, the bot token
//! is printed.

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // Initialise a minimal logger so that any internal logs are shown.
    tracing_subscriber::fmt().with_env_filter("info").init();

    // Build a configuration without a token – the QR login does not need a
    // pre‑existing token.
    let config = weixin_agent::WeixinConfig::builder().token("").build()?;
    let qr = weixin_agent::StandaloneQrLogin::new(&config);

    // Start a QR login session. `None` means the default bot type.
    let mut session = qr.start(None).await?;

    // Show the QR code. The SDK returns the QR code image URL/content in
    // `session.qrcode_img_content`. We print it directly and also try to render
    // an ASCII QR in the terminal for convenience.
    println!("\n请扫描二维码 (URL/Content): {}", session.qrcode_img_content);
    // Try to render a terminal QR using the `qr2term` crate. Errors are ignored –
    // the raw URL is still useful for manual scanning.
    let _ = qr2term::print_qr(&session.qrcode_img_content);

    // Poll the login status until the login is confirmed. The loop mirrors the
    // snippet from the README.
    loop {
        match qr.poll_status(&session).await? {
            weixin_agent::LoginStatus::Confirmed { bot_token, .. } => {
                println!("\n✅ 登录成功! token: {}", bot_token);
                break;
            }
            weixin_agent::LoginStatus::Expired => {
                println!("二维码已失效，重新获取…");
                session = qr.start(None).await?;
                println!("请扫描新的二维码: {}", session.qrcode_img_content);
                let _ = qr2term::print_qr(&session.qrcode_img_content);
            }
            _ => {
                // Other states (Wait, Scanned, ScannedButRedirect) – just wait a
                // little before polling again.
                tokio::time::sleep(std::time::Duration::from_secs(2)).await;
            }
        }
    }

    Ok(())
}
