# Round 6 summary (2026-11-25): phase 4 security UI

Build: `5dbc67a` on `main`, headless, scripted fake agent (and the real
Claude driver against `fake-claude` for the sandbox report), Chromium.
Scope: protected-file marks in the review inbox, sandbox status, policies,
the audit log. The fake agents never write protected files, so scripts wrote
them into the worker's copy or the project folder, standing in for the
agent's Bash.

Reports: [security-reviewer](security-reviewer.md), [newcomer](newcomer.md).

## Issues, ranked

| #   | Severity | Issue                                                                                            | Findings       | Area                        |
| --- | -------- | ------------------------------------------------------------------------------------------------ | -------------- | --------------------------- |
| 1   | major    | One Approve applies protected files whose diffs were never shown                                 | SEC-1, NEW-54  | `ui/src/inbox`, review core |
| 2   | major    | No session says whether it runs in a sandbox; no event when the report arrives                   | SEC-2, NEW-53  | `ui/src/agents`, protocol   |
| 3   | major    | The direct-session line promises a restore Reject does not do; the person's own edits are blamed | SEC-3, NEW-55  | `ui/src/inbox`              |
| 4   | major    | No UI to view or edit project policies                                                           | SEC-4          | new `ui/src/policy`         |
| 5   | major    | `.husky/`, nested `.envrc`/`.claude/` unmarked; `.git/hooks` never appears in a review           | SEC-5          | protected list, worktree    |
| 6   | minor    | The trust card says "trusted" after approving a settings change until reload                     | NEW-56         | `ui/src/agents`             |
| 7   | minor    | Setup card contradicts itself with `ANTHROPIC_API_KEY` set; a dollar amount beside plan limits   | NEW-57, NEW-58 | `ui/src/agents/setup`       |
| 8   | minor    | Empty `.claude`, `.vscode`, `.idea`, `.devcontainer` folders from the sandbox show in the tree   | NEW-59         | `crates/sandbox-linux`      |
| 9   | minor    | The audit log is written for engineers (tool ids, SHA-256, "Review r1")                          | NEW-60         | `ui/src/audit`              |
| 10  | polish   | "Worker: Worker docs" titles; an item with protected files does not stand out in the list        | NEW-61, NEW-62 | `ui/src/inbox`              |

Fixed from round 5: the stale-diff approve guard holds; NEW-41, NEW-50.
