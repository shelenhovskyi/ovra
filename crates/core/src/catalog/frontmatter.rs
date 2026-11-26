//! The `---` header of a skill, subagent or command file: the small part of
//! YAML those headers use (`key: value`, quoted values, folded `>` and `|`
//! blocks, `- item` lists and `[a, b]`), read leniently. Anything else
//! stays as written.

/// The header's fields in order, and the text after it.
pub(super) fn split(text: &str) -> (Vec<(String, String)>, &str) {
    let text = text.strip_prefix('\u{feff}').unwrap_or(text);
    let Some(rest) = text
        .strip_prefix("---\n")
        .or_else(|| text.strip_prefix("---\r\n"))
    else {
        return (Vec::new(), text);
    };
    let mut header = Vec::new();
    let mut at = 0;
    let mut body = None;
    for line in rest.split_inclusive('\n') {
        at += line.len();
        if line.trim_end() == "---" {
            body = Some(&rest[at..]);
            break;
        }
        header.push(line.trim_end_matches(['\n', '\r']));
    }
    match body {
        Some(body) => (fields(&header), body),
        // No closing line: not a header.
        None => (Vec::new(), text),
    }
}

fn fields(lines: &[&str]) -> Vec<(String, String)> {
    let mut out: Vec<(String, String)> = Vec::new();
    let mut i = 0;
    while i < lines.len() {
        let line = lines[i];
        i += 1;
        if line.starts_with([' ', '\t', '#', '-']) || line.trim().is_empty() {
            continue;
        }
        let Some((key, value)) = line.split_once(':') else {
            continue;
        };
        let value = value.trim();
        // The indented lines under the key, if it has any.
        let mut nested = Vec::new();
        while i < lines.len() && (lines[i].starts_with([' ', '\t']) || lines[i].trim().is_empty()) {
            nested.push(lines[i].trim());
            i += 1;
        }
        while nested.last().is_some_and(|l| l.is_empty()) {
            nested.pop();
        }
        let value = if value.is_empty() || value.starts_with(['>', '|']) {
            let literal = value.starts_with('|');
            let items: Vec<&str> = nested.iter().filter_map(|l| l.strip_prefix("- ")).collect();
            if !items.is_empty() && value.is_empty() {
                items
                    .iter()
                    .map(|v| unquote(v.trim()))
                    .collect::<Vec<_>>()
                    .join(", ")
            } else if literal {
                nested.join("\n")
            } else {
                nested.join(" ").trim().to_owned()
            }
        } else if let Some(list) = value.strip_prefix('[').and_then(|v| v.strip_suffix(']')) {
            list.split(',')
                .map(|v| unquote(v.trim()))
                .filter(|v| !v.is_empty())
                .collect::<Vec<_>>()
                .join(", ")
        } else {
            let mut value = unquote(value);
            // A plain value may go on over indented lines.
            for more in nested.iter().filter(|l| !l.is_empty()) {
                value.push(' ');
                value.push_str(more);
            }
            value
        };
        out.push((key.trim().to_owned(), value));
    }
    out
}

fn unquote(value: &str) -> String {
    for quote in ['"', '\''] {
        if let Some(inner) = value
            .strip_prefix(quote)
            .and_then(|v| v.strip_suffix(quote))
        {
            return inner.to_owned();
        }
    }
    value.to_owned()
}

/// The first line of prose: not a heading, comment, rule or blank line.
pub(super) fn first_line(body: &str) -> Option<String> {
    let mut comment = false;
    for line in body.lines() {
        let line = line.trim();
        if comment {
            comment = !line.contains("-->");
            continue;
        }
        if line.starts_with("<!--") {
            comment = !line.contains("-->");
            continue;
        }
        if line.is_empty()
            || line.starts_with(['#', '>', '|'])
            || line.chars().all(|c| "-=*_`".contains(c))
        {
            continue;
        }
        return Some(shorten(line.trim_start_matches(['-', '*', ' '])));
    }
    None
}

/// At most 200 characters, on a word boundary when there is one.
pub(super) fn shorten(text: &str) -> String {
    const MAX: usize = 200;
    if text.chars().count() <= MAX {
        return text.to_owned();
    }
    let cut: String = text.chars().take(MAX).collect();
    let cut = cut.rsplit_once(' ').map_or(cut.as_str(), |(head, _)| head);
    format!("{}…", cut.trim_end())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_plain_quoted_folded_and_listed_values() {
        let text = "---\nname: review\ndescription: >\n  Reviews a diff\n  for bugs.\ntools:\n  - Read\n  - \"Grep\"\nmodel: 'sonnet'\nallowed-tools: [Bash, Read]\nnote: one\n  two\n---\n\n# Body\n";
        let (fields, body) = split(text);
        let pairs: Vec<(&str, &str)> = fields
            .iter()
            .map(|(k, v)| (k.as_str(), v.as_str()))
            .collect();
        assert_eq!(
            pairs,
            [
                ("name", "review"),
                ("description", "Reviews a diff for bugs."),
                ("tools", "Read, Grep"),
                ("model", "sonnet"),
                ("allowed-tools", "Bash, Read"),
                ("note", "one two"),
            ]
        );
        assert_eq!(body, "\n# Body\n");
    }

    #[test]
    fn a_file_without_a_closed_header_is_all_body() {
        assert_eq!(split("# Title\ntext").0, []);
        let (fields, body) = split("---\nname: x\nno end");
        assert!(fields.is_empty());
        assert_eq!(body, "---\nname: x\nno end");
        let (fields, body) = split("\u{feff}---\r\nname: x\r\n---\r\nhi");
        assert_eq!(fields, [("name".to_owned(), "x".to_owned())]);
        assert_eq!(body, "hi");
    }

    #[test]
    fn the_first_line_skips_headings_comments_and_rules() {
        let body = "# Rules\n<!-- hidden\nstill -->\n---\n\n- Use English everywhere.\nMore.";
        assert_eq!(first_line(body).as_deref(), Some("Use English everywhere."));
        assert_eq!(first_line("# Only a title\n"), None);
        let long = "word ".repeat(60);
        let short = first_line(&long).unwrap_or_default();
        assert!(short.ends_with("word…") && short.chars().count() <= 201);
    }
}
