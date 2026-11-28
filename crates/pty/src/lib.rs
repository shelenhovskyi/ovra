//! Shells in pseudo-terminals, one per terminal tab.
//! Contract: `ovra_core::services::TerminalService`; fake:
//! `ovra_core::fakes::EchoTerminals`.
//!
//! Each terminal runs the user's login shell (`$SHELL` on Unix, `%ComSpec%`
//! on Windows) through `portable-pty`, which uses ConPTY on Windows. Three
//! threads serve it: one waits for the shell to end and drops the
//! pseudo-terminal, the others read and emit output (`output.rs`).

mod output;
mod scrollback;
mod utf8;

use std::collections::HashMap;
use std::io::Write;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::thread;

use ovra_core::protocol::terminals::{TermSize, TerminalId};
use ovra_core::protocol::{ErrorCode, ProtocolError};
use ovra_core::services::{EventSink, Result, TerminalService};
use portable_pty::{ChildKiller, CommandBuilder, MasterPty, PtySize, native_pty_system};

use crate::output::Chunk;
use crate::scrollback::Scrollback;

/// The real terminal service.
#[derive(Debug, Default)]
pub struct Pty {
    terminals: Arc<Mutex<HashMap<TerminalId, Terminal>>>,
    opened: AtomicU64,
    /// Ends every id, so ids differ between runs: a front end that kept an
    /// id across an app restart gets `not_found`, not another shell. Empty
    /// in tests.
    run: String,
    /// Program and arguments to run instead of the user's shell.
    #[cfg(test)]
    program: Option<Vec<String>>,
}

/// An open terminal; `session` is gone once the shell ended.
#[derive(Debug)]
struct Terminal {
    session: Option<Session>,
    output: Arc<Mutex<Scrollback>>,
}

struct Session {
    master: Box<dyn MasterPty + Send>,
    writer: Box<dyn Write + Send>,
    killer: Box<dyn ChildKiller + Send + Sync>,
}

impl std::fmt::Debug for Session {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Session").finish_non_exhaustive()
    }
}

impl Pty {
    pub fn new() -> Self {
        // The whole time, not its sub-second part, and apart from the pid:
        // pid 0x1a at 0xbc and pid 0x1 at 0xabc must not read the same.
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |d| d.as_nanos());
        Self {
            run: format!("-{:x}-{nanos:x}", std::process::id()),
            ..Self::default()
        }
    }

    #[cfg(test)]
    fn with_program(program: &[&str]) -> Self {
        Self {
            program: Some(program.iter().map(|s| (*s).to_owned()).collect()),
            ..Self::default()
        }
    }

    fn command(&self) -> CommandBuilder {
        #[cfg(test)]
        if let Some(program) = &self.program {
            let mut cmd = CommandBuilder::from_argv(program.iter().map(Into::into).collect());
            // Keep test shells free of the developer's rc files.
            cmd.env_remove("ENV");
            cmd.env("PS1", "$ ");
            return cmd;
        }
        let mut cmd = CommandBuilder::new_default_prog();
        if let Some(path) = ovra_core::child_path::login() {
            cmd.env("PATH", path);
        }
        cmd
    }

    fn terminals(&self) -> MutexGuard<'_, HashMap<TerminalId, Terminal>> {
        lock(&self.terminals)
    }

    /// Runs `f` on the shell of a terminal that still runs.
    fn with_session<T>(
        &self,
        id: &TerminalId,
        f: impl FnOnce(&mut Session) -> std::io::Result<T>,
    ) -> Result<T> {
        let mut terminals = self.terminals();
        let session = terminals
            .get_mut(id)
            .and_then(|t| t.session.as_mut())
            .ok_or_else(|| not_open(id))?;
        f(session).map_err(|e| internal(format!("terminal {}: {e}", id.0)))
    }
}

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

fn not_open(id: &TerminalId) -> ProtocolError {
    ProtocolError::new(
        ErrorCode::NotFound,
        format!("terminal {} is not open", id.0),
    )
}

