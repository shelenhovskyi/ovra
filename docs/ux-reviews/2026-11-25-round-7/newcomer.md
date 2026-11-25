# Nadia, the newcomer: Round 7 (2026-11-25)

Build: 3b6b65a (main, headless build already in `target/debug` and `ui/dist`, not rebuilt), Playwright's bundled Chromium, 1280x800.
Scope: the screens a newcomer meets after her first agent session: the More menu (Audit log, Project policies, Network and secrets, Servers, Agent browser), the Git sidebar tab (stage, commit, branch, diff, discard), the Search sidebar tab, opening, reading and editing files (viewer, editor, tabs, save, unsaved changes, reload), the terminal, and the Review inbox (keep, reject, comment, two workers changing the same file). Fixtures: `small` (seeded as demo-app), `empty` (seeded in one run), `monorepo` (created, not used). The scripted fake agent was used throughout; "Plan and split" with "Add notes for the project" gives three workers that each write `NOTES.md`.

Runs: `more`, `files`, `git`, `search`, `inbox`, `inbox2`, `settings`, `empty`. Screenshots and logs are in `/tmp/claude-0/r7-newcomer/<run>/`; this report names them as `<run>/<file>`. Five are copied to `/mnt/project-files/ovra/screenshots/ux-round2/newcomer-r7-*.png`. IDs continue after NEW-62 from round 6.

## Summary

Nadia could do every task in scope: read and edit a file, save it, recover unsaved text after a reload, stage, commit and branch, search, use a terminal, and keep or reject workers' changes. The worst problems are about trust in what she sees. The only Edit button sits at the end of the tab strip, so once eight or more files are open it scrolls out of sight (NEW-63). Typing into a file she is viewing is silently ignored. In the inbox, keeping a second worker's `NOTES.md` says "your version changed since the worker started", but she never touched the file, and both versions are the same (NEW-64). Most screens are calm and well worded. The More menu panels are the exception: they read like settings pages written for the people who built them, with raw library errors (NEW-65), rule syntax and JSON she is asked to write by hand.

New: 0 blocker, 2 major, 10 minor, 6 polish.

Re-check of round 6: NEW-53 (no sandbox shown on a session) is fixed. Every session row and the session detail now say "No sandbox", and the detail adds "the agent can read and change anything your user can" (`inbox/72-after-allow.png`). NEW-60 (audit entries written for engineers) is still open: `Allowed mcp__ovra__request_review`, raw `{"summary":"Wrote NOTES.md."}`, `SHA-256 of what it writes: 7194…` (`inbox2/86-audit-full.png`). NEW-61 (inbox items titled "Worker: …") is still open: "Worker: lint · Add notes for the project".

## Findings

### NEW-63 major: The only way to edit a file is a small "Edit" link at the end of the tab strip; with eight or more tabs it scrolls out of view, and typing in the viewer does nothing

- **Area:** ui/src/editor (Tabs.svelte, the `.mode` button inside the scrolling `.tabs` strip), ui/src/viewers (Viewer.svelte)
- **Steps:** 1. Open demo-app and click `src/main.rs` in the tree. 2. Click into the text and type `XYZ`, as in VS Code. 3. Click ten more files in the tree (each opens its own tab). 4. Look for Edit.
- **Expected:** Clicking into a file and typing either edits it or shows a hint such as "Read-only view. Press Edit (Ctrl+E) to change this file." Edit sits next to the file (in the path bar beside "Open in default app") and is always visible. VS Code opens a file in a "preview" tab that the next click replaces, so the tab strip does not fill up while she browses.
- **Actual:** The typed text vanishes with no message, and the viewer only shows a blue focus frame (`files/22-typed-in-viewer.png`). The Edit/View toggle is a link-styled button inside the horizontally scrolling tab strip. With ten tabs at 1280 px it is at x=939.7 and the strip ends at x=940, so 0.3 px of it shows (`settings/96-many-tabs.png`). Nothing shows that the strip scrolls: the first tab is cut to "ile with spaces.txt". Every tree click adds a permanent tab, so ten clicks are enough. Mod+E works but is not shown on the button (it has no title).
- **Why it matters for Nadia:** "Find a file, read it" is her core job, and changing a line is the natural next step. After a few minutes of browsing, the one control that turns reading into editing disappears, and typing gives no clue why nothing happens.
- **Suggested fix:** Move Edit/View out of the scrolling strip, into the file's path bar (`src/main.rs … Edit (Ctrl+E) | Open in default app`) or pin it outside the strip. When the viewer has focus and she presses a printable key, show a one-line notice "This is a read-only view. Edit (Ctrl+E)". Show fade edges or scroll buttons when the tab strip overflows. Consider a preview tab for single clicks in the tree, so browsing does not pile up tabs (planned or not, the overflow problem stays without it).

