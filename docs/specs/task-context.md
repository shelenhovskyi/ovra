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
  - **Files and folders** of the project, fuzzy matched as in the palette;
    with nothing typed yet, recent files and then the project's top level.
    What the box holds already is left out. When nothing fits, the menu says
    "No matches" and Enter sends the message as typed. A full-width `＠`
    opens it too.
- Up/Down move, Enter or Tab adds, Escape closes and keeps the typed `@`
  (typing more of it opens the menu again);
  the menu follows the caret and closes when the field loses focus.
  Ctrl/Cmd+Enter never chooses an item.
  Choosing an item puts its short name in place of the typed `@query`, so
  the sentence still reads, and adds a **chip** under the
  field: an icon and a short name (`lib.rs`, `src/`, `lib.rs:12-18`,
  the terminal's title, `Screenshot`). The tooltip has the full name and, for
  text items, the first lines that will be sent. × removes it; so does
  Backspace in an empty field (last chip; a held Backspace takes only one),
  and adding or removing one is announced to screen readers. Focus returns to the field.
- **Ask agent** (editor menu, the problem's hover card, a Problems row,
  the terminal menu with a selection) adds the selection, the problem or the terminal
  selection as a chip to the Agents panel's open box (the reply box of the
  open task, else the new-task box), shows the panel and focuses the field.
  Ovra has no test runner view yet: the terminal (where a failed test
  prints) is where that lives today.
- Chips go with the message and are cleared after it is sent. The
  transcript shows the person's text and a folded "Context sent" line with
  the items; the context goes to the agent as Ovra's message after it, like
  browser picks.

## Dropped and pasted files

Added 2026-11-26: every box that talks to an agent takes files the way
the file tree does.

- **Dropping** files on the task box or the reply box adds them: rows
  from the file tree, and files from the system's file manager (the
  desktop app reports those with their paths, as for the tree; a browser in
  headless mode gives the page the files themselves). The box shows an
  accent outline and "Drop to add to the message" while files are over it.
- **Pasting** (Cmd/Ctrl+V) a screenshot or a copied file adds it too; a
  paste that also carries other text pastes the text.
- A file or folder **inside the project** becomes a chip as if picked with
  @: named, not read, with the usual checks (a dropped `.env` is amber).
- A file **from elsewhere**, and anything pasted, is copied into the
  project's attachments folder (`.ovra-attachments/`, which holds a
  `.gitignore` of `*`, so git never shows it), because the agent reads only
  the project. The drop or the paste is the person's explicit action, as a
  drop on the tree is. A copy in the project can be read by every agent, so
  **before copying**, names that look like secret stores by the chip check
  (the file and its folder: `.ssh/id_ed25519`, `credentials`, `.env`) wait
  for "Copy anyway" or Cancel. A pasted file is 10 MiB at most (its base64
  must fit one WebSocket frame); a desktop drop is copied like a drop on the
  tree. A name that is taken gets `-2`, `-3` (`copy` for a desktop drop).
  The core refuses to write when `.ovra-attachments` is not a folder (an
  agent could make it a link into the code).
- **Focus**: the task box there takes @ context, drops and pastes like
  the panel's task box (it shares that box's chips). A question's answer is text to the board, and the asking
  worker's worktree does not get the attachments folder, so only project
  files can be dropped there; they go in as paths, and one that looks like a
  secret store is refused ("type the path if you mean it"). Pasted files
  and files from elsewhere get a note pointing to the reply box.
- `context.attach { project, name, data }` (base64) saves one file under a
  safe name (letters, digits, `.`, `_`, `-`, spaces; never only dots),
  marks it as the person's change and answers `context.attached { path }`;
  `context.attachments { project }` makes the folder for the desktop app's
  copy of a drop (`ovra_import_drop`, clash "keep both"). The phone
  companion cannot call either (`context.*` is not a phone method).
