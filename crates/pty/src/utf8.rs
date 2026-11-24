//! Turns raw terminal output into UTF-8 text without splitting characters.

/// Decodes chunks of bytes. A multi-byte character cut off at the end of a
/// chunk is held back until the next one; invalid bytes become U+FFFD.
#[derive(Debug, Default)]
pub(crate) struct Utf8Stream {
    pending: Vec<u8>,
}

impl Utf8Stream {
    /// Text for everything decodable so far; may be empty.
    pub(crate) fn push(&mut self, bytes: &[u8]) -> String {
        self.pending.extend_from_slice(bytes);
        let mut out = String::new();
        let mut rest = self.pending.as_slice();
        loop {
            match std::str::from_utf8(rest) {
                Ok(text) => {
                    out.push_str(text);
                    rest = &[];
                    break;
                }
                Err(e) => {
                    let (valid, after) = rest.split_at(e.valid_up_to());
                    // `valid_up_to` guarantees the prefix is UTF-8.
                    out.push_str(std::str::from_utf8(valid).unwrap_or_default());
                    match e.error_len() {
                        Some(bad) => {
                            out.push(char::REPLACEMENT_CHARACTER);
                            rest = &after[bad..];
                        }
                        // An incomplete character at the end: wait for more.
                        None => {
                            rest = after;
                            break;
                        }
                    }
                }
            }
        }
        self.pending = rest.to_vec();
        out
    }

    /// Text for bytes still held back when the stream ends.
    pub(crate) fn finish(&mut self) -> String {
        let rest = std::mem::take(&mut self.pending);
        String::from_utf8_lossy(&rest).into_owned()
    }
}

#[cfg(test)]
mod tests {
    use super::Utf8Stream;

    #[test]
    fn holds_back_a_character_split_across_chunks() {
        let bytes = "aé€😀b".as_bytes();
        // Every split point, including inside each multi-byte character.
        for cut in 0..=bytes.len() {
            let mut stream = Utf8Stream::default();
            let mut text = stream.push(&bytes[..cut]);
            assert!(
                text.chars().all(|c| c != char::REPLACEMENT_CHARACTER),
                "{cut}"
            );
            text += &stream.push(&bytes[cut..]);
            text += &stream.finish();
            assert_eq!(text, "aé€😀b", "{cut}");
        }
    }

    #[test]
    fn replaces_invalid_bytes_and_a_truncated_tail() {
        let mut stream = Utf8Stream::default();
        assert_eq!(stream.push(b"a\xffb\xe2\x82"), "a\u{fffd}b");
        assert_eq!(stream.finish(), "\u{fffd}");
        assert_eq!(stream.finish(), "");
    }
}
