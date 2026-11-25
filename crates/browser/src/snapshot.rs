//! The accessibility tree as text (`Accessibility.getFullAXTree`), one
//! line per element: `- role "name" [ref=e12]`. Refs number on across a
//! page's snapshots, so a ref of an older one is never reused and fails.

use std::collections::HashMap;
use std::fmt::Write as _;

use ovra_core::protocol::{ErrorCode, ProtocolError};
use ovra_core::services::Result;
use serde_json::Value;

/// Names are cut to this many characters.
const NAME_LIMIT: usize = 200;

/// The refs of a page's latest snapshot.
#[derive(Debug, Default)]
pub(crate) struct Refs {
    /// Ref to the element's `backendNodeId`.
    nodes: HashMap<String, i64>,
    /// The number the next snapshot's first ref gets.
    next: u64,
}

impl Refs {
    pub(crate) fn resolve(&self, target: &str) -> Result<i64> {
        self.nodes.get(target).copied().ok_or_else(|| {
            ProtocolError::new(
                ErrorCode::BadRequest,
                format!("{target} is not a ref of the latest snapshot: take a new snapshot"),
            )
        })
    }
}

/// Renders `nodes` below the element `root` (a `backendNodeId`; the whole
/// page if `None`) at most `depth` levels deep, and makes its refs the
/// page's only ones.
pub(crate) fn render(
    nodes: &[Value],
    root: Option<i64>,
    depth: Option<u32>,
    refs: &mut Refs,
) -> Result<String> {
    let by_id: HashMap<&str, &Value> = nodes
        .iter()
        .filter_map(|n| Some((n["nodeId"].as_str()?, n)))
        .collect();
    let start = match root {
        Some(backend) => nodes
            .iter()
            .find(|n| n["backendDOMNodeId"].as_i64() == Some(backend)),
        None => nodes
            .iter()
            .find(|n| n.get("parentId").is_none())
            .or(nodes.first()),
    };
    let start = start.ok_or_else(|| {
        ProtocolError::new(
            ErrorCode::NotFound,
            "that element is gone: take a new snapshot",
        )
    })?;
    refs.nodes.clear();
    let mut walk = Walk {
        by_id,
        depth: depth.map_or(usize::MAX, |d| d as usize),
        refs,
        text: String::new(),
    };
    walk.node(start, 0, "");
    Ok(walk.text)
}

struct Walk<'a, 'r> {
    by_id: HashMap<&'a str, &'a Value>,
    depth: usize,
    refs: &'r mut Refs,
    text: String,
}

fn value_str(v: &Value) -> Option<&str> {
    v["value"].as_str().filter(|s| !s.is_empty())
}

fn quoted(s: &str) -> String {
    let mut flat: String = s
        .chars()
        .map(|c| if c.is_control() { ' ' } else { c })
        .take(NAME_LIMIT)
        .collect();
    if s.chars().count() > NAME_LIMIT {
        flat.push_str("...");
    }
    format!("\"{}\"", flat.replace('\\', "\\\\").replace('"', "\\\""))
}

/// `[checked]`, `[level=2]` and the like, from the node's properties.
fn states(node: &Value) -> String {
    let mut out = String::new();
    for property in node["properties"].as_array().into_iter().flatten() {
        let name = property["name"].as_str().unwrap_or_default();
        let value = &property["value"]["value"];
        match (name, value) {
            ("checked" | "pressed", Value::String(v)) if v == "mixed" => {
                let _ = write!(out, " [{name}=mixed]");
            }
            ("checked" | "pressed" | "disabled" | "expanded" | "selected", v)
                if v == &Value::Bool(true) || v == "true" =>
            {
                let _ = write!(out, " [{name}]");
            }
            ("level", Value::Number(n)) => {
                let _ = write!(out, " [level={n}]");
            }
            _ => {}
        }
    }
    if let Some(value) = node.get("value").and_then(value_str) {
        let _ = write!(out, " [value={}]", quoted(value));
    }
    out
}