- Not yet: copies stay after their chip is removed (they are ignored and
  can be deleted from the tree); workers in their own worktrees do not get
  the copies; Windows device names (`NUL`, `COM1`) are not mapped yet.

## GitHub issues and failed checks

Added 2026-11-26 on the owner's request ("a task from a GitHub issue or a
failed CI check, tucked away so it takes no room"). No panel or button of
its own: it lives in the @ menu.

- The plain @ menu's last row, **GitHub issue or failed check** (`@#`),
  shows with nothing typed after `@` or a word that names it (`gi`,
  `issue`, `check`, `ci`). Choosing it, or typing `@#`, switches the menu
  to GitHub's rows; only then does Ovra ask the person's `gh` (ADR 0010).
  The rows are read again each time the menu opens.
- **Failed checks** come first: the failed GitHub Actions checks of the
  open pull request from the main checkout's branch, found by the branch's
  upstream (`pr.branch_checks`). Then the **open issues** of `origin`'s
  repository, newest first, 30 at most (`pr.issues`). Typing after `@#`
  filters them; a number no listed issue has gets a "Read issue #N" row.
- Choosing a row reads the issue's text (`pr.issue`: the body, then the
  latest 10 comments under their authors' logins) or the failed log's end
  (`pr.check_log`), each cut to 15 KiB, and adds a chip: `#12` with the
  title and first lines in its tooltip, or the check's name. The name
  takes the `@#query`'s place, so "Fix @#12" reads "Fix #12".
- When `gh` is missing or not signed in, the menu says so in its empty
  row ("Install GitHub CLI (gh) and run `gh auth login` in a terminal").
- Safety: anyone who can open an issue wrote its title, text and comments,
  so the agent gets them inside `<untrusted origin="github issue">` with a
  note that instructions in them are not the person's; a log goes inside
  `<untrusted origin="ci">` with the same note; links go inside too (a
  check's link is whatever the app that posted it set). Both get the usual
  secret check (a token in an issue waits for Send anyway) and size caps.
  URLs lose their user part in the forge. An issue or log that cannot be
  read adds no chip and gives the typed `@#query` back. The agent gets no GitHub tool: the person's click reads, the
  agent only gets the text.
- A failed check of a pull request Ovra opened for a worker keeps its
  **Send log** in Reviews (`docs/specs/pr-flow.md`); `@#` is for the
  person's own branch and for new tasks. Not yet: a fork workflow (the
  branch tracks the person's fork, the pull request is in the parent
  repository) finds no pull request.

## Safety

The core checks every item, when it is added (`context.check`) and again
when it is sent (`agents.start`, `agents.send`, `orchestration.start`):

- Paths go through `RelPath::parse`: nothing outside the project.
- Files and folders are **referenced, never read** by Ovra: the agent reads
  them with its own tools, so the sandbox and approvals apply as before.
- An item is **sensitive** when its path is protected
  (`ovra_core::protected`), or looks like a secret store (`.env*`, `*.env`,
  `*.pem`, `*.key`, `*.tfvars`, `id_rsa*`, `.npmrc`, `.netrc`,
  `credentials*`, `service-account*`, `kubeconfig*`, `secrets/`, `.ssh/`,
  `.aws/`, `.kube/` and the like), or its text holds a token, a secret
  header, a `password=`-style pair or a URL with a password (the audit log's detector) or one of
  the project's secret values. A sensitive chip is amber with the reason
  ("May hold a secret") and is not sent until the person chooses **Send
  anyway** on it (Start and Send stay disabled until then); the core refuses a sensitive item without that confirmation.
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
`problem`, `terminal`, `page`, `issue`, `check`, each with `confirmed`), `context.check
{ project, session?, items }` answering `context.checked { notes }` (one `null` or a
warning per item). `agents.start`, `agents.send` and `orchestration.start`
take an optional `context: ContextItem[]`.

## Out of scope

Limits and cost display; reading file contents into the message; images in
messages; a test view; saved context sets.
