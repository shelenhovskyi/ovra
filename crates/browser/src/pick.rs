//! What the person picks: the [`Pick`] payload of an element. The page's
//! own scripts cannot change what is read, because it is read in an
//! isolated world of Ovra's, with the browser's own `getComputedStyle`
//! and `getBoundingClientRect`, not ones the page may have replaced.

use std::collections::BTreeMap;

use ovra_core::protocol::browser::{Pick, Rect, SourceGuess};
use serde_json::Value;

/// The element itself (a text node's parent), or its ancestor `up` levels
/// above.
pub(crate) const ANCESTOR: &str = "function (up) {
  let e = this.nodeType === 1 ? this : this.parentElement;
  for (let i = 0; i < up && e && e.parentElement; i++) e = e.parentElement;
  return e;
}";

/// What the payload needs, read in one call.
pub(crate) const READ: &str = "function () {
  const e = this;
  const parts = [];
  for (let n = e; n && n.nodeType === 1; n = n.parentElement) {
    if (n.id && document.querySelectorAll('#' + CSS.escape(n.id)).length === 1) {
      parts.unshift('#' + CSS.escape(n.id));
      break;
    }
    let part = n.localName;
    const same = n.parentElement
      ? Array.from(n.parentElement.children).filter((c) => c.localName === n.localName)
      : [];
    if (same.length > 1) part += ':nth-of-type(' + (same.indexOf(n) + 1) + ')';
    parts.unshift(part);
  }
  const attributes = {};
  for (const a of e.attributes) attributes[a.name] = a.value;
  const computed = getComputedStyle(e);
  const styles = {};
  for (const k of STYLES) styles[k] = computed.getPropertyValue(k);
  const r = e.getBoundingClientRect();
  return {
    selector: parts.join(' > '),
    tag: e.localName,
    attributes,
    text: (e.innerText ?? e.textContent ?? '').slice(0, 1000),
    html: e.outerHTML.slice(0, 4096),
    rect: { x: r.x, y: r.y, width: r.width, height: r.height },
    styles,
  };
}";

/// The computed styles a pick carries.
pub(crate) const STYLES: [&str; 10] = [
    "color",
    "background-color",
    "font-size",
    "font-family",
    "font-weight",
    "display",
    "position",
    "margin",
    "padding",
    "border",
];

const TEXT_LIMIT: usize = 200;
const HTML_LIMIT: usize = 2048;

/// [`READ`] with the style names filled in.
pub(crate) fn read_function() -> String {
    let names: Vec<String> = STYLES.iter().map(|s| format!("'{s}'")).collect();
    READ.replace("STYLES", &format!("[{}]", names.join(", ")))
}

/// `role "name"` from the element's accessibility node, else its tag.
pub(crate) fn locator(ax: &Value, tag: &str) -> String {
    let node = &ax["nodes"][0];
    let role = node["role"]["value"].as_str().unwrap_or_default();
    let name = node["name"]["value"].as_str().unwrap_or_default().trim();
    match (role, name) {
        ("" | "generic" | "none", _) => tag.to_owned(),
        (role, "") => role.to_owned(),
        (role, name) => format!("{role} \"{}\"", name.replace('"', "\\\"")),
    }
}

/// At most `limit` bytes of `s`, cut at a character.
fn cut(s: &str, limit: usize) -> String {
    let mut end = s.len().min(limit);
    while !s.is_char_boundary(end) {
        end -= 1;
    }
    s[..end].to_owned()
}

/// `src/Save.tsx:12` (or `:12:5`) in `data-source`, the component in
/// `data-component`. Nothing when the page says neither.
fn source(attributes: &BTreeMap<String, String>) -> Option<SourceGuess> {
    let component = attributes.get("data-component").cloned();
    let (file, line) = match attributes.get("data-source") {
        Some(source) => {
            // A line, or a line and a column, after the file.
            let (mut file, mut numbers) = (source.as_str(), Vec::new());
            while numbers.len() < 2 {
                match file.rsplit_once(':') {
                    Some((head, n)) if !head.is_empty() && n.parse::<u32>().is_ok() => {
                        numbers.push(n.parse::<u32>().ok());
                        file = head;
                    }
                    _ => break,
                }
            }
            (Some(file.to_owned()), numbers.last().copied().flatten())
        }
        None => (None, None),
    };
    (component.is_some() || file.is_some()).then_some(SourceGuess {
        component,
        file,
        line,
    })
}

