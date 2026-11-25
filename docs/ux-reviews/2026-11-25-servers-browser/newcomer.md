# Nadia, the newcomer: servers and agent browser

Build: 7cf1845, headless (`--fake-agent --fake-servers --fake-browser`), Chromium 158.0.8127.36, 1280x800.
Scope: Servers dialog (Mod+Shift+S), agent browser pane (Mod+Shift+B): take over, pick, navigation ask, "unavailable" state. Fixtures: `small` (opened from the start screen and seeded), `empty`.

## Summary

Nadia found both features on her own (the "Servers" and "Browser" buttons in the top bar, and "Servers" / "Agent browser" in the palette). With a working
`servers.json` and a sandboxed session she could start, stop and restart servers, read their output, take over the page and pick an element that shows up as a chip in her reply.
The worst problems come before and after that path. Ovra tells her to create `.ovra/servers.json` but gives her no way to create it, and agents are not allowed to write it. A ready server has no way to open it. When the browser is "unavailable" for a session without the sandbox, the text blames a missing Chromium and the Linux sandbox, so she would go install a browser she already has.
Once set up, the features feel calm and well explained. Getting there means editing JSON by hand, and the error text reads like parser output.

## Findings

### NEW-1 major: "The agent browser cannot run here" blames Chromium and the Linux sandbox when the real reason is this session

- **Area:** ui/src/browser (wording also in crates/core, the `unavailable` message)
- **Steps:** 1. Open `demo-app`. 2. Type "Check the home page looks right" in "Task for a new Claude session" and click Start. This is a normal session without the sandbox. 3. Press Mod+Shift+B, type `localhost:5173`, click "Open in new tab".
- **Expected:** One plain sentence about this session, for example "This session runs without the sandbox, so it has no browser. Start a new session with the sandbox on to let the agent use a browser", plus what to do next.
- **Actual:** In yellow: "The agent browser cannot run here. Ovra uses a Chromium or Google Chrome installed on this computer (`chromium`, `chromium-browser` or `google-chrome` on PATH) and never downloads one. For now it also needs the Linux sandbox…". Below that, in grey: "The core said: the agent browser runs only for sessions whose agent runs in the sandbox behind its own netproxy, and this session's agent does not". "Try again" hides the message, then the next Open shows it again (`c01-unavail.png`, `b03-plain-open.png`). `browsers.unavailable` is a single field for the whole pane, not one per session (`browser.svelte.ts:19`), so the message can follow her to a session where the browser does work. I could not confirm this in the UI because the second session did not start in my run.
- **Why it matters for Nadia:** She reads every message literally. The big yellow text says she lacks Chromium or the sandbox, so she would go install Chrome. The real cause is in a grey line that is full of jargon ("netproxy", "core said").
- **Suggested fix:** Have the core send a reason code (`no_browser_binary`, `no_sandbox_support`, `session_not_sandboxed`) and show one sentence for each code. Keep the "unavailable" state per session. Replace "Try again" with the action that helps, for example "Start a sandboxed session". Drop "The core said:".

### NEW-2 major: The empty Servers dialog says to create `.ovra/servers.json`, but Ovra cannot create it and agents may not

