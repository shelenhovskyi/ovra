//! `browser.*`: the agent browser the person watches, takes over and picks
//! elements in (`docs/specs/servers-and-browser.md`, ADR 0009).

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use super::agents::SessionId;

/// Assigned by the browser service when a page opens.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(transparent)]
pub struct PageId(pub String);

/// One open page of a session's browser.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PageInfo {
    pub id: PageId,
    pub session: SessionId,
    pub url: String,
    pub title: String,
}

/// A box in CSS pixels of the page's viewport, rounded.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Rect {
    pub x: i32,
    pub y: i32,
    pub width: i32,
    pub height: i32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MouseButton {
    Left,
    Middle,
    Right,
}

/// The person's input for a page they took over. `x` and `y` are CSS
/// pixels of the viewport; `modifiers` is a bit set: Alt 1, Ctrl 2,
/// Meta 4, Shift 8 (as CDP has it).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum InputEvent {
    MouseMove {
        x: i32,
        y: i32,
        #[serde(default)]
        modifiers: u8,
    },
    MouseDown {
        x: i32,
        y: i32,
        button: MouseButton,
        /// 1 for a click, 2 for the second of a double click.
        clicks: u8,
        #[serde(default)]
        modifiers: u8,
    },
    MouseUp {
        x: i32,
        y: i32,
        button: MouseButton,
        clicks: u8,
        #[serde(default)]
        modifiers: u8,
    },
    Wheel {
        x: i32,
        y: i32,
        delta_x: i32,
        delta_y: i32,
        #[serde(default)]
        modifiers: u8,
    },
    /// `key` and `code` as in `KeyboardEvent`; `text` what the key types.
    KeyDown {
        key: String,
        code: String,
        #[serde(default)]
        text: Option<String>,
        #[serde(default)]
        modifiers: u8,
    },
    KeyUp {
        key: String,
        code: String,
        #[serde(default)]
        modifiers: u8,
    },
}

/// Where an element's code may come from, read from what the page's build
/// exposes (React dev builds, `data-source` attributes). Always a guess.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct SourceGuess {
    pub component: Option<String>,
    pub file: Option<String>,
    pub line: Option<u32>,
}

/// An element the person picked, as the agent gets it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Pick {
    pub pick_id: String,
    pub url: String,
    /// A CSS selector that finds it.
    pub selector: String,
    /// A role-and-name locator, e.g. `button "Save"`.
    pub locator: String,
    pub tag: String,
    #[serde(default)]
    pub id: Option<String>,
    #[serde(default)]
    pub classes: Vec<String>,
    /// Its other attributes.
    #[serde(default)]
    pub attributes: BTreeMap<String, String>,
    /// The first 200 characters of its text.
    pub text: String,
    /// `outerHTML`, cut to 2 KiB.
    pub html: String,
    pub rect: Rect,
    /// `color`, `background-color`, `font-size`, `font-family`,
    /// `font-weight`, `display`, `position`, `margin`, `padding`, `border`.
    #[serde(default)]
    pub styles: BTreeMap<String, String>,
    #[serde(default)]
    pub source: Option<SourceGuess>,
    /// The person's note for it.
    #[serde(default)]
    pub note: String,
}

/// What a screencast frame shows, to map the canvas onto the page.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct FrameMetadata {
    /// The viewport in CSS pixels.
    pub width: u32,
    pub height: u32,
    pub scroll_x: i32,
    pub scroll_y: i32,
}

/// Why a session's agent browser cannot run, for the pane to say what helps.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BrowserRefusal {
    /// Neither Chromium nor Google Chrome was found when Ovra started.
    NoChromium,
    /// The sandbox it runs in is unavailable on this computer.
    NoSandbox,
    /// Not on this system yet (Linux only for now).
    NotSupported,
    /// The session's agent does not run in the sandbox behind its own
    /// netproxy.
    NotSandboxed,
    SessionEnded,
}

/// Whether a session's browser can run.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "state", rename_all = "snake_case")]
pub enum BrowserAvailability {
    Available,
    /// `reason` is the core's words, with the detail the code lacks.
    Refused {
        code: BrowserRefusal,
        reason: String,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "method", content = "params")]
