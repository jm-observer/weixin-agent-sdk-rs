//! Debug mode state and message timing utilities.

use std::sync::atomic::{AtomicBool, Ordering};

/// Debug mode manager – per‑client instance.
pub struct DebugMode {
    enabled: AtomicBool,
}

impl DebugMode {
    /// Create a new instance, initially disabled.
    pub fn new() -> Self {
        Self {
            enabled: AtomicBool::new(false),
        }
    }

    /// Toggle the debug flag and return the new state.
    pub fn toggle(&self) -> bool {
        let prev = self.enabled.fetch_xor(true, Ordering::Relaxed);
        !prev
    }

    /// Query whether debug mode is currently enabled.
    pub fn is_enabled(&self) -> bool {
        self.enabled.load(Ordering::Relaxed)
    }
}

impl Default for DebugMode {
    fn default() -> Self {
        Self::new()
    }
}

/// Timing information for a single message processing lifecycle.
#[derive(Debug, Clone)]
pub struct MessageTiming {
    /// Message creation timestamp on the platform (ms since epoch).
    pub event_timestamp_ms: i64,
    /// When the SDK received the message (ms since epoch).
    pub received_at_ms: u64,
    /// When inbound processing completed (optional, ms since epoch).
    pub inbound_done_ms: Option<u64>,
    /// When the reply was sent (optional, ms since epoch).
    pub reply_done_ms: Option<u64>,
}

impl MessageTiming {
    /// Milliseconds from platform to SDK (network latency).
    pub fn platform_to_plugin_ms(&self) -> u64 {
        self.received_at_ms
            .saturating_sub(self.event_timestamp_ms.try_into().unwrap_or(0))
    }

    /// Milliseconds spent in inbound processing.
    pub fn inbound_processing_ms(&self) -> Option<u64> {
        self.inbound_done_ms.map(|t| t.saturating_sub(self.received_at_ms))
    }

    /// Total elapsed time from SDK receipt to reply completion.
    pub fn total_ms(&self) -> Option<u64> {
        self.reply_done_ms.map(|t| t.saturating_sub(self.received_at_ms))
    }

    /// Generate a human‑readable report.
    pub fn format_report(&self) -> String {
        let mut parts = Vec::new();
        parts.push(format!("Platform→Plugin: {}ms", self.platform_to_plugin_ms()));
        if let Some(inb) = self.inbound_processing_ms() {
            parts.push(format!("Inbound processing: {inb}ms"));
        }
        if let Some(total) = self.total_ms() {
            parts.push(format!("Total: {total}ms"));
        }
        format!("\n\n---\n{}", parts.join("\n"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn toggle_works() {
        let dm = DebugMode::new();
        assert!(!dm.is_enabled());
        assert!(dm.toggle());
        assert!(dm.is_enabled());
        assert!(!dm.toggle());
        assert!(!dm.is_enabled());
    }

    #[test]
    fn timing_report() {
        let timing = MessageTiming {
            event_timestamp_ms: 1000,
            received_at_ms: 1100,
            inbound_done_ms: Some(1150),
            reply_done_ms: Some(1200),
        };
        let report = timing.format_report();
        assert!(report.contains("Platform→Plugin: 100ms"));
        assert!(report.contains("Inbound processing: 50ms"));
        assert!(report.contains("Total: 100ms"));
    }
}
