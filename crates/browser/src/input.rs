//! The person's and the agent's input as CDP `Input.*` commands.

use ovra_core::protocol::browser::{InputEvent, MouseButton};
use serde_json::{Value, json};

fn button(button: MouseButton) -> &'static str {
    match button {
        MouseButton::Left => "left",
        MouseButton::Middle => "middle",
        MouseButton::Right => "right",
    }
}

/// Windows' virtual key code for a `KeyboardEvent.code`, which Chromium
/// needs to act on keys that type nothing (Enter, Backspace, arrows).
fn key_code(code: &str) -> Option<u32> {
    let one = |prefix: &str| {
        code.strip_prefix(prefix)
            .filter(|rest| rest.len() == 1)
            .and_then(|rest| rest.chars().next())
    };
    if let Some(c) = one("Key").filter(char::is_ascii_uppercase) {
        return Some(c as u32);
    }
    if let Some(c) = one("Digit").filter(char::is_ascii_digit) {
        return Some(c as u32);
    }
    Some(match code {
        "Backspace" => 8,
        "Tab" => 9,
        "Enter" | "NumpadEnter" => 13,
        "ShiftLeft" | "ShiftRight" => 16,
        "ControlLeft" | "ControlRight" => 17,
        "AltLeft" | "AltRight" => 18,
        "Escape" => 27,
        "Space" => 32,
        "PageUp" => 33,
        "PageDown" => 34,
        "End" => 35,
        "Home" => 36,
        "ArrowLeft" => 37,
        "ArrowUp" => 38,
        "ArrowRight" => 39,
        "ArrowDown" => 40,
        "Delete" => 46,
        "MetaLeft" | "MetaRight" => 91,
        _ => return None,
    })
}

fn key(kind: &str, key: &str, code: &str, text: Option<&str>, modifiers: u8) -> Value {
    let mut params = json!({ "type": kind, "key": key, "code": code, "modifiers": modifiers });
    if let Some(text) = text {
        params["text"] = json!(text);
    }
    if let Some(vk) = key_code(code) {
        params["windowsVirtualKeyCode"] = json!(vk);
    }
    params
}

fn mouse(kind: &str, x: i32, y: i32, modifiers: u8) -> Value {
    json!({ "type": kind, "x": x, "y": y, "modifiers": modifiers })
}

/// The method and parameters for one event of the person's.
pub(crate) fn command(event: &InputEvent) -> (&'static str, Value) {
    const MOUSE: &str = "Input.dispatchMouseEvent";
    const KEY: &str = "Input.dispatchKeyEvent";
    match event {
        InputEvent::MouseMove { x, y, modifiers } => {
            (MOUSE, mouse("mouseMoved", *x, *y, *modifiers))
        }
        InputEvent::MouseDown {
            x,
            y,
            button: b,
            clicks,
            modifiers,
        }
        | InputEvent::MouseUp {
            x,
            y,
            button: b,
            clicks,
            modifiers,
        } => {
            let kind = if matches!(event, InputEvent::MouseDown { .. }) {
                "mousePressed"
            } else {
                "mouseReleased"
            };
            let mut params = mouse(kind, *x, *y, *modifiers);
            params["button"] = json!(button(*b));
            params["clickCount"] = json!(clicks);
            (MOUSE, params)
        }
        InputEvent::Wheel {
            x,
            y,
            delta_x,
            delta_y,
            modifiers,
        } => {
            let mut params = mouse("mouseWheel", *x, *y, *modifiers);
            params["deltaX"] = json!(delta_x);
            params["deltaY"] = json!(delta_y);
            (MOUSE, params)
        }
        InputEvent::KeyDown {
            key: k,
            code,
            text,
            modifiers,
        } => {
            let kind = if text.is_some() {
                "keyDown"
            } else {
                "rawKeyDown"
            };
            (KEY, key(kind, k, code, text.as_deref(), *modifiers))
        }
        InputEvent::KeyUp {
            key: k,
            code,
            modifiers,
        } => (KEY, key("keyUp", k, code, None, *modifiers)),
    }
}

/// A click at `x`, `y`: move, press, release, and again for a double one.
pub(crate) fn click(
    x: i32,
    y: i32,
    b: MouseButton,
    double: bool,
    modifiers: u8,
) -> Vec<InputEvent> {
    let mut events = vec![InputEvent::MouseMove { x, y, modifiers }];
    for clicks in 1..=(if double { 2 } else { 1 }) {
        events.push(InputEvent::MouseDown {
            x,
            y,
            button: b,
            clicks,
            modifiers,
        });
        events.push(InputEvent::MouseUp {
            x,
            y,
            button: b,
            clicks,
            modifiers,
        });
    }
    events
}

/// A key as an agent names it (`Enter`, `a`, `Control+a`): down and up.
pub(crate) fn press(name: &str) -> Vec<InputEvent> {
    let mut parts: Vec<&str> = name.split('+').collect();
    // `+` itself, and `Shift++`.
    let key = match parts.pop() {
        Some("") if !parts.is_empty() => {
            parts.pop();
            "+"
        }
        Some(key) => key,
        None => name,
    };
    let modifiers = parts.iter().fold(0u8, |bits, part| {
        bits | match *part {
            "Alt" => 1,
            "Control" | "Ctrl" => 2,
            "Meta" | "Command" => 4,
            "Shift" => 8,
            _ => 0,
        }
    });
    let mut chars = key.chars();
    let (code, text) = match (chars.next(), chars.next()) {
        (Some(c), None) if c.is_ascii_alphabetic() => {
            (format!("Key{}", c.to_ascii_uppercase()), Some(key))
        }
        (Some(c), None) if c.is_ascii_digit() => (format!("Digit{c}"), Some(key)),
        (Some(' '), None) => ("Space".to_owned(), Some(" ")),
        (Some(_), None) => (String::new(), Some(key)),
        _ if key == "Enter" => (key.to_owned(), Some("\r")),
        _ => (key.to_owned(), None),
    };
    // A shortcut types nothing.
    let text = text.filter(|_| modifiers & 0b0111 == 0).map(str::to_owned);
    vec![
        InputEvent::KeyDown {
            key: key.to_owned(),
            code: code.clone(),
            text,
            modifiers,
        },
        InputEvent::KeyUp {
            key: key.to_owned(),
            code,
            modifiers,
        },
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn events_become_cdp_commands() {
        let mut events = click(5, 6, MouseButton::Right, true, 8);
        events.extend(press("Enter"));
        events.extend(press("Control+a"));
        events.extend(press("Shift++"));
        events.push(InputEvent::Wheel {
            x: 1,
            y: 2,
            delta_x: 0,
            delta_y: -120,
            modifiers: 0,
        });
        let commands: Vec<String> = events
            .iter()
            .map(|e| {
                let (method, params) = command(e);
                format!("{method} {params}")
            })
            .collect();
        insta::assert_debug_snapshot!(commands);
    }
}