### NEW-64 major: Keeping two workers' versions of the same file says "your version changed", though she never touched it and the two versions are identical

- **Area:** ui/src/inbox (describe.ts wording, Inbox.svelte); the reason may also need a field from crates/orchestration review state
- **Steps:** 1. Start "Add notes for the project" with "Plan and split". 2. Allow the three Write NOTES.md cards. 3. Reviews: each item shows "1 file, 1 also in another open review". 4. Keep lint's changes. 5. Select tests' item and click Keep changes.
- **Expected:** Before the second Keep, the warning says what happens: "lint, tests and docs all wrote NOTES.md. You can keep only one version; the others will not be copied." After it, the result names the real cause: "NOTES.md was not copied: you already kept lint's version. The two versions are the same, so there is nothing to do." Ideally Ovra sees that the content is identical and simply marks the file as kept.
- **Actual:** The status line, header, notice and file badge repeat the same thing four times: "Kept, not all copied" / "Kept, not all copied. Not copied: NOTES.md." / "Not copied into your project folder, because your version changed since the worker started: NOTES.md … Replace mine with the worker's version, copy parts by hand, or keep yours and mark this done." / "Not copied: your version changed" (`inbox2/81-second-kept.png`). Both files contain "Notes from a worker." The inbox opened on the last item (docs), not the first. Before Keep, "Also changed in another open review" did not say what would happen.
- **Why it matters for Nadia:** She reads messages literally. "Your version changed" makes her think she broke something, and she has three options (Replace mine, copy by hand, Mark done) for a non-problem. "Plan and split" makes this case common: workers often touch shared files.
- **Suggested fix:** When the target changed because another review of the same task was kept, say so and name that worker. If the bytes are equal, treat the file as copied. Before Keep, turn "Also changed in another open review" into a sentence that names the other items and says that only the first one kept is copied. Show the cause once, not in four places. Open the inbox on the first waiting item.

### NEW-65 minor: Network and secrets shows a raw D-Bus error and repeats itself

- **Area:** ui/src/network (NetworkPanel.svelte); the error text comes from the secrets service in the daemon
- **Steps:** 1. More, then Network and secrets, on a Linux machine without a running keyring (here, headless).
- **Expected:** "Secrets can't be saved: no system keychain is running (on Linux, GNOME Keyring or KWallet). Start one and reopen this panel. Ovra never stores secrets in a file." The form is disabled while saving is impossible. Technical details go behind "Details".
- **Actual:** "Secrets cannot be stored: the Secret Service cannot be reached (zbus error: Failed to connect to address `unix:path=/run/user/0/bus`: No such file or directory (os error 2)); Ovra keeps secrets nowhere else. Ovra keeps them only in the system keychain, never in a file." The last two sentences say the same thing. The Name, Host, Header, Prefix and Value fields stay editable (`more/10-network.png`).
- **Why it matters for Nadia:** "zbus", "os error 2" and a socket path are exactly the kind of error text that scares her. She cannot tell whether she did something wrong or what to install.
- **Suggested fix:** Map the "service unreachable" case to one plain sentence with the fix, keep the raw error in a collapsed "Details", delete the duplicate sentence, and disable the form with the reason shown next to Save secret.

### NEW-66 minor: Project policies gives two contradictory errors for one typo and lets her type a match for tools that do not take one

