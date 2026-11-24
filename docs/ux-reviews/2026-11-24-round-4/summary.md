# Round 4 summary (2026-11-24): agents, approvals, trust, inbox

Build: `81d16b1` on `main`, headless, debug build, Chromium. Agents: the
scripted fake agent and the real Claude driver against `fake-claude`
(`edit_allowed`, `denied_outside_folder`, `interrupted`, `subagent`, plus
reviewer-made fixtures for big diffs). New persona: security-reviewer.
Scope: terminals, Agents panel, Approvals, Claude onboarding, Trust project,
review inbox, direct mode.

Reports: [security-reviewer](security-reviewer.md),
[agent-orchestrator](agent-orchestrator.md), [newcomer](newcomer.md),
[keyboard-power-user](keyboard-power-user.md),
[monorepo-maintainer](monorepo-maintainer.md),
[accessibility-user](accessibility-user.md). 68 new findings.

Round 3 re-checks: the tree freeze while disconnected (A11Y-21, ORCH-20),
resync after reconnect (ORCH-21), late core and restarted-core screen
(ORCH-22), the close-unsaved modal (A11Y-22, KEY-24, MONO-24), screen-reader
diffs (A11Y-23), Go to file Enter and ranking (KEY-23, MONO-21) and reveal
after restore (MONO-19) are fixed. Still open: focus after closing the last
tab (KEY-22/25), long tab labels (MONO-23).

Limits of this round: neither fake agent writes files, so attribution of real
agent edits was tested with files written by the review scripts during agent
turns. The harness's logging proxy slows terminal floods; terminal numbers
come from a page without it.

## Issues, ranked