fn internal(message: String) -> ProtocolError {
    ProtocolError::new(ErrorCode::Internal, message)
}

fn pty_size(size: TermSize) -> PtySize {
    PtySize {
        rows: size.rows.max(1),
        cols: size.cols.max(1),
        pixel_width: 0,
        pixel_height: 0,
    }
}

impl TerminalService for Pty {
    fn open(&self, cwd: PathBuf, size: TermSize, sink: EventSink) -> Result<TerminalId> {
        if !cwd.is_dir() {
            return Err(ProtocolError::new(
                ErrorCode::NotFound,
                format!("folder {} does not exist", cwd.display()),
            ));
        }
        let pair = native_pty_system()
            .openpty(pty_size(size))
            .map_err(|e| internal(format!("cannot open a terminal: {e}")))?;
        let mut cmd = self.command();
        cmd.cwd(&cwd);
        cmd.env("TERM", "xterm-256color");
        cmd.env("COLORTERM", "truecolor");
        let mut child = pair
            .slave
            .spawn_command(cmd)
            .map_err(|e| internal(format!("cannot start the shell: {e}")))?;
        // The shell holds the only other end; output ends when it exits.
        drop(pair.slave);
        let io = pair
            .master
            .try_clone_reader()
            .and_then(|r| Ok((r, pair.master.take_writer()?)));
        let (reader, writer) = match io {
            Ok(io) => io,
            Err(e) => {
                let _ = child.kill();
                return Err(internal(format!("cannot open a terminal: {e}")));
            }
        };

        let id = TerminalId(format!(
            "t{}{}",
            self.opened.fetch_add(1, Ordering::Relaxed) + 1,
            self.run
        ));
        let killer = child.clone_killer();
        let output = Arc::new(Mutex::new(Scrollback::default()));
        self.terminals().insert(
            id.clone(),
            Terminal {
                session: Some(Session {
                    master: pair.master,
                    writer,
                    killer,
                }),
                output: Arc::clone(&output),
            },
        );

        let exited = output::serve(reader, id.clone(), sink, output);
        let terminals = Arc::clone(&self.terminals);
        let waiter_id = id.clone();
        thread::spawn(move || {
            let id = waiter_id;
            let status = child.wait();
            // Dropping the master ends the reader on Windows (ConPTY). The
            // scrollback stays until the terminal is closed.
            let session = lock(&terminals).get_mut(&id).and_then(|t| t.session.take());
            drop(session);
            let code = match status {
                Ok(s) if s.signal().is_none() => i32::try_from(s.exit_code()).ok(),
                _ => None,
            };
            // The output side emits `terminals.exited` after the last output.
            let _ = exited.send(Chunk::Exited(code));
        });
        Ok(id)
    }

    fn input(&self, id: &TerminalId, data: &str) -> Result<()> {
        self.with_session(id, |s| {
            s.writer.write_all(data.as_bytes())?;
            s.writer.flush()
        })
    }

    fn resize(&self, id: &TerminalId, size: TermSize) -> Result<()> {
        self.with_session(id, |s| {
            s.master
                .resize(pty_size(size))
                .map_err(|e| std::io::Error::other(e.to_string()))
        })
    }

    fn close(&self, id: &TerminalId) -> Result<()> {
        let terminal = self.terminals().remove(id).ok_or_else(|| not_open(id))?;
        match terminal.session {
            Some(mut s) => s
                .killer
                .kill()
                .map_err(|e| internal(format!("terminal {}: {e}", id.0))),
            None => Ok(()),
        }
    }

    fn running(&self) -> usize {
        self.terminals()
            .values()
            .filter(|t| t.session.is_some())
            .count()
    }

    fn scrollback(&self, id: &TerminalId) -> Result<(String, u64)> {
        let output = self
            .terminals()
            .get(id)
            .map(|t| Arc::clone(&t.output))
            .ok_or_else(|| not_open(id))?;
        let tail = lock(&output).tail();
        Ok(tail)
    }
}

#[cfg(test)]
mod tests;
