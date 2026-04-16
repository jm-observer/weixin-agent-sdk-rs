//! Streaming Markdown filter with CJK awareness.

use regex::Regex;

/// Check if a character is a CJK ideograph.
fn is_cjk(c: char) -> bool {
    matches!(c,
        '\u{4E00}'..='\u{9FFF}' |
        '\u{3400}'..='\u{4DBF}' |
        '\u{3000}'..='\u{303F}' |
        '\u{3040}'..='\u{309F}' |
        '\u{30A0}'..='\u{30FF}' |
        '\u{AC00}'..='\u{D7AF}'
    )
}

/// Simple state machine for streaming processing.
#[derive(Debug, Default)]
pub struct StreamingMarkdownFilter {
    output: String,
    in_fence: bool,
}

impl StreamingMarkdownFilter {
    /// Create a new filter.
    pub fn new() -> Self {
        Self::default()
    }

    /// Feed a chunk of text.
    ///
    /// # Panics
    ///
    /// This method does not panic.
    pub fn feed(&mut self, chunk: &str) {
        let mut processed = String::new();
        let re_bold = Regex::new(r"\*\*(.+?)\*\*").unwrap();
        let re_italic = Regex::new(r"\*(.+?)\*").unwrap();

        for line in chunk.lines() {
            // Handle code fence start/end.
            if line.trim_start().starts_with("```") {
                self.in_fence = !self.in_fence;
                processed.push_str(line);
                processed.push('\n');
                continue;
            }
            if self.in_fence {
                processed.push_str(line);
                processed.push('\n');
                continue;
            }
            // Remove image lines.
            if line.trim_start().starts_with("![") {
                continue;
            }
            // Heading H5/H6: strip leading '#'.
            let trimmed = line.trim_start();
            if trimmed.starts_with("#####") || trimmed.starts_with("######") {
                let after = trimmed.trim_start_matches('#').trim_start();
                processed.push_str(after);
                processed.push('\n');
                continue;
            }
            // Process inline * and **
            let mut line_processed = line.to_owned();
            // ** pattern
            line_processed = re_bold
                .replace_all(&line_processed, |caps: &regex::Captures| {
                    let inner = &caps[1];
                    if inner.chars().any(is_cjk) {
                        inner.to_string()
                    } else {
                        caps[0].to_string()
                    }
                })
                .to_string();
            // * pattern (avoid matching already processed **)
            line_processed = re_italic
                .replace_all(&line_processed, |caps: &regex::Captures| {
                    let inner = &caps[1];
                    if inner.chars().any(is_cjk) {
                        inner.to_string()
                    } else {
                        caps[0].to_string()
                    }
                })
                .to_string();
            processed.push_str(&line_processed);
            processed.push('\n');
        }
        self.output.push_str(&processed);
    }

    /// Finish and obtain filtered text.
    pub fn finish(self) -> String {
        self.output
    }

    /// Convenience method for a full string.
    pub fn filter(text: &str) -> String {
        let mut filter = Self::new();
        filter.feed(text);
        filter.finish()
    }
}
