//! Slash command handling for debugging and echo.

use crate::messaging::debug_mode::{DebugMode, MessageTiming};
use crate::messaging::inbound::MessageContext;

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
    let cmd_text = text.trim();
    if let Some(echo_text) = cmd_text.strip_prefix("/echo ") {
        let timing_info = format!("\n\n---\nPlatform→SDK: {}ms", timing.platform_to_plugin_ms());
        let _ = ctx.reply_text(&format!("{echo_text}{timing_info}")).await;
        return SlashCommandResult::Handled;
    }
    if cmd_text == "/toggle-debug" {
        let new_state = debug_mode.toggle();
        let status = if new_state { "ON" } else { "OFF" };
        let _ = ctx.reply_text(&format!("[Debug mode: {status}]")).await;
        return SlashCommandResult::Handled;
    }
    SlashCommandResult::NotACommand
}