- **Area:** ui/src/servers (needs a file-create call from the fs area if none exists)
- **Steps:** 1. Open `demo-app`, which has no `.ovra`. 2. Click "Servers".
- **Expected:** A "Create servers.json" button that writes the example shown and opens it in the editor.
- **Actual:** "No servers are defined for this work dir. Add them to `.ovra/servers.json` in the project… Only you can edit the file." The dialog shows an example but has no button (`04-servers-empty.png`). The tree has no "new file" action. `.ovra/` is a protected path, so asking the agent to write the file fails too. The only way is another editor or a terminal.
- **Why it matters for Nadia:** This is the first step of the feature, and it sends her out of the app. She also does not know how to create a dot-folder.
- **Suggested fix:** Add a "Create .ovra/servers.json" button that writes the example (with the project's real `dev` script from `package.json`, if there is one) and opens it in an editor tab, already in Edit mode. Also add a "Copy" button on the example.

### NEW-3 major: A ready server cannot be opened in a browser

- **Area:** ui/src/servers
- **Steps:** 1. Define `web` (port 5173) and click Start. 2. Wait for the green "ready" badge.
- **Expected:** An "Open" button or link on the ready row: in her own browser (spec "Open in browser", `servers.open`, task S.F), or in the agent browser.
- **Actual:** The row offers Output, Restart and Stop only (`08-outputs.png`). The protocol already has `servers.open`/`servers.opened` (`ui/src/lib/protocol/servers.ts:68`), but nothing in the UI calls it. Typing `localhost:5173` into the agent browser instead asks her to "Let the agent open" it (see NEW-8).
- **Why it matters for Nadia:** She starts a dev server so she can look at her app. "ready, port 5173" with nothing to click is a dead end, and `localhost:5173` in her own browser does not work, because the server runs inside a pod.
- **Suggested fix:** Show "Open in browser" on ready rows, calling `servers.open` and opening the returned URL. Add "Show in agent browser" when a session is selected.

### NEW-4 minor: The Servers dialog does not notice a new or changed `servers.json`

- **Area:** ui/src/servers
- **Steps:** 1. Open the empty Servers dialog. 2. Create `.ovra/servers.json` with three servers (from outside, as she has to; see NEW-2). 3. Look at the dialog.
- **Expected:** The list shows the three servers, or there is a "Reload" button.
- **Actual:** The tree shows `.ovra` right away, but the dialog still says "No servers are defined" (`05-after-write-no-reopen.png`). The new servers appear only after Close and reopen. After fixing a broken file, the error also stays until she reopens the dialog.
- **Why it matters for Nadia:** She thinks her file is wrong and starts changing correct JSON.
- **Suggested fix:** Re-list when `.ovra/servers.json` changes (the fs watcher already updates the tree), or at least add a "Reload" button next to the file error and the empty state.

### NEW-5 minor: `servers.json` errors read like parser output

- **Area:** crates/core (servers definition parsing), ui/src/servers
- **Steps:** Open the Servers dialog with each of these files: `"command": "pnpm dev"`; `"name": "Web App"`; a trailing comma.
- **Expected:** Plain sentences that say what to write, and a way to jump to the file.
- **Actual:** "Could not read .ovra/servers.json: .ovra/servers.json: invalid type: string "pnpm dev", expected a sequence at line 1 column 46" (`case-string-command.png`). "servers[0]: the name must match ^[a-z0-9][a-z0-9-]{0,31}$". The path appears twice. "port must be 1024 to 65535", "another server uses port 5173" and "cwd … leaves the project" read fine.
- **Why it matters for Nadia:** A regex and "expected a sequence" mean nothing to her. Writing `"pnpm dev"` as one string is the most likely beginner mistake.
- **Suggested fix:** Map serde type errors per field: "`command` is a list: write `["pnpm", "dev"]`". For names: "lowercase letters, digits and dashes, up to 32 characters". Drop the repeated path. Add an "Open servers.json" button that jumps to the line.

### NEW-6 minor: Server states use technical words and show a crash in grey

- **Area:** ui/src/servers
- **Steps:** 1. Start `web`, then click Stop. 2. Start `worker`, which exits with code 1.
- **Expected:** "stopped" (by you) for 1. "crashed (exit code 1)", shown as an error, for 2.
- **Actual:** "exited (signal)" for the server she stopped herself. "exited (code 1)" in the same neutral grey as a normal stop, while "failed: port 8080 is taken" is red (`10-stopped.png`). The rows also show "run 2" and "in main checkout", and the header says "work dir". She cannot read any of these.
- **Why it matters for Nadia:** She cannot tell "I stopped it" from "it broke", and a crash does not stand out.
- **Suggested fix:** Show "stopped" when the stop came from a person or agent, and "crashed (exit code N)" in the error style for a nonzero exit. Give "run N" a tooltip or hide it. Say "project folder" instead of "work dir" in the main checkout.

### NEW-7 minor: Escape stops working in the Servers dialog after clicking Stop; clicking the backdrop does nothing

- **Area:** ui/src/servers
- **Steps:** 1. Start `web`. 2. Click "Stop web". 3. Press Escape. 4. Click the dark backdrop.
- **Expected:** Escape closes the dialog, as the code comment and `aria-keyshortcuts` say. A click outside closes it too.
- **Actual:** The Stop button becomes disabled and focus drops to `<body>`, so Escape does nothing and the dialog stays open. The backdrop click does nothing either (session log: "ACTIVE after stop: BODY", "DIALOG visible after Escape: true"). Pressing Mod+Shift+S then closes the dialog instead of focusing it.
- **Why it matters for Nadia:** Escape works the first time and then stops working. Only the Close button still works.
- **Suggested fix:** Handle Escape on the backdrop or window while the dialog is open. When the focused button becomes disabled, move focus to the row or to the Start button. Close the dialog on a backdrop click.

### NEW-8 minor: The navigation ask asks her to let "the agent" open an address she typed, without saying why

- **Area:** ui/src/browser
- **Steps:** 1. In a sandboxed session ("Browse the page"), open the Browser pane. 2. Type `localhost:5173` and click "Open in new tab".
- **Expected:** Either the page opens, because she asked for it, or the pane explains why it asks, for example "Nothing Ovra runs is on port 5173. Allow this session's browser to open it?".
- **Actual:** "Browse the page: Let the agent open **http://localhost:5173**? Allow / Deny" (`b05-ask.png`). The badge on the top "Browser" button is a nice touch.
- **Why it matters for Nadia:** She did not ask the agent to do anything, so being asked to give the agent permission feels like a bug.
- **Suggested fix:** When the person opened the page, word the ask from her side and give the reason (no server on that port, or a host that is not on the allowlist). If no server is defined, link to the Servers dialog.

### NEW-9 minor: The notification toast covers the page view and the pick's "Note for the agent" field

- **Area:** ui/src/shell (toasts), ui/src/browser
- **Steps:** 1. Start "Browse the page", open the page, then pick the button. 2. Look at the bottom of the Browser pane.
- **Expected:** The page and the pick list stay readable and clickable.
- **Actual:** "Browse the page finished and waits for your reply" stays in the bottom right. It covers part of the page and most of the "Note for the agent (optional)" input of the pick (`b06-page.png`, `b12-chip.png`). The e2e test clicks "away from where notifications show" to avoid it.
- **Why it matters for Nadia:** She has to dismiss the toast before she can write the note, and she may think the field is missing.
- **Suggested fix:** Place toasts so they do not cover an active pane, or let the browser pane leave room for them. Auto-dismiss the toast once she is looking at that session.

### NEW-10 polish: Clicking the page does nothing while the agent drives; the page view is small

- **Area:** ui/src/browser
- **Steps:** Open a page and click on it before pressing "Take over".
- **Expected:** A hint, or an offer to take over.
- **Actual:** Nothing happens. The hint "Take over to use your mouse and keyboard on it" is above the page and easy to miss. At 1280x800 the page view is 656x250 px, while the editor area above it is empty (`b06-page.png`).
- **Suggested fix:** On a click in agent mode, flash the hint or show a "Take over?" button over the page. Give the Browser pane more height when no file is open, or let it be resized.

### NEW-11 polish: The Servers and Browser buttons have no tooltip with their shortcut

- **Area:** ui/src/shell (top bar), ui/src/servers, ui/src/browser
- **Steps:** Hover over "Servers" and "Browser" in the top bar.
- **Expected:** A tooltip like the one on Reviews ("Review inbox (Alt+Shift+R)") and Pick ("Pick an element (Alt+Shift+P)").
- **Actual:** There is no title. Nadia would also read "Browser" as her own web browser; the palette calls it "Agent browser".
- **Suggested fix:** Add titles "Dev servers (Ctrl+Shift+S)" and "Agent browser (Ctrl+Shift+B)", and consider the label "Agent browser".

## What worked well

- Both features are visible in the top bar, and the palette finds "Servers" (Ctrl+Shift+S) and "Agent browser" (Ctrl+Shift+B) by the first word Nadia would type.
- The empty state explains the file and shows a real example. The JSON checks catch duplicate ports, bad ports and a `cwd` outside the project before anything runs.
- Server rows are clear once running: a coloured status badge, "failed: port 8080 is taken" in plain words, "started by you", and output with ANSI colours. Start, Stop and Restart respond within about 70 ms.
- The browser pane states each step: "Start an agent session first", "No pages open. The agent opens pages…", "The agent drives this page", "You are controlling the page. Shift+Escape gives it back." Plain Escape correctly does not end control.
- Picking is a pleasure: a hover label (`button "Save"`), a numbered green outline, a chip under "Reply to Claude", and "Picks go with your next message to this session."
- A pending navigation ask puts a badge on the top "Browser" button, so she sees it even with the pane hidden.
