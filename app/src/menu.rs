//! The macOS menu bar (docs/design/patterns.md, "Menu bar"). Its items are
//! the UI's commands: picking one sends the command id to the window as
//! `ovra://menu`, and the UI runs it through its command runner, like a
//! shortcut. Ids and keys mirror the UI's command registry, in its notation
//! (`Mod+Shift+E`); tests/e2e/specs/menu-bar.spec.ts checks them against the
//! registered commands.

use tauri::menu::{
    AboutMetadata, HELP_SUBMENU_ID, Menu, MenuEvent, MenuItem, PredefinedMenuItem, Submenu,
    WINDOW_SUBMENU_ID,
};
use tauri::{AppHandle, Emitter, Runtime};

/// An item macOS implements itself.
#[derive(Clone, Copy, Debug)]
pub(crate) enum Native {
    About,
    Services,
    Hide,
    HideOthers,
    ShowAll,
    Quit,
    Undo,
    Redo,
    Cut,
    Copy,
    Paste,
    SelectAll,
    Fullscreen,
    Minimize,
    Zoom,
    BringAllToFront,
}

#[derive(Debug)]
pub(crate) enum Entry {
    /// Runs the UI command `id`. `keys` is the command's shortcut; it is left
    /// out where another item already shows it.
    Command {
        id: &'static str,
        title: &'static str,
        keys: Option<&'static str>,
    },
    Native(Native),
    Separator,
}

#[derive(Debug)]
pub(crate) struct Group {
    pub(crate) title: &'static str,
    /// Tauri's ids for the menus macOS treats as Window and Help.
    pub(crate) id: Option<&'static str>,
    pub(crate) entries: &'static [Entry],
}

const fn run(id: &'static str, title: &'static str, keys: &'static str) -> Entry {
    Entry::Command {
        id,
        title,
        keys: Some(keys),
    }
}

const fn run_unbound(id: &'static str, title: &'static str) -> Entry {
    Entry::Command {
        id,
        title,
        keys: None,
    }
}

use Entry::{Native as N, Separator as Sep};

pub(crate) const MENU_BAR: &[Group] = &[
    Group {
        title: "Ovra",
        id: None,
        entries: &[
            N(Native::About),
            Sep,
            run("settings.open", "Settings…", "Mod+,"),
            Sep,
            N(Native::Services),
            Sep,
            N(Native::Hide),
            N(Native::HideOthers),
            N(Native::ShowAll),
            Sep,
            N(Native::Quit),
        ],
    },
    Group {
        title: "File",
        id: None,
        entries: &[
            run("agents.new_session", "New Task", "Mod+N"),
            run("tree.open_folder", "Open Folder…", "Mod+O"),
            run_unbound("palette.switch_project", "Open Recent…"),
            Sep,
            run("editor.save", "Save", "Mod+S"),
            run("editor.close_tab", "Close Tab", "Mod+W"),
        ],
    },
    Group {
        title: "Edit",
        id: None,
        entries: &[
            N(Native::Undo),
            N(Native::Redo),
            Sep,
            N(Native::Cut),
            N(Native::Copy),
            N(Native::Paste),
            N(Native::SelectAll),
            Sep,
            run("editor.find", "Find", "Mod+F"),
            run("search.show", "Find in Files", "Mod+Shift+F"),
        ],
    },
    Group {
        title: "View",
        id: None,
        entries: &[
            run("palette.search", "Search or Run…", "Mod+K"),
            Sep,
            run("tree.focus", "Files", "Mod+Shift+E"),
            run("git.show", "Git", "Ctrl+Shift+G"),
            Sep,
            run("layout.toggle_side", "Show/Hide Sidebar", "Mod+0"),
            run("layout.toggle_agents", "Show/Hide Agents", "Alt+Mod+0"),
            run_unbound("layout.swap", "Swap Side Panels"),
            Sep,
            run("terminal.toggle", "Terminal", "Ctrl+`"),
            Sep,
            N(Native::Fullscreen),
        ],
    },
    Group {
        title: "Agents",
        id: None,
        entries: &[
            run_unbound("agents.new_session", "New Task"),
            Sep,
            run("approvals.focus", "Go to Approvals", "Mod+Shift+A"),
            run("inbox.toggle", "Reviews", "Mod+Shift+R"),
            run("browser.toggle", "Agent Browser", "Mod+Shift+B"),
            run("servers.toggle", "Servers", "Mod+Alt+S"),
            run("audit.toggle", "Audit Log", "Alt+Mod+L"),
        ],
    },
    Group {
        title: "Window",
        id: Some(WINDOW_SUBMENU_ID),
        entries: &[
            N(Native::Minimize),
            N(Native::Zoom),
            Sep,
            N(Native::BringAllToFront),
        ],
    },
    Group {
        title: "Help",
        id: Some(HELP_SUBMENU_ID),
        entries: &[run_unbound("palette.shortcuts", "Keyboard Shortcuts")],
    },
];

/// `keys` in Tauri's accelerator notation (`Mod` is `CmdOrCtrl`), or `None`
/// for a chord such as `Mod+K Mod+I`, which a menu cannot show.
pub(crate) fn accelerator(keys: &str) -> Option<String> {
    if keys.contains(' ') {
        return None;
    }
    let parts: Vec<&str> = keys
        .split('+')
        .map(|part| if part == "Mod" { "CmdOrCtrl" } else { part })
        .collect();
    Some(parts.join("+"))
}