- **Area:** ui/src/policy (PolicyPanel.svelte, rules.ts)
- **Steps:** 1. More, then Project policies. 2. Click Add rule as it is. 3. Type `bash`, match `rm -rf`, action Deny, then Add rule. 4. Type `WebFetch`, match `github.com`, then Add rule.
- **Expected:** The Tool field starts empty and clearly so. A wrong case is fixed for her, or there is one error: "Did you mean Bash?". The Match field is disabled with "WebFetch has no match: the rule covers every call" when the chosen tool does not take one. The intro is two short sentences, with the ordering rules behind "How rules combine".
- **Actual:** Step 2 says "Name the tool, such as Bash or WebFetch.": the grey placeholder "Bash" looked like a value. Step 3 shows a yellow hint "Tool names match exactly: bash never matches Bash. Write Bash." and also a red server error "bash(rm -rf): only Bash and file tools take a match; leave it out to match every call" (`settings/90-policy-bash.png`). Read literally, the second message says her Bash match is not allowed. Step 4 is accepted by the form and refused by the core with "WebFetch(github.com): only Bash and file tools take a match…". The intro is 60 words with "deny/ask/auto", "checked first", and "auto never skips more than they allow". "File tools" is never explained.
- **Why it matters for Nadia:** A rule like "never run `rm -rf`" is the first one a careful newcomer tries. Two errors that disagree, written in rule syntax, make her give up on the panel.
- **Suggested fix:** Use a select (or a combobox that only accepts list values) for Tool, with the suggestions it already has (Bash, Read, Edit, …). Disable Match for tools without one and say why. Do not send a rule the client already knows is invalid, so only one message shows. Write errors in plain words, not `Tool(match)` syntax. Shorten the intro.

### NEW-67 minor: Servers' empty state asks her to hand-write JSON in a hidden folder, with no button to start

- **Area:** ui/src/servers (ServersPanel.svelte)
- **Steps:** 1. More, then Servers.
- **Expected:** "No dev servers yet. [Create servers.json]". The button creates `.ovra/servers.json` from the example and opens it in the editor. It also explains why she would want one: "so agents can open your app while they work".
- **Actual:** "No servers are defined for this folder. Add them to .ovra/servers.json in the project. Each runs its command (no shell) in a sandbox with PORT set; agents working in this folder can reach it while it is ready. Only you can edit the file." Then a JSON example (`more/10-servers.png`). "Runs in" is a select with one option, "Project folder". "No shell", "PORT set" and "Only you can edit the file" go unexplained. The `.ovra` folder is not in the tree, so she has to create a hidden folder herself.
- **Why it matters for Nadia:** She does not know the schema or why agents need servers. The panel leaves her with homework and no first step.
- **Suggested fix:** Add a "Create servers.json" action that writes the example and opens it. Start the copy with the benefit. Hide "Runs in" while it has only one choice.

### NEW-68 minor: After a file is open, a mouse user has no visible way to open the terminal or search, and the More menu does not list them

