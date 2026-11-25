//! The control protocol between Ovra and a pod: JSON lines over
//! `<pod dir>/control.sock`. Ovra sends [`Request`]s, the pod answers with
//! [`Event`]s. `id` is Ovra's name for one spawn, unique per pod.

use serde::{Deserialize, Serialize};

use ovra_core::services::CommandLine;

/// The control socket's file name in the pod's folder.
pub(crate) const CONTROL: &str = "control.sock";

/// What pod-init prints on stdout once the control socket listens.
pub(crate) const LISTENING: &str = "ovra-pod: listening";

/// The longest line of server output in one event; longer lines are split.
pub(crate) const MAX_OUTPUT_LINE: usize = 8 * 1024;

/// The longest control line either side reads. Room for an output line of
/// [`MAX_OUTPUT_LINE`] bytes with every byte escaped.
pub(crate) const MAX_CONTROL_LINE: usize = 7 * MAX_OUTPUT_LINE;

/// The file name of the socket that relays to `port` inside the pod.
pub(crate) fn port_socket(port: u16) -> String {
    format!("{port}.sock")
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum Request {
    Spawn(Spawn),
    Signal { id: String, sig: Sig },
}

/// One server or setup step to start, in its own process group.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct Spawn {
    pub(crate) id: String,
    /// The program, then its arguments.
    pub(crate) argv: Vec<String>,
    /// Variables to set (`Some`) or remove (`None`).
    pub(crate) env: Vec<(String, Option<String>)>,
    pub(crate) cwd: String,
    /// The server's port; a setup step has none.
    pub(crate) port: Option<u16>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum Sig {
    Term,
    Kill,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum Event {
    /// One line of stdout and stderr merged, without its line ending.
    Output { id: String, line: String },
    /// `127.0.0.1:<port>` inside the pod accepted a connection; its
    /// `<port>.sock` relays there.
    Listening { id: String, port: u16 },
    /// `code` is missing when a signal ended it.
    Exited { id: String, code: Option<i32> },
    /// It never ran.
    Failed { id: String, reason: String },
}

impl Spawn {
    /// The spawn of `line`. The control protocol is JSON, so every part of
    /// it must be UTF-8.
    pub(crate) fn new(id: String, line: &CommandLine, port: Option<u16>) -> Result<Self, String> {
        let text = |s: &std::ffi::OsStr| {
            s.to_str()
                .map(str::to_owned)
                .ok_or_else(|| format!("not UTF-8: {}", s.to_string_lossy()))
        };
        let mut argv = vec![text(line.program.as_os_str())?];
        for arg in &line.args {
            argv.push(text(arg)?);
        }
        let env = line
            .env
            .iter()
            .map(|(k, v)| Ok((text(k)?, v.as_deref().map(text).transpose()?)))
            .collect::<Result<_, String>>()?;
        Ok(Self {
            id,
            argv,
            env,
            cwd: text(line.cwd.as_os_str())?,
            port,
        })
    }
}

/// One message as a line, with its `\n`.
pub(crate) fn encode<T: Serialize>(message: &T) -> Vec<u8> {
    // Plain enums of strings and numbers always serialize.
    let mut line = serde_json::to_vec(message).unwrap_or_default();
    line.push(b'\n');
    line
}

/// Splits a stream of bytes into lines of at most `max` bytes: a longer
/// line comes out in pieces. Line endings (`\n`, `\r\n`) are removed.
#[derive(Debug, Default)]
pub(crate) struct Lines {
    pending: Vec<u8>,
}

impl Lines {
    /// Adds `bytes` and returns the lines they complete.
    pub(crate) fn push(&mut self, bytes: &[u8], max: usize) -> Vec<Vec<u8>> {
        let mut out = Vec::new();
        for &b in bytes {
            if b == b'\n' {
                let mut line = std::mem::take(&mut self.pending);
                if line.last() == Some(&b'\r') {
                    line.pop();
                }
                out.push(line);
            } else {
                self.pending.push(b);
                if self.pending.len() >= max {
                    out.push(std::mem::take(&mut self.pending));
                }
            }
        }
        out
    }

    /// What is left at the end of the stream, if anything.
    pub(crate) fn finish(self) -> Option<Vec<u8>> {
        (!self.pending.is_empty()).then_some(self.pending)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wire_format_is_stable() {
        let spawn = Spawn {
            id: "3".into(),
            argv: vec!["pnpm".into(), "dev".into()],
            env: vec![("PORT".into(), Some("5173".into())), ("CI".into(), None)],
            cwd: "/w/ui".into(),
            port: Some(5173),
        };
        let requests = [
            Request::Spawn(spawn),
            Request::Signal {
                id: "3".into(),
                sig: Sig::Term,
            },
        ];
        let events = [
            Event::Output {
                id: "3".into(),
                line: "ready in 300 ms".into(),
            },
            Event::Listening {
                id: "3".into(),
                port: 5173,
            },
            Event::Exited {
                id: "3".into(),
                code: None,
            },
            Event::Failed {
                id: "4".into(),
                reason: "port 5173 is taken in this pod".into(),
            },
        ];
        let lines = |all: Vec<Vec<u8>>| -> Vec<String> {
            all.into_iter()
                .map(|l| String::from_utf8_lossy(&l).into_owned())
                .collect()
        };
        insta::assert_debug_snapshot!((
            lines(requests.iter().map(encode).collect()),
            lines(events.iter().map(encode).collect()),
        ));
        for request in requests {
            let line = encode(&request);
            assert_eq!(serde_json::from_slice(&line).ok(), Some(request));
        }
    }

    #[test]
    fn a_command_line_becomes_a_spawn_if_it_is_utf8() {
        use std::os::unix::ffi::OsStringExt as _;
        let mut line = CommandLine {
            program: "pnpm".into(),
            args: vec!["dev".into()],
            env: vec![("PORT".into(), Some("5173".into())), ("CI".into(), None)],
            cwd: "/w/ui".into(),
        };
        let spawn = Spawn::new("3".into(), &line, Some(5173)).expect("utf-8");
        assert_eq!(spawn.argv, ["pnpm", "dev"]);
        assert_eq!(spawn.env[1], ("CI".into(), None));
        assert_eq!(spawn.cwd, "/w/ui");
        line.args.push(std::ffi::OsString::from_vec(vec![0xff]));
        assert!(Spawn::new("1".into(), &line, None).is_err());
    }

    #[test]
    fn lines_are_split_at_endings_and_at_the_limit() {
        let mut lines = Lines::default();
        assert_eq!(lines.push(b"ab", 5), Vec::<Vec<u8>>::new());
        assert_eq!(lines.push(b"c\r\nd\n\n", 5), [&b"abc"[..], b"d", b""]);
        assert_eq!(lines.push(b"123456789ab", 5), [b"12345", b"6789a"]);
        assert_eq!(lines.finish(), Some(b"b".to_vec()));
        assert_eq!(Lines::default().finish(), None);
    }
}
