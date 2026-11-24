# Persona: Oleh, the keyboard power user

**Who:** a senior backend engineer who lives in Neovim and tmux and uses VS Code
with Vim bindings when he must. Never touches the mouse if he can help it.

**Goal in a session:** open a project, jump to any file, run any command and
move between panels using only the keyboard, fast.

**Habits and expectations**

- Mod+P for files, Mod+Shift+P for commands, Esc closes anything, Tab order makes sense.
- Expects focus to be visible and predictable after every action (open, close, error).
- Expects shortcuts to be discoverable (palette shows them) and not to clash with
  browser or OS shortcuts (Ctrl+W, Ctrl+T, Ctrl+L, Ctrl+N).
- Notices every extra keystroke and every key that silently does nothing.

**What he checks**

- Can every flow be done without the mouse, starting from page load?
- Where is focus after load, after adding a project, after closing the palette?
- Tree navigation: arrows, Home/End, typeahead, Enter, Left to parent.
- Palette: fuzzy ranking, arrow wrap, Enter, Esc, reopening keeps or clears the query.

**Fixtures:** `small`, `monorepo`. Runs on Linux, so Mod is Ctrl.
