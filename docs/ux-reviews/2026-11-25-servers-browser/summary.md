# Servers and agent browser round: summary

Build: 7cf1845, headless with fake servers and a fake browser. Personas:
newcomer, keyboard-power-user, agent-orchestrator. 31 findings, grouped
into issues below by owning area.

Every persona could start, stop and restart a server, read its output,
take over a page and pick an element. What breaks is focus in the
Servers dialog, keyboard-only picking, and what the browser pane says
when one session cannot browse.

## ui/src/browser

| #   | Issue                                                                                                      | Findings               | Severity |
| --- | ---------------------------------------------------------------------------------------------------------- | ---------------------- | -------- |
| B1  | Stopping a session with an open page throws in the page view; the pane stays blank for the next session    | ORCH-1                 | major    |
| B2  | "Unavailable" shows the machine-wide Chromium hint, not the session's reason, and sticks to other sessions | NEW-1, ORCH-4          | major    |
| B3  | Picking needs a mouse: arrow keys and Enter do nothing without a pointer position                          | KEY-1                  | major    |
| B4  | Shift+Escape leaves Shift held in the page; Alt+letter types into the page                                 | KEY-3, KEY-4           | major    |
| B5  | Focus falls to the page body after Allow/Deny and after removing a pick chip; the ask takes no focus       | KEY-5, KEY-6           | minor    |
| B6  | Navigation ask wording for an address the person typed; a denial cannot be undone                          | NEW-8, ORCH-7          | minor    |
| B7  | Tabs do not name the work dir; the pick hint does not name the session                                     | ORCH-8, ORCH-9         | minor    |
| B8  | Escape does not hide the pane; a toast covers the pick's note field                                        | KEY-7, NEW-9           | minor    |
| B9  | Clicking the page does nothing while the agent drives; header buttons hide their shortcuts                 | NEW-10, NEW-11, KEY-10 | polish   |

## ui/src/servers

| #   | Issue                                                                                       | Findings                    | Severity |
| --- | ------------------------------------------------------------------------------------------- | --------------------------- | -------- |
| S1  | After Start/Stop focus falls to body, so Escape and the toggle no longer close the dialog   | KEY-2, NEW-7, ORCH-5, KEY-8 | major    |
| S2  | Restart still offered for an approved worker's server, which then ran again in its worktree | ORCH-3                      | major    |
| S3  | One work dir at a time, always reopening on the main checkout; no count of running servers  | ORCH-2                      | major    |
| S4  | No "Open in browser" for a ready server                                                     | NEW-3, KEY-9                | major    |
| S5  | No way to create `servers.json` from Ovra                                                   | NEW-2                       | major    |
| S6  | The dialog misses a new or changed `servers.json`; raw parser errors                        | NEW-4, NEW-5                | minor    |
| S7  | A stop reads "exited (signal)"; a crash or failed start looks like a normal exit            | NEW-6, ORCH-10              | minor    |
| S8  | No palette commands to start, stop or restart a server; audit shows internal ids            | KEY-9, ORCH-6               | minor    |

S2 is fixed in the core: a worker that ended gets no new servers. B1-B4
and S1 are fixed in the same thread right after this round. S3-S5 are
follow-ups: S4 waits for `servers.open` (a host forward for the person's
own browser), S5 for a decision on whether Ovra writes `.ovra/` itself.
The next round re-checks them.