/// The payload from what [`READ`] answered.
pub(crate) fn build(read: &Value, pick_id: String, url: String, locator: String) -> Pick {
    let text = |key: &str| read[key].as_str().unwrap_or_default().to_owned();
    let map = |key: &str| -> BTreeMap<String, String> {
        read[key]
            .as_object()
            .into_iter()
            .flatten()
            .filter_map(|(k, v)| Some((k.clone(), v.as_str()?.to_owned())))
            .collect()
    };
    let mut attributes = map("attributes");
    let id = attributes.remove("id");
    let classes = attributes
        .remove("class")
        .map(|c| c.split_whitespace().map(str::to_owned).collect())
        .unwrap_or_default();
    let number = |key: &str| read["rect"][key].as_f64().unwrap_or_default().round() as i32;
    let words: Vec<&str> = read["text"]
        .as_str()
        .unwrap_or_default()
        .split_whitespace()
        .collect();
    Pick {
        pick_id,
        url,
        selector: text("selector"),
        locator,
        tag: text("tag"),
        id,
        classes,
        source: source(&attributes),
        attributes,
        text: words.join(" ").chars().take(TEXT_LIMIT).collect(),
        html: cut(read["html"].as_str().unwrap_or_default(), HTML_LIMIT),
        rect: Rect {
            x: number("x"),
            y: number("y"),
            width: number("width"),
            height: number("height"),
        },
        styles: map("styles"),
        note: String::new(),
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use serde_json::json;

    /// What [`READ`] answers for a save button.
    pub(crate) fn read() -> Value {
        json!({
            "selector": "main > form > button:nth-of-type(2)",
            "tag": "button",
            "attributes": {
                "id": "save", "class": "btn  primary", "type": "submit",
                "data-source": "src/components/Save.tsx:12:5", "data-component": "SaveButton",
            },
            "text": "  Save\n   changes  ",
            "html": format!("<button id=\"save\">{}</button>", "é".repeat(1100)),
            "rect": {"x": 10.4, "y": 20.6, "width": 80.0, "height": 23.5},
            "styles": {"color": "rgb(255, 255, 255)", "display": "inline-block"},
        })
    }

    #[test]
    fn builds_the_payload_the_agent_gets() {
        let ax =
            json!({"nodes": [{"role": {"value": "button"}, "name": {"value": "Save changes"}}]});
        let pick = build(
            &read(),
            "pk1".into(),
            "http://localhost:5173/".into(),
            locator(&ax, "button"),
        );
        assert!(pick.html.len() <= HTML_LIMIT && pick.html.ends_with('é'));
        insta::assert_json_snapshot!(Pick {
            html: format!("{}... ({} bytes)", &pick.html[..18], pick.html.len()),
            ..pick
        });
    }

    #[test]
    fn locators_and_sources_fall_back() {
        assert_eq!(locator(&json!({"nodes": []}), "div"), "div");
        let generic = json!({"nodes": [{"role": {"value": "generic"}, "name": {"value": "x"}}]});
        assert_eq!(locator(&generic, "span"), "span");
        let attrs = |source: &str| BTreeMap::from([("data-source".to_owned(), source.to_owned())]);
        let guess = |source: &str| source_of(&attrs(source));
        assert_eq!(guess("src/a.ts:7"), (Some("src/a.ts".into()), Some(7)));
        assert_eq!(guess("src/a.ts"), (Some("src/a.ts".into()), None));
        assert_eq!(guess("C:/x/a.ts:3:9"), (Some("C:/x/a.ts".into()), Some(3)));
        assert_eq!(source(&BTreeMap::new()), None);
        assert!(read_function().contains("['color', 'background-color'"));
    }

    fn source_of(attrs: &BTreeMap<String, String>) -> (Option<String>, Option<u32>) {
        source(attrs).map_or((None, None), |s| (s.file, s.line))
    }
}
