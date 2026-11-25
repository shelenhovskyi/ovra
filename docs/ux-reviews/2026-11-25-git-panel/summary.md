# Git panel round: summary

Build: 1395c64, headless. Personas: newcomer, keyboard-power-user,
accessibility-user. 40 findings, grouped into issues below by owning area.

Every persona could finish the panel's tasks (stage files and hunks,
discard and undo, commit, amend, new branch). The editor side is where it
breaks: change markers are invisible, and the diff view does not fit the
tab model.

## ui/src/editor (git markers and blame)

| #   | Issue                                                                                                                                     | Findings                 | Severity |
| --- | ----------------------------------------------------------------------------------------------------------------------------------------- | ------------------------ | -------- |
| E1  | Gutter markers are 0 px wide: nobody can see them                                                                                         | NEW-1, KEY-1, A11Y-36    | major    |
| E2  | Changes in the editor cannot be found by keyboard or screen reader: gutter is `aria-hidden`, no next/previous change command, no shortcut | A11Y-37, KEY-12          | major    |
| E3  | Change popup: focus can leave the dialog, Escape then stops working, stale popup stays; focus starts on Revert for unsaved files          | A11Y-41, KEY-10, A11Y-47 | major    |
| E4  | Inline blame is read as part of the code line, 2.5:1 contrast; Toggle blame is silent                                                     | A11Y-38, KEY-14          | major    |
| E5  | Added and changed markers differ only by colour, also in forced colours                                                                   | A11Y-42                  | minor    |

## ui/src/git (panel and diff view)

| #   | Issue                                                                                                                                               | Findings                                 | Severity     |
| --- | --------------------------------------------------------------------------------------------------------------------------------------------------- | ---------------------------------------- | ------------ |
| P1  | Diff view sits outside the tab model: the old tab stays selected, Alt+W / Alt+] act on the hidden file, Escape does nothing, focus is lost on close | KEY-4, A11Y-39, A11Y-40, A11Y-45, NEW-4  | major        |
| P2  | Clicking or Enter on a file row opens the file, not its diff; place in the list is lost after each diff                                             | NEW-2, KEY-5                             | major        |
| P3  | Four Tab stops per file, no arrow keys                                                                                                              | KEY-3                                    | major        |
| P4  | Blocked switch shows raw git output with a repeated prefix and suggests stash                                                                       | NEW-3, KEY-15                            | major        |
| P5  | Amend does not keep the last message                                                                                                                | KEY-6, NEW-7                             | major        |
| P6  | Palette names "Show git changes" and "Show git change at cursor" collide; git actions are not in the palette                                        | KEY-2, A11Y-46, KEY-8                    | major        |
| P7  | Focus lands on invisible headings; silent or doubled announcements                                                                                  | A11Y-48, KEY-7, A11Y-44                  | minor        |
| P8  | Small symbol buttons, duplicate names, full hashes read aloud, low-contrast line numbers                                                            | NEW-6, A11Y-49, A11Y-43, A11Y-50, NEW-10 | minor        |
| P9  | Invalid branch name refused without a reason; branch menu focus and keys                                                                            | NEW-8, KEY-9                             | minor        |
| P10 | "Open terminal" opens an empty terminal (no way to prefill `git push`)                                                                              | NEW-5                                    | minor        |
| P11 | Git jargon, silent keys, reveal in tree from the Git tab                                                                                            | NEW-9, KEY-11, KEY-13                    | polish/minor |

Majors (E1-E4, P1-P6) are fixed in the same thread right after this round;
the next round re-checks them.