/// The UI command a picked menu item stands for; `None` for native items.
pub(crate) fn command_for(menu_id: &str) -> Option<&'static str> {
    MENU_BAR
        .iter()
        .flat_map(|group| group.entries)
        .find_map(|entry| match entry {
            Entry::Command { id, .. } if *id == menu_id => Some(*id),
            _ => None,
        })
}

/// Builds the menu bar for `app`.
pub(crate) fn build<R: Runtime>(app: &AppHandle<R>) -> tauri::Result<Menu<R>> {
    let menu = Menu::new(app)?;
    for group in MENU_BAR {
        let submenu = match group.id {
            Some(id) => Submenu::with_id(app, id, group.title, true)?,
            None => Submenu::new(app, group.title, true)?,
        };
        for entry in group.entries {
            match entry {
                Entry::Command { id, title, keys } => submenu.append(&MenuItem::with_id(
                    app,
                    *id,
                    *title,
                    true,
                    keys.and_then(accelerator),
                )?)?,
                Entry::Native(native) => submenu.append(&native_item(app, *native)?)?,
                Entry::Separator => submenu.append(&PredefinedMenuItem::separator(app)?)?,
            }
        }
        menu.append(&submenu)?;
    }
    Ok(menu)
}

fn native_item<R: Runtime>(
    app: &AppHandle<R>,
    native: Native,
) -> tauri::Result<PredefinedMenuItem<R>> {
    match native {
        Native::About => {
            let info = app.package_info();
            let metadata = AboutMetadata {
                name: Some("Ovra".to_owned()),
                version: Some(info.version.to_string()),
                ..Default::default()
            };
            PredefinedMenuItem::about(app, Some("About Ovra"), Some(metadata))
        }
        Native::Services => PredefinedMenuItem::services(app, None),
        Native::Hide => PredefinedMenuItem::hide(app, None),
        Native::HideOthers => PredefinedMenuItem::hide_others(app, None),
        Native::ShowAll => PredefinedMenuItem::show_all(app, None),
        Native::Quit => PredefinedMenuItem::quit(app, None),
        Native::Undo => PredefinedMenuItem::undo(app, None),
        Native::Redo => PredefinedMenuItem::redo(app, None),
        Native::Cut => PredefinedMenuItem::cut(app, None),
        Native::Copy => PredefinedMenuItem::copy(app, None),
        Native::Paste => PredefinedMenuItem::paste(app, None),
        Native::SelectAll => PredefinedMenuItem::select_all(app, None),
        Native::Fullscreen => PredefinedMenuItem::fullscreen(app, None),
        Native::Minimize => PredefinedMenuItem::minimize(app, None),
        Native::Zoom => PredefinedMenuItem::maximize(app, None),
        Native::BringAllToFront => PredefinedMenuItem::bring_all_to_front(app, None),
    }
}

/// Hands a picked command item to the window.
pub(crate) fn forward<R: Runtime>(app: &AppHandle<R>, event: MenuEvent) {
    if let Some(id) = command_for(event.id().as_ref()) {
        let _ = app.emit("ovra://menu", id);
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashSet;
    use std::fmt::Write;

    use super::*;

    /// The menu bar as text: one line per item, with its accelerator.
    fn outline() -> String {
        let mut out = String::new();
        for group in MENU_BAR {
            let _ = writeln!(out, "{}", group.title);
            for entry in group.entries {
                let _ = match entry {
                    Entry::Command { id, title, keys } => {
                        let shown = keys.and_then(accelerator).unwrap_or_default();
                        let line = format!("  {title:<20} {shown:<20}");
                        writeln!(out, "{} -> {id}", line.trim_end())
                    }
                    Entry::Native(native) => writeln!(out, "  ({native:?})"),
                    Entry::Separator => writeln!(out, "  ---"),
                };
            }
        }
        out
    }

    #[test]
    fn the_menu_bar_follows_the_design() {
        insta::assert_snapshot!(outline());
    }

    #[test]
    fn mod_becomes_cmd_or_ctrl_and_chords_get_no_accelerator() {
        assert_eq!(accelerator("Alt+Mod+0").as_deref(), Some("Alt+CmdOrCtrl+0"));
        assert_eq!(accelerator("Mod+,").as_deref(), Some("CmdOrCtrl+,"));
        assert_eq!(accelerator("Ctrl+`").as_deref(), Some("Ctrl+`"));
        assert_eq!(accelerator("Mod+K Mod+I"), None);
    }

    #[test]
    fn no_two_items_share_an_accelerator() {
        let mut seen = HashSet::new();
        for entry in MENU_BAR.iter().flat_map(|group| group.entries) {
            if let Entry::Command {
                keys: Some(keys), ..
            } = entry
            {
                assert!(seen.insert(*keys), "{keys} is on two items");
            }
        }
    }

    #[test]
    fn only_command_items_reach_the_window() {
        assert_eq!(command_for("settings.open"), Some("settings.open"));
        assert_eq!(command_for("palette.shortcuts"), Some("palette.shortcuts"));
        assert_eq!(command_for("quit"), None);
    }
}
