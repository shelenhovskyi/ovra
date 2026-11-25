# Basic IDE features

Status: P0 done on 2026-11-25 (asked for by the owner that day). P1 and P2
are a backlog; each item lands as its own small task.

Ovra is agent-first, but a person still opens, reads, fixes and moves files
by hand between agent turns. When the everyday moves of VS Code, Zed,
JetBrains or Cursor are missing, the IDE feels broken, however good the agent
flow is. This page lists those moves, what Ovra has today and in which order
the gaps close.

Priorities:

- **P0**: missing it makes people leave the app for Finder or another
  editor. Do now.
- **P1**: expected within the first week of use. Next.
- **P2**: power-user comfort. Later, or when someone asks.

What Ovra does not take on: anything that competes with the agents
(inline AI completion, AI chat in the editor; ADR 0006), a plugin
marketplace (phase 6), and debugging (agents run code in the sandbox).

## Rules for every item

- Every file operation goes through the core (`fs.*`), with the path checked
  by `RelPath::parse`. Services refuse a path whose parent resolves outside
  the project through a symlink. Nothing in `.git` is created, moved or
  trashed from the UI; the project root cannot be renamed or trashed.
- Delete moves to the system trash (`fs.trash`), never `rm`. The person can
  restore from the trash; the UI says "Move to Trash".
- Changes the person makes count as the person's in direct-mode review,
  like a save does (`person_saved`).
- Menus follow `docs/specs/quiet-style.md`: short labels, shortcut on the
  right, no icons. Every menu item is also a command in the palette, and
  every menu opens from the keyboard (Shift+F10 or the Menu key).
- Shortcuts match on `KeyboardEvent.code`, so they work on any layout.

## Files and the file tree

| Feature                                                      | VS Code         | Zed | JetBrains  | Cursor | Ovra now | Priority |
| ------------------------------------------------------------ | --------------- | --- | ---------- | ------ | -------- | -------- |
| Context menu on files and folders                            | yes             | yes | yes        | yes    | no       | P0       |
| New file / new folder, inline name, `a/b/c.ts` makes parents | yes             | yes | yes        | yes    | no       | P0       |
| Rename in place (F2, Enter on macOS)                         | yes             | yes | Shift+F6   | yes    | no       | P0       |
| Delete to trash, with confirmation                           | yes             | yes | yes        | yes    | no       | P0       |
| Move by drag and drop in the tree                            | yes             | yes | yes        | yes    | no       | P0       |
| Duplicate                                                    | no (copy/paste) | yes | yes (Copy) | no     | no       | P0       |
| Copy path / copy relative path                               | yes             | yes | yes        | yes    | no       | P0       |
| Reveal in Finder / Explorer / file manager                   | yes             | yes | yes        | yes    | no       | P0       |
| Open tabs follow a rename or move; trashed files' tabs close | yes             | yes | yes        | yes    | no       | P0       |
| Cut / copy / paste of files in the tree                      | yes             | yes | yes        | yes    | no       | P1       |
| Multi-select (Shift, Mod click) for trash and move           | yes             | yes | yes        | yes    | no       | P1       |
| Undo the last file operation (Mod+Z in the tree)             | yes             | no  | yes        | yes    | no       | P1       |
| Drop files from Finder into the tree (copy in)               | yes             | yes | yes        | yes    | no       | P1       |
| Open in integrated terminal (here)                           | yes             | yes | yes        | yes    | no       | P1       |
| Find in folder (search scoped to it)                         | yes             | yes | yes        | yes    | no       | P1       |
| Collapse all, reveal active file                             | yes             | yes | yes        | yes    | yes      | done     |
| Git status colours, changed-only filter                      | yes             | yes | yes        | yes    | yes      | done     |
| File nesting (`.ts` + `.test.ts`), compact folders           | yes             | no  | yes        | yes    | no       | P2       |
| New file from a template                                     | no              | no  | yes        | no     | no       | P2       |
| Compare two files                                            | yes             | no  | yes        | yes    | no       | P2       |

## Tabs

The tab strip belongs to the simplify-UI work (preview tabs, pinning,
remembered View or Edit per file type). This list only adds the menu.