- **Area:** ui/src/shell (Shell.svelte header, MoreMenu.svelte)
- **Steps:** 1. Open demo-app and open any file (the start panel with "Open a terminal" goes away). 2. Look for a terminal entry in the header, More menu or footer.
- **Expected:** A visible entry, as in VS Code's Terminal menu or status bar: a "Terminal" button in the footer or header, or an item in More.
- **Actual:** The header has only Reviews and More. More lists Audit log, Project policies, Network and secrets, Servers and Agent browser (`more/02-more-menu.png`). The only mouse path left is the Git tab, "Open terminal". Search is a sidebar tab, so it is fine.
- **Why it matters for Nadia:** She only vaguely knows Ctrl+` from VS Code and works with the mouse first. The start panel teaches the terminal once, then the entry disappears.
- **Suggested fix:** Add a "Terminal" toggle to the footer (next to "Language servers off"), or a "Terminal" item at the top of More with its shortcut.

### NEW-69 minor: Clicking the Search tab does not put the cursor in the search box

- **Area:** ui/src/search (Search.svelte / SearchPanel.svelte), ui/src/shell (sidebar tabs)
- **Steps:** 1. Click the Search tab in the sidebar. 2. Type `hello`.
- **Expected:** The text goes into "Search in files", as with VS Code's search view.
- **Actual:** Focus stays on the tab button (`<button aria-pressed="true">Search</button>`) and the typed text is lost. The panel still says "Type to search the contents of the project's files." (`search/51-search-hello.png`).
- **Why it matters for Nadia:** The hint tells her to type, she types, and nothing happens.
- **Suggested fix:** Focus the search input when the Search tab is chosen by a click, and keep the current Ctrl+Shift+F behaviour.

### NEW-70 minor: Whether a file opens in the viewer or the editor depends on what she did last

- **Area:** ui/src/editor (open-files.svelte.ts), ui/src/shell (workspace.svelte.ts `mode`), ui/src/search (Search.svelte)
- **Steps:** 1. Open `main.rs`, click Edit and type a line. 2. Click `lib.rs` in the tree. 3. Press Ctrl+E to go back to View, then click a search result.
- **Expected:** Each tab remembers its own mode, or a tree click always opens a file the same way.
- **Actual:** Edit mode is global. After step 1, every tree click opens files in the editor (`files/25-switched.png`), and a search result always opens the editor (`search/54-search-open.png`). The same README.md is rendered Markdown in one place and raw text marked "Plain text" in another.
- **Why it matters for Nadia:** She cannot predict what a click does. A rendered README suddenly showing raw `# Demo app` looks like something broke.
- **Suggested fix:** Store the mode per tab: tree clicks open new tabs in View, and search results keep opening in the editor at the match. Or, simpler, make a tree click always open View.

### NEW-71 minor: Git row actions are bare glyphs, and Discard sits between Open and Stage

- **Area:** ui/src/git (FileSection.svelte)
- **Steps:** 1. Git tab. 2. Look at a changed file's row: `↗ ↺ +`.
- **Expected:** Recognisable icons, and the destructive action set apart (last, or only on hover, in a warning colour).
- **Actual:** Three 24x24 buttons with the Unicode glyphs ↗ (Open), ↺ (Discard) and + (Stage). Their meaning shows only in the tooltip. Discard is in the middle, one slip from Stage (`git/40-git.png`). It asks for confirmation and offers Undo, which is good, but "Discard your changes to new_module.rs? The file is deleted." is still a scary moment to reach by mistake.
- **Why it matters for Nadia:** She does not know that ↺ means "throw away my work".
- **Suggested fix:** Use drawn icons from one set, put Discard last with a gap or show it only on hover, and colour it as a warning. Keep the confirmation.

### NEW-72 minor: After a commit, the diff tab stays open and shows "No unstaged changes in this file"

- **Area:** ui/src/git (GitDiff.svelte)
- **Steps:** 1. Git tab, click `main.rs` (its diff opens). 2. Stage it with +. 3. Commit.
- **Expected:** The diff follows the file: it switches to "Staged" after staging, and closes or says "Committed in 83166ba" after the commit.
- **Actual:** The tab "main.rs diff" stays on "Unstaged changes" with "No unstaged changes in this file." and a "Show staged changes" button, even after the commit (`git/fail-search.png`, main area).
- **Why it matters for Nadia:** "No changes" right after she committed makes her wonder whether her change was lost.
- **Suggested fix:** When the shown side becomes empty, switch to the other side if it has changes; otherwise show "No changes left: committed as 83166ba" with a Close button.

### NEW-73 minor: Agent browser opens as a pane with a dead-end message, unlike the other More items

- **Area:** ui/src/browser (BrowserPane.svelte), ui/src/shell (MoreMenu.svelte)
- **Steps:** 1. More, then Agent browser, before any session.
- **Expected:** The same kind of surface as its menu siblings, or a clear next step: "Each agent session gets its own browser. [Give Claude a task]".
- **Actual:** A pane appears under the editor: "Browser. Start an agent session first: each session has its own browser." It has a Hide button and no action, and Escape does not close it, while the four other items open modal dialogs that close with Escape (`more/10-browser.png`).
- **Why it matters for Nadia:** She clicked a menu item and got a half-screen panel she cannot use or dismiss the way she just learned.
- **Suggested fix:** Add a "Give Claude a task" button that focuses the prompt. Close the empty pane with Escape. Consider greying out the menu item with "(starts with a session)" until a session exists.

