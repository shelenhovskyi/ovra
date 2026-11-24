//! The kept tail of a terminal's output, numbered like its output events.

use ovra_core::protocol::terminals::{TerminalId, TerminalsEvent};
use ovra_core::services::EventSink;

/// How much output a terminal keeps for `terminals.scrollback`.
pub(crate) const KEEP_BYTES: usize = 1024 * 1024;

#[derive(Debug, Default)]
pub(crate) struct Scrollback {
    text: String,
    seq: u64,
}

impl Scrollback {
    /// Numbers `data`, keeps it and emits it. Callers hold the lock around
    /// this, so a scrollback answer and the events after it never overlap.
    pub(crate) fn emit(&mut self, sink: &EventSink, id: &TerminalId, data: String) {
        if data.is_empty() {
            return;
        }
        self.seq += 1;
        self.text.push_str(&data);
        // Trim in steps of half the size so long floods copy little.
        if self.text.len() > KEEP_BYTES + KEEP_BYTES / 2 {
            let cut = tail_start(&self.text, KEEP_BYTES);
            self.text.drain(..cut);
        }
        sink.emit(TerminalsEvent::Output {
            id: id.clone(),
            data,
            seq: self.seq,
        });
    }

    /// At most `KEEP_BYTES` of the latest output, and the last `seq` in it.
    pub(crate) fn tail(&self) -> (String, u64) {
        let start = tail_start(&self.text, KEEP_BYTES);
        (self.text[start..].to_owned(), self.seq)
    }
}

/// Where the last `keep` bytes of `text` start, moved forward to a
/// character boundary.
fn tail_start(text: &str, keep: usize) -> usize {
    let mut start = text.len().saturating_sub(keep);
    while !text.is_char_boundary(start) {
        start += 1;
    }
    start
}

#[cfg(test)]
mod tests {
    use ovra_core::Core;

    use super::*;

    #[test]
    fn keeps_the_latest_output_on_a_character_boundary() {
        let core = Core::new("test");
        let mut rx = core.subscribe();
        let id = TerminalId("t1".into());
        let mut scrollback = Scrollback::default();
        assert_eq!(scrollback.tail(), (String::new(), 0));

        scrollback.emit(&core.sink(), &id, String::new());
        scrollback.emit(&core.sink(), &id, "é".repeat(KEEP_BYTES / 2 + 1));
        scrollback.emit(&core.sink(), &id, "end".into());
        let (text, seq) = scrollback.tail();
        assert_eq!(seq, 2);
        assert!(text.ends_with("éend"));
        // The character the limit splits is dropped whole.
        assert_eq!(text.len(), KEEP_BYTES - 1);

        for _ in 0..3 {
            scrollback.emit(&core.sink(), &id, "x".repeat(KEEP_BYTES / 2));
        }
        let (text, seq) = scrollback.tail();
        assert_eq!((text.len(), seq), (KEEP_BYTES, 5));
        assert!(text.chars().all(|c| c == 'x'));

        let seqs: Vec<u64> = std::iter::from_fn(|| rx.try_recv().ok())
            .filter_map(|e| match e {
                ovra_core::protocol::Event::Terminals(TerminalsEvent::Output { seq, .. }) => {
                    Some(seq)
                }
                _ => None,
            })
            .collect();
        assert_eq!(seqs, [1, 2, 3, 4, 5]);
    }
}
