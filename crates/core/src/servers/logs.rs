//! A server's output: its last 1 MiB, numbered by line from 1, kept across
//! its restarts.

use std::collections::VecDeque;

use crate::protocol::servers::{LogLine, LogPage};

/// Text bytes kept per server.
pub(crate) const MAX_LOG_BYTES: usize = 1 << 20;
/// A longer line is cut, so one line cannot push out all others.
const MAX_LINE_BYTES: usize = 16 * 1024;
/// Most lines one page holds, and how many without a `limit`.
pub(crate) const MAX_PAGE: usize = 500;
const DEFAULT_PAGE: usize = 100;

#[derive(Debug, Default)]
pub(crate) struct Log {
    lines: VecDeque<LogLine>,
    bytes: usize,
    last: u64,
}

impl Log {
    /// Adds a line, dropping the oldest ones over [`MAX_LOG_BYTES`].
    pub(crate) fn push(&mut self, mut text: String) -> LogLine {
        if text.len() > MAX_LINE_BYTES {
            let mut cut = MAX_LINE_BYTES;
            while !text.is_char_boundary(cut) {
                cut -= 1;
            }
            text.truncate(cut);
        }
        self.last += 1;
        let line = LogLine { n: self.last, text };
        self.bytes += line.text.len();
        self.lines.push_back(line.clone());
        while self.bytes > MAX_LOG_BYTES {
            let Some(old) = self.lines.pop_front() else {
                break;
            };
            self.bytes -= old.text.len();
        }
        line
    }

    /// Up to `limit` lines numbered above `since` that contain `grep`
    /// (in their text without ANSI escapes), without ANSI escapes when
    /// `plain`. `next` continues after the last line looked at.
    pub(crate) fn page(
        &self,
        since: u64,
        limit: Option<usize>,
        grep: Option<&str>,
        plain: bool,
    ) -> LogPage {
        let limit = limit.unwrap_or(DEFAULT_PAGE).clamp(1, MAX_PAGE);
        let grep = grep.filter(|g| !g.is_empty());
        let start = self.lines.partition_point(|l| l.n <= since);
        let mut page = LogPage {
            lines: Vec::new(),
            next: since,
        };
        for line in self.lines.range(start..) {
            page.next = line.n;
            let text = if plain || grep.is_some() {
                strip_ansi(&line.text)
            } else {
                line.text.clone()
            };
            if grep.is_some_and(|g| !text.contains(g)) {
                continue;
            }
            page.lines.push(LogLine {
                n: line.n,
                text: if plain { text } else { line.text.clone() },
            });
            if page.lines.len() == limit {
                break;
            }
        }
        page
    }
}

/// `text` without terminal escapes (CSI and OSC sequences, two-character
/// escapes) and control characters other than tabs: what agents read.
pub(crate) fn strip_ansi(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut chars = text.chars();
    while let Some(c) = chars.next() {
        match c {
            '\u{1b}' => match chars.next() {
                // Parameters and intermediates up to a final byte.
                Some('[') => {
                    for c in chars.by_ref() {
                        if ('\u{40}'..='\u{7e}').contains(&c) {
                            break;
                        }
                    }
                }
                // Up to BEL or ESC \.
                Some(']') => {
                    while let Some(c) = chars.next() {
                        if c == '\u{7}' {
                            break;
                        }
                        if c == '\u{1b}' {
                            chars.next();
                            break;
                        }
                    }
                }
                _ => {}
            },
            '\t' => out.push(c),
            c if c.is_control() => {}
            c => out.push(c),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn texts(page: &LogPage) -> Vec<(u64, &str)> {
        page.lines.iter().map(|l| (l.n, l.text.as_str())).collect()
    }

    #[test]
    fn pages_continue_where_the_last_one_ended() {
        let mut log = Log::default();
        for n in 1..=5 {
            log.push(format!("line {n}"));
        }
        let first = log.page(0, Some(2), None, false);
        assert_eq!(texts(&first), [(1, "line 1"), (2, "line 2")]);
        let rest = log.page(first.next, None, None, false);
        assert_eq!(texts(&rest), [(3, "line 3"), (4, "line 4"), (5, "line 5")]);
        assert_eq!(rest.next, 5);
        let none = log.page(rest.next, None, None, false);
        assert_eq!((none.lines.len(), none.next), (0, 5));
    }

    #[test]
    fn grep_matches_the_text_without_escapes_and_pages_past_misses() {
        let mut log = Log::default();
        log.push("\u{1b}[32mready\u{1b}[0m in 300 ms".to_owned());
        log.push("error: one".to_owned());
        log.push("fine".to_owned());
        log.push("\u{1b}]0;title\u{7}error: two\r".to_owned());
        let page = log.page(0, Some(1), Some("error"), false);
        assert_eq!(texts(&page), [(2, "error: one")]);
        let page = log.page(page.next, Some(1), Some("error"), true);
        assert_eq!(texts(&page), [(4, "error: two")]);
        // The UI gets the escapes, agents do not.
        let raw = log.page(0, Some(1), Some("ready in"), false);
        assert_eq!(raw.lines[0].text, "\u{1b}[32mready\u{1b}[0m in 300 ms");
        let plain = log.page(0, Some(1), None, true);
        assert_eq!(plain.lines[0].text, "ready in 300 ms");
        // A grep past the end looks at nothing and stays put.
        assert_eq!(log.page(4, None, Some("x"), false).next, 4);
    }

    #[test]
    fn the_ring_keeps_the_newest_mebibyte() {
        let mut log = Log::default();
        let line = "x".repeat(1023);
        for _ in 0..1100 {
            log.push(line.clone());
        }
        assert!(log.bytes <= MAX_LOG_BYTES);
        let page = log.page(0, Some(1), None, false);
        // 1025 lines of 1023 bytes fit; the oldest 75 went.
        assert_eq!(page.lines[0].n, 76);
        assert_eq!(log.page(1099, None, None, false).lines[0].n, 1100);
        // A huge line is cut, never kept whole.
        let huge = log.push("é".repeat(MAX_LINE_BYTES));
        assert!(huge.text.len() <= MAX_LINE_BYTES && huge.text.len() > MAX_LINE_BYTES - 2);
        assert_eq!(
            log.page(0, Some(MAX_PAGE + 1), None, false).lines.len(),
            MAX_PAGE
        );
    }
}