impl<'a> Walk<'a, '_> {
    fn node(&mut self, node: &'a Value, level: usize, parent_name: &str) {
        let role = node["role"]["value"].as_str().unwrap_or_default();
        let name = node.get("name").and_then(value_str).unwrap_or_default();
        let shown = !(node["ignored"].as_bool().unwrap_or(false)
            || matches!(role, "InlineTextBox" | "LineBreak")
            || (matches!(role, "generic" | "none" | "") && name.is_empty())
            || (role == "StaticText" && (name.is_empty() || name == parent_name)));
        let mut below = level;
        if shown {
            if level >= self.depth {
                return;
            }
            below = level + 1;
            let indent = "  ".repeat(level);
            if role == "StaticText" {
                let _ = writeln!(self.text, "{indent}- text {}", quoted(name));
            } else {
                let _ = write!(self.text, "{indent}- {role}");
                if !name.is_empty() {
                    let _ = write!(self.text, " {}", quoted(name));
                }
                self.text.push_str(&states(node));
                if let Some(backend) = node["backendDOMNodeId"].as_i64() {
                    self.refs.next += 1;
                    let r = format!("e{}", self.refs.next);
                    let _ = write!(self.text, " [ref={r}]");
                    self.refs.nodes.insert(r, backend);
                }
                self.text.push('\n');
            }
        }
        let name = if shown { name } else { parent_name };
        for child in node["childIds"].as_array().into_iter().flatten() {
            if let Some(child) = child.as_str().and_then(|id| self.by_id.get(id).copied()) {
                self.node(child, below, name);
            }
        }
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use serde_json::json;

    /// A small page as `Accessibility.getFullAXTree` answers it.
    pub(crate) fn tree() -> Value {
        let node = |id: &str, role: &str, name: &str, backend: i64, children: &[&str]| {
            json!({
                "nodeId": id, "ignored": false, "backendDOMNodeId": backend,
                "role": {"type": "role", "value": role},
                "name": {"type": "computedString", "value": name},
                "childIds": children,
            })
        };
        let mut heading = node("3", "heading", "Sign in", 13, &["4"]);
        heading["properties"] =
            json!([{"name": "level", "value": {"type": "integer", "value": 1}}]);
        let mut email = node("6", "textbox", "Email", 16, &[]);
        email["value"] = json!({"type": "string", "value": "a@b.c"});
        let mut save = node("7", "button", "Save \"now\"", 17, &["8"]);
        save["properties"] =
            json!([{"name": "disabled", "value": {"type": "boolean", "value": true}}]);
        let mut hidden = node("9", "generic", "", 19, &["10"]);
        hidden["ignored"] = json!(true);
        json!({"nodes": [
            node("1", "RootWebArea", "Demo", 11, &["2"]),
            node("2", "generic", "", 12, &["3", "5", "9"]),
            heading,
            node("4", "StaticText", "Sign in", 14, &[]),
            node("5", "form", "", 15, &["6", "7"]),
            email,
            save,
            node("8", "StaticText", "Save \"now\"", 18, &[]),
            hidden,
            node("10", "link", "Help", 20, &["11"]),
            node("11", "StaticText", "Need help?", 21, &[]),
        ]})
    }

    fn nodes() -> Vec<Value> {
        tree()["nodes"].as_array().cloned().unwrap_or_default()
    }

    #[test]
    fn renders_one_line_per_element_with_a_ref() {
        let mut refs = Refs::default();
        let text = render(&nodes(), None, None, &mut refs).expect("render");
        insta::assert_snapshot!(text);
        assert_eq!(refs.resolve("e6").ok(), Some(20));
    }

    #[test]
    fn refs_of_an_older_snapshot_fail_and_subtrees_keep_their_depth() {
        let mut refs = Refs::default();
        render(&nodes(), None, None, &mut refs).expect("first");
        let form = refs.resolve("e3").expect("form");
        let text = render(&nodes(), Some(form), Some(1), &mut refs).expect("subtree");
        assert_eq!(text, "- form [ref=e7]\n");
        let stale = refs.resolve("e3").err().map(|e| e.message);
        assert_eq!(
            stale.as_deref(),
            Some("e3 is not a ref of the latest snapshot: take a new snapshot")
        );
        assert!(render(&nodes(), Some(99), None, &mut refs).is_err());
    }
}