| #   | Severity | Issue                                                                                                                                                                                                                                                                                                                                               | Area                                                             | Findings                                           |
| --- | -------- | --------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------- | -------------------------------------------------- |
| 1   | blocker  | Holding Y approves every waiting request, across projects, including ones that arrive during the hold: key repeat is not ignored and focus moves to the next card. Typing a sentence can do the same: the next card takes focus itself, and the "y" in "carry" allowed `git status` in another project, "and" denied one                            | `ui/src/approvals` (security)                                    | SEC-1, KEY-29                                      |
| 2   | blocker  | Reject in direct mode reverts the person's own saved edits made while an agent Bash call ran (counted as the agent's per the spec). No marker, no file list in the confirm, no undo; the only copy is in the `refs/ovra/snapshot` reflog. Approve would commit them under Claude's name                                                             | `crates/core/src/orchestration`, `ui/src/inbox`; spec rule       | SEC-2, ORCH-28                                     |
| 3   | blocker  | Core aborts with a stack overflow when a second direct session in the same project ends (fake agent and real driver)                                                                                                                                                                                                                                | `crates/core/src/orchestration`                                  | NEW-30                                             |
| 4   | major    | Answers go to requests the person did not read: after another window answers, the next card slides under the pointer and takes the click; the toast's "Review" focuses another task's card; Allow/Deny look disabled for seconds but accept input at 500 ms; cards don't name the project or say inside/outside the project                         | `ui/src/approvals`, `ui/src/notify` (security)                   | SEC-3, SEC-4, SEC-5, SEC-6, SEC-7, KEY-41, ORCH-34 |
| 5   | major    | A trusted project's `Bash(*)` allow rule silently turns off approval for every shell command; the trust card does not warn, reads like an error, highlights Trust, and added/removed entries sound alike to screen readers                                                                                                                          | Trust card (`ui/src/agents`), `crates/agent-runtime` (security)  | SEC-8, NEW-37, A11Y-30                             |
| 6   | major    | Inbox misleads about what is decided: list stale until the agent's next turn (own edits and "changed outside" markers missing), decided items show "0 files", later changes pile into approved items, the agent is not told of a reject; Reject says "Rejected." but restores nothing for "outside" files; open reviews vanish after a core restart | `ui/src/inbox`, `crates/core/src/orchestration`                  | ORCH-30, ORCH-31, ORCH-32, NEW-31, MONO-26         |
| 7   | major    | Reject flow is easy to trigger by mistake: focus lands on "Reject the changes" (double Enter rejects), Esc in the reject step closes the whole inbox; "Approve and commit" disabled with no reason when the message is empty                                                                                                                        | `ui/src/inbox`                                                   | SEC-9, KEY-35, NEW-38, ORCH-36, A11Y minor         |
| 8   | major    | Only one agent per project: every session is direct mode, a second Start or Resume fails with "another agent already works in this project's main checkout", without naming the holder or offering an isolated session                                                                                                                              | `ui/src/agents`, orchestration                                   | ORCH-29, NEW-35                                    |
| 9   | major    | Agents panel: the transcript gets 74-84 px (0 px while an approval waits) under setup, trust and task cards; it cannot take focus; it shows text meant for the model ("do not mention to user", interrupt boilerplate) and screen readers read whole tool outputs                                                                                   | `ui/src/agents`                                                  | NEW-32, KEY-30, A11Y-28, NEW-39, MONO minor        |
| 10  | major    | Focus drops to `<body>` after Interrupt, Stop, Review and trust, Approve, Keep reviewing; Trust project jumps to the tree; Esc does nothing in the trust review                                                                                                                                                                                     | `ui/src/agents`, `ui/src/inbox`                                  | KEY-31, A11Y-27, ORCH-35                           |
| 11  | major    | At 200% zoom the main area and inbox diff shrink to 0-90 px: the 260 px sidebar and 339 px right column never shrink                                                                                                                                                                                                                                | `ui/src/shell`, `ui/src/inbox`                                   | A11Y-26                                            |
| 12  | major    | Reject of a 653-file change takes 50 s with no progress (one `git diff` per file in a loop, `direct.rs` ~405); approve takes 0.9 s                                                                                                                                                                                                                  | `crates/core/src/orchestration`                                  | MONO-25                                            |
| 13  | major    | Fast terminal output silently dropped (34-51% of `seq 1 3000000` arrives): one 1024-slot broadcast channel for all events; Agents panel, inbox and terminals ignore `info.resync` (duplicated transcript turn seen)                                                                                                                                 | `crates/transport`/core events, `ui/src/terminal`, agents, inbox | MONO-27                                            |
| 14  | major    | Snapshots on big repos: 40k untracked files make session start 2.2 s, Allow-to-next-approval 6.8 s, 630 ms per diff, `.git` grows 4 MB to 165 MB kept by the snapshot reflog                                                                                                                                                                        | `crates/worktree`                                                | MONO-28                                            |
| 15  | major    | Terminal output invisible to screen readers until a palette command; the setting is not saved                                                                                                                                                                                                                                                       | `ui/src/terminal`                                                | A11Y-29                                            |
| 16  | minor    | Onboarding without Claude: Start stays enabled, failed session offers Resume; bad paths give raw errors (license text for `/usr/bin/ls`), stale "Using ..." line; setup says "run in a terminal" but there is no visible terminal button                                                                                                            | `ui/src/agents`, `crates/agent-runtime`                          | NEW-33, NEW-34, NEW-36, A11Y minor                 |
| 17  | minor    | Keyboard leftovers: global shortcuts act behind the modal inbox; terminal can be left only by hiding it; Alt+W in the terminal closes an editor tab; Approve/Reject come before the diff in Tab order; no keys for Interrupt, Stop, New session; first Tab lands on Reviews                                                                         | keys, terminal, inbox, agents                                    | KEY-32 to KEY-42                                   |
| 18  | minor    | Reload loses the terminal while its shell keeps running; edits made only by the person open a "waiting" review; inbox list for 653 files is flat with no filter or natural sort                                                                                                                                                                     | terminal, orchestration, inbox                                   | ORCH-37, ORCH-33, MONO minor                       |

## Suggested tasks

1. **Approvals hardening** (security, human review): ignore key repeat,
   require keyup between answers, never move focus to a new card by itself,
   arm a card from when it is shown and focused, re-arm on any change under
   the pointer, name the project and mark paths outside it. Highest priority.
2. **Direct-mode attribution** (spec change, owner): never revert the
   person's own saves; mark mixed files, list files in the reject confirm,
   offer undo from the snapshot.
3. **Fix the orchestration stack overflow** with a regression test for two
   sequential direct sessions.
4. **Trust card**: warn about broad allow rules such as `Bash(*)`, neutral
   wording, deny-first emphasis, text labels for added/removed.
5. **Inbox truthfulness**: refresh on open and on outside edits, keep the
   decided file list, close an item when it is decided, reject progress and a
   batched restore, survive core restarts.
6. **Agents panel layout**: collapse setup and trust cards once done, give the
   transcript the space, filter model-only text, keyboard focus.
7. **Event back-pressure**: separate terminal output from other events, and
   resync every panel on `info.resync`.
8. **Isolated sessions**: let a second task run in a worktree, or name the
   session that holds the checkout.
