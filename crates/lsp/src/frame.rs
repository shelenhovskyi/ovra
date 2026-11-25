//! The LSP base protocol: `Content-Length: N\r\n\r\n` and N bytes of JSON.

use serde_json::Value;

/// A larger message ends the server (`docs/specs/lsp.md`, "Limits"). As a
/// `serde_json::Value` a message can take ~16 times its length.
pub(crate) const MAX_MESSAGE: usize = 4 << 20;

/// Headers longer than this are not a header.
const MAX_HEADER: usize = 4096;

/// One framed message.
pub(crate) fn encode(message: &Value) -> Vec<u8> {
    let body = message.to_string();
    let mut bytes = format!("Content-Length: {}\r\n\r\n", body.len()).into_bytes();
    bytes.extend_from_slice(body.as_bytes());
    bytes
}

/// Turns the bytes a server writes, in whatever pieces they come, into
/// messages. An error is final: the stream cannot be trusted after it.
#[derive(Debug, Default)]
pub(crate) struct Decoder {
    buf: Vec<u8>,
}

impl Decoder {
    pub(crate) fn push(&mut self, bytes: &[u8]) {
        self.buf.extend_from_slice(bytes);
    }

    /// The next whole message and its length in bytes, `None` until more
    /// bytes come.
    pub(crate) fn next(&mut self) -> Result<Option<(Value, usize)>, String> {
        let Some(end) = self.buf.windows(4).position(|w| w == b"\r\n\r\n") else {
            if self.buf.len() > MAX_HEADER {
                return Err("the server wrote a header that never ends".into());
            }
            return Ok(None);
        };
        if end > MAX_HEADER {
            return Err("the server wrote a header that is too long".into());
        }
        let length = content_length(&self.buf[..end])?;
        if length > MAX_MESSAGE {
            return Err(format!(
                "the server wrote a message of {length} bytes, over the limit of {MAX_MESSAGE}"
            ));
        }
        let start = end + 4;
        if self.buf.len() < start + length {
            return Ok(None);
        }
        let message = serde_json::from_slice(&self.buf[start..start + length])
            .map_err(|e| format!("the server wrote a message that is not JSON: {e}"));
        self.buf.drain(..start + length);
        message.map(|m| Some((m, length)))
    }
}

fn content_length(header: &[u8]) -> Result<usize, String> {
    let bad = || "the server wrote a bad header".to_owned();
    let header = std::str::from_utf8(header).map_err(|_| bad())?;
    let mut length = None;
    for line in header.split("\r\n") {
        let (name, value) = line.split_once(':').ok_or_else(bad)?;
        if name.trim().eq_ignore_ascii_case("content-length") {
            length = Some(value.trim().parse::<usize>().map_err(|_| bad())?);
        }
    }
    length.ok_or_else(bad)
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    fn all(decoder: &mut Decoder) -> Result<Vec<Value>, String> {
        let mut out = Vec::new();
        while let Some((message, length)) = decoder.next()? {
            assert_eq!(length, message.to_string().len());
            out.push(message);
        }
        Ok(out)
    }

    #[test]
    fn reads_a_message_split_across_reads() {
        let bytes = encode(&json!({"id": 1, "text": "é"}));
        let mut decoder = Decoder::default();
        for piece in bytes.chunks(3) {
            assert_eq!(all(&mut decoder), Ok(vec![]));
            decoder.push(piece);
        }
        assert_eq!(all(&mut decoder), Ok(vec![json!({"id": 1, "text": "é"})]));
        assert_eq!(all(&mut decoder), Ok(vec![]));
    }

    #[test]
    fn reads_two_messages_in_one_read_with_other_headers() {
        let mut bytes = b"content-type: application/vscode-jsonrpc; charset=utf-8\r\n\
            CONTENT-LENGTH: 8\r\n\r\n{\"id\":1}"
            .to_vec();
        bytes.extend(encode(&json!({"id": 2})));
        let mut decoder = Decoder::default();
        decoder.push(&bytes);
        assert_eq!(
            all(&mut decoder),
            Ok(vec![json!({"id": 1}), json!({"id": 2})])
        );
    }

    #[test]
    fn refuses_a_message_over_the_limit_before_it_arrives() {
        let mut decoder = Decoder::default();
        decoder.push(format!("Content-Length: {}\r\n\r\n{{", MAX_MESSAGE + 1).as_bytes());
        assert!(decoder.next().is_err_and(|e| e.contains("over the limit")));
    }

    #[test]
    fn refuses_bad_headers_and_bodies() {
        for bytes in [
            &b"Content-Length: ten\r\n\r\n"[..],
            b"Content-Type: x\r\n\r\n{}",
            b"no colon\r\n\r\n",
            b"Content-Length: 3\r\n\r\nnot",
        ] {
            let mut decoder = Decoder::default();
            decoder.push(bytes);
            assert!(
                decoder.next().is_err(),
                "{}",
                String::from_utf8_lossy(bytes)
            );
        }
        let mut decoder = Decoder::default();
        decoder.push(&[b'x'; MAX_HEADER + 1]);
        assert!(decoder.next().is_err());
    }
}