| Feature                                                                           | VS Code | Zed | JetBrains | Cursor | Ovra now                   | Priority |
| --------------------------------------------------------------------------------- | ------- | --- | --------- | ------ | -------------------------- | -------- |
| Tab context menu: close, close others, close to the right, close saved, close all | yes     | yes | yes       | yes    | no                         | P0       |
| Tab menu: copy path, reveal in tree, reveal in file manager                       | yes     | yes | yes       | yes    | no                         | P0       |
| Preview tab, pinned tabs                                                          | yes     | yes | yes       | yes    | in progress (other thread) | P0       |
| Reopen closed tab (Mod+Shift+T)                                                   | yes     | yes | yes       | yes    | yes                        | done     |
| Save all (Mod+Alt+S)                                                              | yes     | yes | yes       | yes    | yes (Mod+Shift+S)          | done     |
| Drag tabs to reorder                                                              | yes     | yes | yes       | yes    | no                         | P1       |
| Split editor (side by side)                                                       | yes     | yes | yes       | yes    | no                         | P1       |
| Recent files (Mod+E in JetBrains)                                                 | yes     | yes | yes       | yes    | no                         | P1       |

## Editor

| Feature                                                                      | VS Code | Zed | JetBrains | Cursor | Ovra now                    | Priority               |
| ---------------------------------------------------------------------------- | ------- | --- | --------- | ------ | --------------------------- | ---------------------- |
| Editor context menu: cut, copy, paste, find, go to definition, rename symbol | yes     | yes | yes       | yes    | no                          | P0                     |
| Find and replace in file                                                     | yes     | yes | yes       | yes    | yes (Mod+F), not in palette | P0 (make discoverable) |
| Go to line (Ctrl+G)                                                          | yes     | yes | yes       | yes    | hidden key only             | P0 (make discoverable) |
| Undo / redo, multi-cursor, bracket matching                                  | yes     | yes | yes       | yes    | yes                         | done                   |
| Go to definition, references, rename (LSP)                                   | yes     | yes | yes       | yes    | yes (ADR 0007)              | done                   |
| Diagnostics (LSP)                                                            | yes     | yes | yes       | yes    | yes                         | done                   |
| Go to symbol in file (Mod+Shift+O)                                           | yes     | yes | yes       | yes    | yes                         | done                   |
| Format document (LSP), format on save                                        | yes     | yes | yes       | yes    | format yes, on save no      | P1 (on save)           |
| Breadcrumbs (path and symbol)                                                | yes     | yes | yes       | yes    | no                          | P1                     |
| Word wrap toggle                                                             | yes     | yes | yes       | yes    | yes (Alt+Z)                 | done                   |
| Auto save (after delay / on focus change)                                    | yes     | yes | yes       | yes    | no                          | P1                     |
| Font size zoom (Mod+= / Mod+-)                                               | yes     | yes | yes       | yes    | yes                         | done                   |
| Comment line (Mod+/)                                                         | yes     | yes | yes       | yes    | check                       | P1                     |
| Code folding                                                                 | yes     | yes | yes       | yes    | no                          | P1                     |
| Change language mode of a file                                               | yes     | yes | yes       | yes    | no                          | P2                     |
| Indentation and line-ending switch                                           | yes     | yes | yes       | yes    | no                          | P2                     |
| Minimap                                                                      | yes     | yes | no        | yes    | no                          | P2                     |
| Sticky scroll                                                                | yes     | yes | yes       | yes    | no                          | P2                     |
| Snippets                                                                     | yes     | yes | yes       | yes    | no                          | P2                     |
| Go to symbol in project                                                      | yes     | yes | yes       | yes    | no                          | P2                     |

## Workbench

| Feature                                         | VS Code | Zed | JetBrains | Cursor | Ovra now               | Priority |
| ----------------------------------------------- | ------- | --- | --------- | ------ | ---------------------- | -------- |
| Command palette, go to file                     | yes     | yes | yes       | yes    | yes                    | done     |
| Search in files with replace                    | yes     | yes | yes       | yes    | search yes, replace no | P1       |
| Settings UI (theme, font, tab size, auto save)  | yes     | yes | yes       | yes    | no                     | P1       |
| Keyboard shortcuts list and rebinding           | yes     | yes | yes       | yes    | list in palette        | P2       |
| Status bar: line and column, language, encoding | yes     | yes | yes       | yes    | no                     | P1       |
| Problems list (all diagnostics)                 | yes     | yes | yes       | yes    | no                     | P2       |
| Zen / focus mode                                | yes     | yes | yes       | yes    | panels fold            | P2       |

## P0 plan

Built in parallel, files and menus first:

1. Core: `fs.create`, `fs.rename` (also move), `fs.copy` (duplicate),
   `fs.trash`, `open.reveal`. Real implementation in `crates/fs-index`
   (trash through the `trash` crate, MIT) and `crates/opener`.
2. One shared context menu (`ui/src/menu/`), keyboard reachable.
3. File tree: context menu, inline new and rename, trash with confirmation,
   duplicate, copy path, reveal, drag and drop to move; open tabs follow.
4. Editor: context menu; Find, Replace and Go to line in the palette.
5. Tabs: context menu, once the tab work in the simplify-UI thread lands.