pub enum BrowserRequest {
    /// Whether the session's browser can run, without starting it.
    /// Answers `browser.status`.
    #[serde(rename = "browser.status")]
    Status { session: SessionId },
    /// Opens a page in the session's browser, starting it if needed.
    /// Answers `browser.page`.
    #[serde(rename = "browser.open")]
    Open {
        session: SessionId,
        url: String,
        #[serde(default)]
        width: Option<u32>,
        #[serde(default)]
        height: Option<u32>,
    },
    /// Answers `ok`.
    #[serde(rename = "browser.close")]
    Close { page: PageId },
    /// Frames flow only while a panel shows the page. Answers `ok`.
    #[serde(rename = "browser.screencast")]
    Screencast { page: PageId, on: bool },
    /// The person's input for the page. Answers `ok`.
    #[serde(rename = "browser.input")]
    Input { page: PageId, event: InputEvent },
    /// The element at a point, or its ancestor `up` levels above, for the
    /// pick overlay. Answers `browser.picked`.
    #[serde(rename = "browser.pick")]
    Pick {
        page: PageId,
        x: i32,
        y: i32,
        #[serde(default)]
        up: u32,
    },
    /// The picks of a message the person sends, with their notes; the
    /// session's next message carries them. Answers `ok`.
    #[serde(rename = "browser.attach")]
    Attach { page: PageId, picks: Vec<Pick> },
    /// The person's answer to `browser.navigation_asked`, kept for the
    /// origin and session. Answers `ok`.
    #[serde(rename = "browser.answer_navigation")]
    AnswerNavigation {
        page: PageId,
        origin: String,
        allow: bool,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", content = "data")]
pub enum BrowserResponse {
    #[serde(rename = "browser.page")]
    Page(PageInfo),
    #[serde(rename = "browser.picked")]
    Picked(Box<Pick>),
    #[serde(rename = "browser.status")]
    Status(BrowserAvailability),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", content = "data")]
pub enum BrowserEvent {
    /// A screencast frame: `jpeg` is base64.
    #[serde(rename = "browser.frame")]
    Frame {
        page: PageId,
        jpeg: String,
        metadata: FrameMetadata,
    },
    /// A session's pages, after any opened, closed or navigated.
    #[serde(rename = "browser.pages")]
    Pages {
        session: SessionId,
        pages: Vec<PageInfo>,
    },
    /// The page wants an origin that is not a granted server; the person
    /// answers with `browser.answer_navigation`.
    #[serde(rename = "browser.navigation_asked")]
    NavigationAsked {
        page: PageId,
        session: SessionId,
        origin: String,
    },
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::protocol::audit::{AuditEvent, NavigationBy};
    use crate::protocol::{Event, Request, Response};

    fn page() -> PageId {
        PageId("pg1".into())
    }

    #[test]
    fn wire_format_is_stable() {
        let pick = Pick {
            pick_id: "pk1".into(),
            url: "http://localhost:5173/".into(),
            selector: "main > button.save".into(),
            locator: "button \"Save\"".into(),
            tag: "button".into(),
            id: None,
            classes: vec!["save".into()],
            attributes: BTreeMap::from([("type".into(), "submit".into())]),
            text: "Save".into(),
            html: "<button class=\"save\" type=\"submit\">Save</button>".into(),
            rect: Rect {
                x: 10,
                y: 20,
                width: 80,
                height: 24,
            },
            styles: BTreeMap::from([("color".into(), "rgb(0, 0, 0)".into())]),
            source: Some(SourceGuess {
                component: Some("SaveButton".into()),
                file: Some("src/Save.tsx".into()),
                line: Some(12),
            }),
            note: "make it blue".into(),
        };
        let info = PageInfo {
            id: page(),
            session: SessionId("s1".into()),
            url: "http://localhost:5173/".into(),
            title: "Demo".into(),
        };
        insta::assert_json_snapshot!(serde_json::json!({
            "requests": [
                Request::Browser(BrowserRequest::Input { page: page(), event: InputEvent::MouseDown {
                    x: 15, y: 25, button: MouseButton::Left, clicks: 1, modifiers: 8,
                }}),
                Request::Browser(BrowserRequest::Input { page: page(), event: InputEvent::KeyDown {
                    key: "a".into(), code: "KeyA".into(), text: Some("a".into()), modifiers: 0,
                }}),
                Request::Browser(BrowserRequest::Attach { page: page(), picks: vec![pick.clone()] }),
            ],
            "responses": [
                Response::Browser(BrowserResponse::Page(info.clone())),
                Response::Browser(BrowserResponse::Picked(Box::new(pick))),
                Response::Browser(BrowserResponse::Status(BrowserAvailability::Available)),
                Response::Browser(BrowserResponse::Status(BrowserAvailability::Refused {
                    code: BrowserRefusal::NoChromium,
                    reason: "install Chromium or Google Chrome".into(),
                })),
            ],
            "events": [
                Event::from(BrowserEvent::Frame {
                    page: page(),
                    jpeg: "/9j/4A==".into(),
                    metadata: FrameMetadata { width: 1280, height: 800, scroll_x: 0, scroll_y: 120 },
                }),
                Event::from(BrowserEvent::Pages { session: SessionId("s1".into()), pages: vec![info] }),
                Event::from(BrowserEvent::NavigationAsked {
                    page: page(),
                    session: SessionId("s1".into()),
                    origin: "https://example.com".into(),
                }),
            ],
            "audit": [
                AuditEvent::BrowserNavigation {
                    origin: "https://example.com".into(),
                    allowed: false,
                    by: NavigationBy::NoAnswer,
                },
            ],
        }));
    }

    #[test]
    fn every_request_round_trips() {
        let requests = [
            serde_json::json!({"method": "browser.status", "params": {"session": "s1"}}),
            serde_json::json!({"method": "browser.open", "params": {"session": "s1", "url": "http://localhost:5173/", "width": null, "height": 800}}),
            serde_json::json!({"method": "browser.close", "params": {"page": "pg1"}}),
            serde_json::json!({"method": "browser.screencast", "params": {"page": "pg1", "on": true}}),
            serde_json::json!({"method": "browser.input", "params": {"page": "pg1", "event": {"kind": "wheel", "x": 1, "y": 2, "delta_x": 0, "delta_y": -120, "modifiers": 0}}}),
            serde_json::json!({"method": "browser.input", "params": {"page": "pg1", "event": {"kind": "key_up", "key": "Enter", "code": "Enter", "modifiers": 0}}}),
            serde_json::json!({"method": "browser.pick", "params": {"page": "pg1", "x": 3, "y": 4, "up": 1}}),
            serde_json::json!({"method": "browser.answer_navigation", "params": {"page": "pg1", "origin": "https://example.com", "allow": false}}),
        ];
        for json in requests {
            let parsed: Request = serde_json::from_value(json.clone()).expect("request");
            assert!(matches!(parsed, Request::Browser(_)), "{json}");
            assert_eq!(serde_json::to_value(&parsed).ok(), Some(json));
        }
    }
}
