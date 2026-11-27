//! Hints that a working session may be stuck (`docs/specs/stuck-and-resume.md`):
//! the same tool call with the same result again and again, a run of
//! failed tool calls, or nothing from the agent for a long time. Only
//! hints: Ovra never stops an agent for them, the person decides.

use std::collections::{HashMap, VecDeque};
use std::hash::{DefaultHasher, Hash, Hasher};
use std::time::Duration;

use tokio::time::Instant;

use crate::Core;
use crate::core::WeakCore;
use crate::protocol::agents::{AgentMessage, AgentStatus, AgentsEvent, Stuck, ToolCall};

/// The same call with the same result this many times is a loop.
const REPEATS: u32 = 3;
/// This many failed calls in a row.
const FAILURES: u32 = 5;
/// The latest finished calls looked at for repeats.
const WINDOW: usize = 12;
/// Nothing from a working agent for this long.
pub(crate) const SILENT: Duration = Duration::from_secs(10 * 60);
/// Ovra's own IDE tools: a coordinator that asks about its workers again
/// while it waits for them is not stuck.
const IDE_TOOLS: &str = "mcp__ovra__";

/// One session's watch over its current turn.
#[derive(Debug)]
pub(crate) struct Watch {
    /// Calls of this turn that have not finished, by id.
    calls: HashMap<String, (String, ToolCall, u64)>,
    /// The latest finished calls: what they called and got, hashed.
    done: VecDeque<(u64, String, ToolCall)>,
    /// Failed calls in a row, up to now.
    failing: u32,
    /// When the agent last showed anything.
    last: Instant,
    silent: bool,
    /// The hint clients were last told of.
    shown: Option<Stuck>,
}

impl Default for Watch {
    fn default() -> Self {
        Self {
            calls: HashMap::new(),
            done: VecDeque::new(),
            failing: 0,
            last: Instant::now(),
            silent: false,
            shown: None,
        }
    }
}

impl Watch {
    /// What the hint is now.
    pub(crate) fn hint(&self) -> Option<Stuck> {
        if self.silent {
            return Some(Stuck::Silent {
                minutes: u32::try_from(SILENT.as_secs() / 60).unwrap_or(u32::MAX),
            });
        }
        self.repeating().or_else(|| {
            (self.failing >= FAILURES).then_some(Stuck::Failing {
                times: self.failing,
            })
        })
    }

    fn repeating(&self) -> Option<Stuck> {
        let mut counts: HashMap<u64, u32> = HashMap::new();
        let mut best: Option<(u32, &(u64, String, ToolCall))> = None;
        // The newest first, so that of equal counts the latest wins.
        for done in self.done.iter().rev() {
            let n = counts.entry(done.0).or_default();
            *n += 1;
            if *n >= REPEATS && best.is_none_or(|(times, _)| *n > times) {
                best = Some((*n, done));
            }
        }
        best.map(|(times, (_, name, call))| Stuck::Repeating {
            name: name.clone(),
            call: call.clone(),
            times,
        })
    }

    /// The hint, if it changed since clients were last told.
    pub(crate) fn changed(&mut self) -> Option<Option<Stuck>> {
        let hint = self.hint();
        if hint == self.shown {
            return None;
        }
        self.shown.clone_from(&hint);
        Some(hint)
    }

    /// The agent showed something (a message, a piece of text).
    pub(crate) fn active(&mut self) {
        self.last = Instant::now();
        self.silent = false;
    }

    /// Takes in a message of the transcript.
    pub(crate) fn see(&mut self, message: &AgentMessage) {
        self.active();
        match message {
            // The person's message starts a new turn.
            AgentMessage::UserText { .. } => self.reset(),
            AgentMessage::ToolUse {
                id,
                name,
                input,
                call,
                parent: None,
            } if !name.starts_with(IDE_TOOLS) => {
                let mut hash = DefaultHasher::new();
                name.hash(&mut hash);
                input.to_string().hash(&mut hash);
                self.calls
                    .insert(id.clone(), (name.clone(), call.clone(), hash.finish()));
            }
            AgentMessage::ToolResult {
                tool_use_id,
                output,
                is_error,
                parent: None,
            } => {
                let Some((name, call, hash)) = self.calls.remove(tool_use_id) else {
                    return;
                };
                self.failing = if *is_error { self.failing + 1 } else { 0 };
                let mut hash = {
                    let mut seeded = DefaultHasher::new();
                    hash.hash(&mut seeded);
                    seeded
                };
                output.hash(&mut hash);
                is_error.hash(&mut hash);
                self.done.push_back((hash.finish(), name, call));
                if self.done.len() > WINDOW {
                    self.done.pop_front();
                }
            }
            _ => {}
        }
    }

    /// The session changed state: a turn that ends takes its hint along.
    pub(crate) fn status(&mut self, status: &AgentStatus) {
        self.active();
        if !matches!(
            status,
            AgentStatus::Working | AgentStatus::WaitingForApproval
        ) {
            self.reset();
        }
    }

    fn reset(&mut self) {
        self.calls.clear();
        self.done.clear();
        self.failing = 0;
    }

    /// Marks it silent if it has been for [`SILENT`]; `Some` with the
    /// time to look again otherwise.
    pub(crate) fn silence(&mut self, now: Instant) -> Option<Instant> {
        if self.silent {
            return None;
        }
        let due = self.last + SILENT;
        if due <= now {
            self.silent = true;
            None
        } else {
            Some(due)
        }
    }
}

/// A session works: looks for silence while any does.
pub(crate) fn wake(core: &Core) {
    let Ok(runtime) = tokio::runtime::Handle::try_current() else {
        return;
    };
    let sessions = core.sessions();
    {
        let mut running = super::lock(&sessions.silence_watch);
        if *running {
            return;
        }
        *running = true;
    }
    runtime.spawn(watch(core.downgrade()));
}

async fn watch(core: WeakCore) {
    loop {
        let Some(core) = core.upgrade() else {
            return;
        };
        let sessions = core.sessions();
        let next = {
            // Under the flag `wake` takes, so a session that starts working
            // now either is seen here or starts a new watch.
            let mut running = super::lock(&sessions.silence_watch);
            let (changed, next) = sessions.silence(Instant::now());
            for (session, stuck) in changed {
                core.sink().emit(AgentsEvent::Stuck { session, stuck });
            }
            if next.is_none() {
                *running = false;
                return;
            }
            next
        };
        drop(core);
        if let Some(next) = next {
            tokio::time::sleep_until(next).await;
        }
    }
}

#[cfg(test)]
mod tests;
