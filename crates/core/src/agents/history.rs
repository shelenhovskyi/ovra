//! `agents.history`: past sessions of every project, searched by what their
//! title, project and conversation say (`docs/specs/task-history.md`). It
//! reads the transcripts the registry keeps in memory; there is no index.

use std::collections::BTreeMap;

use crate::protocol::agents::{AgentMessage, HISTORY_LIMIT, HistoryEntry, SessionInfo, Snippet};
use crate::protocol::projects::ProjectId;

/// Longest snippet, in characters, without the ellipses.
const SNIPPET_CHARS: usize = 160;
/// Characters a snippet keeps before the match when it has to be cut.
const BEFORE_CHARS: usize = 50;

/// What the history search needs of one session.
pub(super) struct Candidate<'a> {
    pub(super) info: SessionInfo,
    pub(super) messages: Vec<&'a AgentMessage>,
}

/// The sessions among `candidates` of a project named in `names` (and of
/// `project`, if given) with every word of `query`, newest activity first.
pub(super) fn search(
    candidates: Vec<Candidate<'_>>,
    query: &str,
    project: Option<&ProjectId>,
    names: &BTreeMap<ProjectId, String>,
) -> Vec<HistoryEntry> {
    let words: Vec<String> = query.split_whitespace().map(str::to_lowercase).collect();
    let mut found: Vec<HistoryEntry> = candidates
        .into_iter()
        .filter(|c| project.is_none_or(|p| &c.info.project == p))
        .filter_map(|c| {
            let name = names.get(&c.info.project)?;
            matches(&c, name, &words).map(|snippet| HistoryEntry {
                session: c.info,
                snippet,
            })
        })
        .collect();
    found
        .sort_by_key(|e| std::cmp::Reverse((e.session.active_ms.unwrap_or(0), number(&e.session))));
    found.truncate(HISTORY_LIMIT);
    found
}

/// `None` when some word is nowhere; else the snippet of the first message
/// that has a word the title and project name lack.
fn matches(c: &Candidate<'_>, name: &str, words: &[String]) -> Option<Option<Snippet>> {
    let heading = format!("{}\n{name}", c.info.title).to_lowercase();
    let texts: Vec<&str> = c.messages.iter().filter_map(|m| text(m)).collect();
    let lowered: Vec<String> = texts.iter().map(|t| t.to_lowercase()).collect();
    let mut snippet = None;
    for word in words {
        if heading.contains(word.as_str()) {
            continue;
        }
        let at = lowered.iter().position(|t| t.contains(word.as_str()))?;
        if snippet.is_none() {
            snippet = cut(texts[at], word);
        }
    }
    Some(snippet)
}

/// What the person, a coordinator or the agent wrote; tool calls and their
/// output are left out.
fn text(message: &AgentMessage) -> Option<&str> {
    match message {
        AgentMessage::UserText { text, .. } | AgentMessage::AssistantText { text, .. } => {
            Some(text)
        }
        _ => None,
    }
}

/// The session's number (`s12` is 12), to order sessions with the same time.
fn number(info: &SessionInfo) -> u64 {
    info.id
        .0
        .strip_prefix('s')
        .and_then(|n| n.parse().ok())
        .unwrap_or(0)
}

/// The line of `text` with the first case-insensitive match of `word`
/// (lowercase), cut to [`SNIPPET_CHARS`] around it.
fn cut(text: &str, word: &str) -> Option<Snippet> {
    let chars: Vec<char> = text.chars().collect();
    let wanted: Vec<char> = word.chars().collect();
    let start = (0..chars.len()).find(|&i| starts_with(&chars[i..], &wanted))?;
    let end = start + wanted.len();
    let line_start = chars[..start]
        .iter()
        .rposition(|&c| c == '\n')
        .map_or(0, |i| i + 1);
    let line_end = chars[end..]
        .iter()
        .position(|&c| c == '\n')
        .map_or(chars.len(), |i| end + i);
    let from = if line_end - line_start <= SNIPPET_CHARS {
        line_start
    } else {
        start.saturating_sub(BEFORE_CHARS).max(line_start)
    };
    let to = line_end.min(from + SNIPPET_CHARS).max(end);
    let mut out = String::new();
    if from > line_start {
        out.push('…');
    }
    let lead: String = chars[from..start].iter().collect();
    let hit: String = chars[start..end].iter().collect();
    let rest: String = chars[end..to].iter().collect();
    let lead = format!("{out}{}", lead.trim_start());
    let mark = utf16(&lead);
    out = format!("{lead}{hit}{}", rest.trim_end());
    if to < line_end {
        out.push('…');
    }
    Some(Snippet {
        text: out,
        from: mark,
        to: mark + utf16(&hit),
    })
}

/// Whether `chars` begins with `word` (lowercase), ignoring case one
/// character at a time so that offsets stay the text's own.
fn starts_with(chars: &[char], word: &[char]) -> bool {
    chars.len() >= word.len()
        && chars.iter().zip(word).all(|(c, w)| {
            let mut lower = c.to_lowercase();
            lower.next() == Some(*w) && lower.next().is_none()
        })
}

fn utf16(s: &str) -> u32 {
    u32::try_from(s.encode_utf16().count()).unwrap_or(u32::MAX)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn snippet(text: &str, word: &str) -> String {
        let s = cut(text, word).expect("found");
        let units: Vec<u16> = s.text.encode_utf16().collect();
        let hit = String::from_utf16_lossy(&units[s.from as usize..s.to as usize]);
        format!("{} [{hit}]", s.text)
    }

    #[test]
    fn snippets_show_the_line_of_the_match() {
        let long = format!("{} needle {}", "a".repeat(200), "b".repeat(200));
        let shown = [
            snippet("First line\nFixed the Login bug\nlast", "login"),
            snippet("Kalimera, Κόσμε", "κόσμε"),
            snippet("🙂 emoji then Word", "word"),
            snippet(&long, "needle"),
        ];
        insta::assert_snapshot!(shown.join("\n"), @r"
        Fixed the Login bug [Login]
        Kalimera, Κόσμε [Κόσμε]
        🙂 emoji then Word [Word]
        …aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa needle bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb… [needle]
        ");
    }
}
