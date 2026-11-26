# Context for agent tasks (@)

Status: proposed 2026-11-25. The owner asked for @-context only; limits and
cost display are out of scope.

People point an agent at what they are looking at: a file, a folder, the
selected lines, a problem the language server reports, terminal output or
the agent's browser page. Today they type paths and paste text. This spec
adds one menu and compact chips, and nothing more.

## Behaviour

- **Typing `@`** in the task box ("Give Claude a task") or the reply box
  opens one popup menu above the field. Typing after `@` filters it. The menu
  shows, in this order, only what applies now:
  - **Selection**: the selected lines of the focused editor (`path:12-18`).
  - **Problems**: each language-server problem of the open file
    (`path:12 message`).
  - **Terminal**: the active terminal's selection, else its last lines.
  - **Browser**: a screenshot of the session's browser page (reply box only;
    the browser belongs to a session).
  - **Files and folders** of the project, fuzzy matched as in the palette.
- Up/Down move, Enter or Tab adds, Escape closes and keeps the typed `@`.
  Choosing an item removes the typed `@query` and adds a **chip** under the
  field: an icon and a short name (`lib.rs`, `src/`, `lib.rs:12-18`,
  `Terminal`, `Screenshot`). The full path is its tooltip. × removes it; so
  does Backspace in an empty field (last chip).
- **Ask agent** (editor menu, the problem's hover card, a Problems row,
  the terminal menu with a selection) adds the selection, the problem or the terminal
  selection as a chip to the Agents panel's open box (the reply box of the
  open task, else the new-task box), shows the panel and focuses the field.
  Ovra has no test runner view yet: the terminal (where a failed test
  prints) is where that lives today.
- Chips go with the message and are cleared after it is sent. The
  transcript shows the person's text; the context goes to the agent as
  Ovra's message after it, like browser picks.

## Safety

The core checks every item, when it is added (`context.check`) and again
when it is sent (`agents.start`, `agents.send`, `orchestration.start`):

- Paths go through `RelPath::parse`: nothing outside the project.
- Files and folders are **referenced, never read** by Ovra: the agent reads
  them with its own tools, so the sandbox and approvals apply as before.
- An item is **sensitive** when its path is protected
  (`ovra_core::protected`), or looks like a secret store (`.env*`, `*.pem`,
  `*.key`, `id_rsa*`, `.npmrc`, `.netrc`, `credentials*`, `secrets/`), or its
  text holds a token or a secret header (the audit log's detector) or one of
  the project's secret values. A sensitive chip is amber with the reason
  ("May hold a secret") and is not sent until the person chooses **Include**
  on it; the core refuses a sensitive item without that confirmation.
- Text items (selection, problem, terminal) are cut to 16 KiB each and 48
  KiB together, names and titles included. Everything the person, the
  project or a program wrote reaches the agent either escaped (paths,
  titles, a problem's source) or inside `<untrusted>` (selected lines, a
  problem's message, terminal output), so nothing can close Ovra's note.
  Paths with control characters are refused.
- When the project's secret values cannot be read (a locked keychain),
  every text item counts as sensitive.
- The screenshot item names a page of the task the reply goes to; a new
  task has no browser, so it is refused there. The agent takes the image
  with `browser_screenshot` (Ovra's messages to the agent are text).

## Protocol

`protocol/context.rs`: `ContextItem` (`file`, `folder`, `selection`,
`problem`, `terminal`, `page`, each with `confirmed`), `context.check
{ project, session?, items }` answering `context.checked { notes }` (one `null` or a
warning per item). `agents.start`, `agents.send` and `orchestration.start`
take an optional `context: ContextItem[]`.

## Out of scope

Limits and cost display; reading file contents into the message; images in
messages; a test view; saved context sets.
