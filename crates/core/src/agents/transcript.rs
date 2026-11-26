//! A session's transcript, kept within memory limits. The agent keeps its own
//! full history (Claude Code: its session file), so the core keeps only what
//! the UI shows.

use std::collections::VecDeque;

use crate::protocol::agents::{AgentMessage, ToolCall, TurnUsage};

/// Messages kept per session; the oldest go first.
pub(crate) const MAX_MESSAGES: usize = 5_000;
/// Longest text kept in one message, and the longest prompt accepted.
pub(crate) const MAX_TEXT_BYTES: usize = 64 * 1024;

#[derive(Debug, Default)]
pub(crate) struct Transcript {
    messages: VecDeque<AgentMessage>,
    usage: TurnUsage,
    /// Messages to the agent so far: each run of user texts is one, and
    /// starts a turn (`Checkpoint::turn`).
    turns: u32,
    /// Messages added so far, the oldest ones that went included: the
    /// number of the last one (`agents.message`'s `seq`).
    seq: u64,
}

impl Transcript {
    /// Adds `message`, cut to the limits, and returns what was kept.
    pub(crate) fn push(&mut self, message: AgentMessage) -> AgentMessage {
        let message = clip(message);
        if starts_turn(self.messages.back(), &message) {
            self.turns += 1;
        }
        if let AgentMessage::TurnEnd { usage } = &message {
            add(&mut self.usage, usage);
        }
        if self.messages.len() == MAX_MESSAGES {
            self.messages.pop_front();
        }
        self.messages.push_back(message.clone());
        self.seq += 1;
        message
    }

    /// A transcript loaded from the store, with the usage summed when saved
    /// (older turns may no longer be among the messages).
    pub(crate) fn restore(messages: Vec<AgentMessage>, usage: TurnUsage) -> Self {
        let mut previous = None;
        let mut turns = 0;
        for message in &messages {
            turns += u32::from(starts_turn(previous, message));
            previous = Some(message);
        }
        Self {
            seq: messages.len() as u64,
            messages: messages.into(),
            usage,
            turns,
        }
    }

    /// The number of the latest turn, from 1; 0 before the first message.
    pub(crate) fn turns(&self) -> u32 {
        self.turns
    }

    pub(crate) fn messages(&self) -> Vec<AgentMessage> {
        self.messages.iter().cloned().collect()
    }

    /// The number of the last message added; 0 before the first.
    pub(crate) fn seq(&self) -> u64 {
        self.seq
    }

    pub(crate) fn usage(&self) -> &TurnUsage {
        &self.usage
    }
}

/// `message` with the shared terms of its tool call, from `map(name, input)`
/// when it has none: a call saved before transcripts carried them loads as
/// `other`. Mapping a call that is `other` again changes nothing.
pub(crate) fn with_call(
    mut message: AgentMessage,
    map: impl FnOnce(&str, &serde_json::Value) -> ToolCall,
) -> AgentMessage {
    if let AgentMessage::ToolUse {
        name, input, call, ..
    } = &mut message
        && *call == ToolCall::default()
    {
        *call = map(name, input);
    }
    message
}

/// A user text after anything but another one: the next message to the
/// agent, whose parts (the person's text, Ovra's notes) come in a row.
fn starts_turn(previous: Option<&AgentMessage>, message: &AgentMessage) -> bool {
    matches!(message, AgentMessage::UserText { .. })
        && !matches!(previous, Some(AgentMessage::UserText { .. }))
}

fn add(total: &mut TurnUsage, turn: &TurnUsage) {
    total.input_tokens = total.input_tokens.saturating_add(turn.input_tokens);
    total.output_tokens = total.output_tokens.saturating_add(turn.output_tokens);
    if let Some(cost) = turn.cost_micro_usd {
        total.cost_micro_usd = Some(total.cost_micro_usd.unwrap_or(0).saturating_add(cost));
    }
}

