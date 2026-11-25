# Round 7 summary (2026-11-25): beyond the first session

Personas: newcomer, keyboard-power-user, accessibility-user, on main at
3b6b65a. 43 findings: 0 blocker, 7 major, 26 minor, 10 polish. Every task in
scope can be finished; the lowest screen score is Project policies (18/40).

## Fix tasks, by owning area

**Editor, viewer, tabs, terminal, keys** (`ui/src/editor`, `ui/src/viewers`,
`ui/src/terminal`, `ui/src/keys`)

- NEW-63 major: Edit lives at the end of the scrolling tab strip; move it
  into the file's path bar, hint when typing into the read-only viewer.
- A11Y-36 major: no way to Tab out of the terminal; add a documented exit
  key and a screen reader mode toggle.
- KEY-2 major: punctuation shortcuts (Ctrl+`) fail on the Ukrainian layout;
  match punctuation by `code`.
- Minors: view/edit mode carries over to every next file; editor textbox
  has no name, gutter contrast 2.65:1, unstyled Save button; cancelling
  "Save changes?" loses the text focus; no shortcut back to Files; palette
  Ctrl+N/P; "Mod+K Mod+I" spelled raw; two different × in the terminal.

**Git, search, inbox** (`ui/src/git`, `ui/src/search`, `ui/src/inbox`)

- KEY-1 major: every search result is a Tab stop; roving tabindex, ArrowDown
  from the box.
- A11Y-38 major: git diff lines say added/removed only by a leading sign.
- NEW-64 major: keeping a second worker's identical file says "your version
  changed"; name whose version won, treat identical as copied, warn first.
- Minors: Search tab click does not focus the box; Git row buttons are bare
  glyphs with Discard in the middle; diff stays after commit; raw regex
  errors; duplicated Git messages; Escape erases the inbox comment; inbox
  titles "Worker: …" (NEW-61).

**More panels** (`ui/src/network`, `ui/src/policy`, `ui/src/servers`,
`ui/src/audit`, `ui/src/browser`)

- NEW-65: raw keyring error in Network secrets, editable form that cannot
  save. NEW-66: contradicting policy errors, jargon intro, placeholder that
  looks like a value. NEW-67: Servers empty state asks for hand-written
  JSON. NEW-60: audit shows raw tool ids, JSON and hashes. KEY-3: Escape does
  not hide the agent browser. Polish: five panels in four sizes.

**Approvals, notices, shell styles** (`ui/src/approvals`, `ui/src/notify`,
`ui/src/shell`)

- A11Y-37 major: at 640x400 the approval toast covers the focused card.
- Minors: Y/N by physical key position (QWERTZ); early Y/N gives no word;
  placeholders at 3.1-4.1:1 (A11Y-39); forced-colours sidebar tab state;
  focus rings and target sizes vary; no visible mouse way to a terminal
  once a file is open (NEW-68).
