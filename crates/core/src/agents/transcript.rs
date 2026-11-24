//! A session's transcript, kept within memory limits. The agent keeps its own
//! full history (Claude Code: its session file), so the core keeps only what
//! the UI shows.

use std::collections::VecDeque;

use crate::protocol::agents::{AgentMessage, TurnUsage};

/// Messages kept per session; the oldest go first.
pub(crate) const MAX_MESSAGES: usize = 5_000;
/// Longest text kept in one message, and the longest prompt accepted.
pub(crate) const MAX_TEXT_BYTES: usize = 64 * 1024;

#[derive(Debug, Default)]
pub(crate) struct Transcript {
    messages: VecDeque<AgentMessage>,
    usage: TurnUsage,
}

impl Transcript {
    /// Adds `message`, cut to the limits, and returns what was kept.
    pub(crate) fn push(&mut self, message: AgentMessage) -> AgentMessage {
        let message = clip(message);
        if let AgentMessage::TurnEnd { usage } = &message {
            add(&mut self.usage, usage);
        }
        if self.messages.len() == MAX_MESSAGES {
            self.messages.pop_front();
        }
        self.messages.push_back(message.clone());
        message
    }

    pub(crate) fn messages(&self) -> Vec<AgentMessage> {
        self.messages.iter().cloned().collect()
    }

    pub(crate) fn usage(&self) -> &TurnUsage {
        &self.usage
    }
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
        AgentMessage::UserText { text }
        | AgentMessage::AssistantText { text, .. }
        | AgentMessage::Error { message: text } => clip_text(text),
        AgentMessage::ToolResult { output, .. } => clip_text(output),
        AgentMessage::ToolUse { .. } | AgentMessage::TurnEnd { .. } => {}
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
    fn the_oldest_messages_go_first() {
        let mut transcript = Transcript::default();
        for n in 0..=MAX_MESSAGES {
            transcript.push(said(n.to_string()));
        }
        let messages = transcript.messages();
        assert_eq!(messages.len(), MAX_MESSAGES);
        assert_eq!(messages[0], said("1".into()));
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