/// Cuts long texts so that one tool output cannot flood memory or the UI.
fn clip(mut message: AgentMessage) -> AgentMessage {
    match &mut message {
        AgentMessage::UserText { text, .. }
        | AgentMessage::AssistantText { text, .. }
        | AgentMessage::Error { message: text } => clip_text(text),
        AgentMessage::ToolResult { output, .. } => clip_text(output),
        AgentMessage::ToolUse { .. } | AgentMessage::TurnEnd { .. } | AgentMessage::Interrupted => {
        }
    }
    message
}

fn clip_text(text: &mut String) {
    if text.len() <= MAX_TEXT_BYTES {
        return;
    }
    let mut cut = MAX_TEXT_BYTES;
    while !text.is_char_boundary(cut) {
        cut -= 1;
    }
    let hidden = text.len() - cut;
    text.truncate(cut);
    text.push_str(&format!("\n… {hidden} more bytes not shown"));
}

#[cfg(test)]
mod tests {
    use super::*;

    fn said(text: String) -> AgentMessage {
        AgentMessage::AssistantText { text, parent: None }
    }

    #[test]
    fn long_texts_are_cut_on_a_character_boundary() {
        let mut transcript = Transcript::default();
        // Two-byte characters after one byte: the limit falls inside one.
        let output = format!("a{}", "é".repeat(MAX_TEXT_BYTES));
        let kept = transcript.push(AgentMessage::ToolResult {
            tool_use_id: "t".into(),
            output: output.clone(),
            is_error: false,
            parent: None,
        });
        let AgentMessage::ToolResult { output: cut, .. } = &kept else {
            panic!("unexpected {kept:?}");
        };
        let shown = MAX_TEXT_BYTES - 1;
        assert_eq!(
            *cut,
            format!(
                "{}\n… {} more bytes not shown",
                &output[..shown],
                output.len() - shown
            )
        );
        assert_eq!(transcript.messages(), std::slice::from_ref(&kept));
    }

    #[test]
    fn each_run_of_user_texts_is_one_turn_also_when_restored() {
        let user = |text: &str| AgentMessage::UserText {
            text: text.into(),
            from: crate::protocol::agents::MessageFrom::Person,
        };
        let mut transcript = Transcript::default();
        assert_eq!(transcript.turns(), 0);
        for message in [user("a"), user("note"), said("x".into()), user("b")] {
            transcript.push(message);
        }
        assert_eq!(transcript.turns(), 2);
        let restored = Transcript::restore(transcript.messages(), TurnUsage::default());
        assert_eq!(restored.turns(), 2);
    }

    #[test]
    fn the_oldest_messages_go_first() {
        let mut transcript = Transcript::default();
        for n in 0..=MAX_MESSAGES {
            transcript.push(said(n.to_string()));
        }
        let messages = transcript.messages();
        assert_eq!(messages.len(), MAX_MESSAGES);
        assert_eq!(messages[0], said("1".into()));
        // The numbers count the messages that went too.
        assert_eq!(transcript.seq(), MAX_MESSAGES as u64 + 1);
        assert_eq!(
            Transcript::restore(messages, TurnUsage::default()).seq(),
            MAX_MESSAGES as u64
        );
    }

    #[test]
    fn usage_adds_up_over_turns() {
        let mut transcript = Transcript::default();
        let turn = |cost| AgentMessage::TurnEnd {
            usage: TurnUsage {
                input_tokens: 10,
                output_tokens: 2,
                cost_micro_usd: cost,
            },
        };
        transcript.push(turn(None));
        assert_eq!(transcript.usage().cost_micro_usd, None);
        transcript.push(turn(Some(500)));
        transcript.push(turn(Some(250)));
        assert_eq!(
            *transcript.usage(),
            TurnUsage {
                input_tokens: 30,
                output_tokens: 6,
                cost_micro_usd: Some(750),
            }
        );
    }
}
