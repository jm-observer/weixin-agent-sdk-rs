//! Slash command handling for debugging and echo.

use crate::messaging::inbound::MessageContext;
use crate::messaging::debug_mode::{DebugMode, MessageTiming};

/// Result of slash command handling.
#[derive(Debug, PartialEq, Eq)]
pub enum SlashCommandResult {
    /// Command handled – no further processing.
    Handled,
    /// Not a slash command – continue normal flow.
    NotACommand,
}

/// Handle possible slash commands.
/// Returns `Handled` if the text is a recognized command.
pub async fn handle_slash_command(
    text: &str,
    ctx: &MessageContext,
    debug_mode: &DebugMode,
    timing: &MessageTiming,
) -> SlashCommandResult {
    let txt = text.trim();
    if txt.starts_with("/echo ") {
        let echo_text = &txt[6..];
        let timing_info = format!("\n\n---\nPlatform→SDK: {}ms", timing.platform_to_plugin_ms());
        let _ = ctx.reply_text(&format!("{}{}", echo_text, timing_info)).await;
        return SlashCommandResult::Handled;
    }
    if txt == "/toggle-debug" {
        let new_state = debug_mode.toggle();
        let status = if new_state { "ON" } else { "OFF" };
        let _ = ctx.reply_text(&format!("[Debug mode: {}]", status)).await;
        return SlashCommandResult::Handled;
    }
    SlashCommandResult::NotACommand
}