### NEW-74 minor: Search's regex error is the engine's text, including the internal wrapper

- **Area:** ui/src/search (SearchPanel.svelte); the message comes from the core's search
- **Steps:** 1. Search, turn on `.*`, type `fn (`.
- **Expected:** "Not a valid regular expression: a `(` is not closed. Turn off .* to search for the text as typed."
- **Actual:** "invalid search pattern: regex parse error: (?:fn () ^ error: unclosed group", in red, in lower case. It shows a `(?:` she never typed (`search/53-search-regex.png`).
- **Why it matters for Nadia:** She may turn on `.*` by accident. The message reads as a crash and does not say how to get back to plain search.
- **Suggested fix:** Map parse errors to one plain sentence that suggests turning off regex. Do not show the internal non-capturing wrapper.

### NEW-75 polish: The five More panels have four different sizes and layouts

- **Area:** ui/src/shell (shared dialog frame), ui/src/audit, ui/src/policy, ui/src/network, ui/src/servers, ui/src/browser
- **Steps:** Open each More item in turn.
- **Actual:** Audit log 1232 px wide, Policies and Network 762, Servers 902, Review inbox 1232, and Agent browser is a docked pane. The empty Audit log is a full-window empty sheet that shows its filter row (Show: Everything, Refused only) over "Nothing recorded yet." (`more/10-audit.png`).
- **Suggested fix:** Use one dialog frame and two widths (form: about 760 px, list or diff: full). Hide filters while the log is empty, and give the empty state a next step: "Nothing yet. Entries appear when an agent starts or asks to use a tool."

### NEW-76 polish: The same Git message shows twice

- **Area:** ui/src/git (GitPanel.svelte notice, CommitBox.svelte)
- **Steps:** 1. Click Commit with nothing staged. 2. Later, commit something.
- **Actual:** "Stage changes to commit them, or amend the last commit." shows at the top of the panel and again under the button. "Committed 83166ba: Say hello world" also shows twice (`git` session log).
- **Suggested fix:** Show each message once, next to the Commit button.

### NEW-77 polish: An empty repository shows the whole Git panel with nothing to do

- **Area:** ui/src/git (GitPanel.svelte)
- **Steps:** Open the `empty` fixture, then the Git tab.
- **Actual:** Commit box, "Amend", Staged changes (0) with "Unstage all", Changes (0) with "Stage all", the push and pull hint, and Recent commits (collapsed, empty) (`empty/a1-empty-git.png`).
- **Suggested fix:** With no changes and no commits, show one line, "No changes yet. Edit a file or give Claude a task; changes show up here.", and hide the empty sections and their buttons.

### NEW-78 polish: "Keep changes" and "Keep reviewing" sit side by side with opposite meanings

- **Area:** ui/src/inbox (Inbox.svelte)
- **Steps:** Review item, then Reject….
- **Actual:** The confirm row puts "Reject the changes" and "Keep reviewing" right under "Keep changes" (`inbox/76-reject.png`).
- **Suggested fix:** Rename the cancel button "Cancel" or "Back", so "Keep" always means "keep the agent's changes".

### NEW-79 polish: Small wording and duplication in Network

- **Area:** ui/src/network (hosts.ts, NetworkPanel.svelte)
- **Actual:** `https://github.com/` is refused with "Type only the host name, without a scheme, path or wildcard." instead of being cleaned to `github.com`. "Always allowed for the agent itself (4)" lists `api.anthropic.com` and `api.anthropic.com:443` as two of the four. `localhost` is accepted silently.
- **Suggested fix:** Strip the scheme and path and keep the host. Merge entries that differ only by the default port. Say what `localhost` means inside the sandbox (or refuse it with a reason).

### NEW-80 polish: Two different × buttons in the terminal panel

- **Area:** ui/src/terminal (Terminals.svelte)
- **Actual:** × on a terminal's tab ends that shell. The × at the far right only hides the panel. Both look the same (`git/61-terminal-ls.png`). An exited terminal says "Process exited with code 0." and has no Restart.
- **Suggested fix:** Use a chevron or "Hide" for the panel button, and add "Restart" to an exited terminal's message.

## Nielsen scores per screen

Ten heuristics, each 0 to 4 (H1 status, H2 real-world words, H3 control, H4 consistency, H5 error prevention, H6 recognition, H7 flexibility, H8 minimalism, H9 error recovery, H10 help). Total out of 40.

| Screen               | H1  | H2  | H3  | H4  | H5  | H6  | H7  | H8  | H9  | H10 | Total | Weakest point                                       |
| -------------------- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | ----- | --------------------------------------------------- |
| More menu            | 3   | 3   | 4   | 3   | 3   | 4   | 3   | 3   | 3   | 3   | 32    | No terminal entry; only 2 of 5 items show shortcuts |
| Audit log            | 3   | 1   | 3   | 2   | 3   | 2   | 3   | 2   | 2   | 2   | 23    | Tool ids, JSON and hashes (NEW-60)                  |
| Project policies     | 2   | 1   | 3   | 2   | 1   | 2   | 2   | 2   | 1   | 2   | 18    | Two contradictory errors, jargon intro (NEW-66)     |
| Network and secrets  | 3   | 2   | 3   | 2   | 2   | 3   | 2   | 2   | 1   | 3   | 23    | Raw zbus error (NEW-65)                             |
| Servers              | 2   | 2   | 3   | 2   | 2   | 1   | 2   | 3   | 2   | 2   | 21    | Hand-written JSON, no first step (NEW-67)           |
| Agent browser        | 2   | 3   | 2   | 1   | 3   | 2   | 2   | 3   | 2   | 2   | 22    | Dead end, different surface (NEW-73)                |
| Git tab              | 3   | 3   | 3   | 3   | 3   | 2   | 3   | 2   | 3   | 3   | 28    | Glyph buttons, stale diff (NEW-71, NEW-72)          |
| Search tab           | 3   | 3   | 3   | 3   | 3   | 3   | 3   | 3   | 1   | 2   | 27    | No focus on open, raw regex error                   |
| Viewer, editor, tabs | 3   | 3   | 3   | 2   | 3   | 1   | 3   | 3   | 3   | 2   | 26    | Edit disappears, typing ignored (NEW-63)            |
| Terminal             | 3   | 3   | 3   | 3   | 3   | 3   | 3   | 4   | 3   | 2   | 30    | Two × meanings, no restart                          |
| Review inbox         | 3   | 2   | 3   | 3   | 2   | 3   | 3   | 2   | 2   | 3   | 26    | Misleading "your version changed" (NEW-64)          |

## What worked well

- Unsaved changes are handled well. Closing a changed tab asks "Save changes to src/main.rs before closing?" with Save, Don't save and Cancel (`files/26-close-question.png`). A reload keeps the draft and says "Showing unsaved changes from the editor." (`files/28-after-reload.png`). Ctrl+S saves and the dot clears.
- Files that cannot be shown say so plainly and offer a way out: "This file is 6.0 MB, too large to show here. [Open in default app]" (`files/30-huge.png`).
- Git explains itself in her words: "Nothing is staged. Staged changes (git calls this the index) go into the next commit.", "Push and pull run in your terminal…" with an Open terminal button. A branch name with a space gets "Branch names cannot contain spaces. Try my-feature." Discard asks first and offers Undo.
- The inbox's Reject confirmation says exactly what happens: "Reject discards the worker's changes from its own copy of the project. Your project folder is not touched." "Comment sent. The agent continues, and this review updates when its changes do." is a good confirmation.
- Sessions now say "No sandbox" on every row, and the header badge "Reviews (3 waiting)" plus the toast with a Review button lead her straight to the inbox (`inbox/72-after-allow.png`).
- The More menu gives each item a one-line purpose ("Which websites agents may reach, and secrets"). Network warns "Anyone can receive data here, so an agent could send your files out through it." when she allows github.com.
- The terminal opens fast in the project folder and marks itself "(exited)" with the exit code.
