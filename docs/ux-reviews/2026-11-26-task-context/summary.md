# Task context (@ menu, chips, Ask agent): persona round

Date: 2026-11-26. Personas: newcomer, keyboard power user, security reviewer.
Feature: `docs/specs/task-context.md`. Reports in this folder.

## Fixed

- NEW-1 Ask agent on the hover card does nothing: The action goes on the diagnostic itself (the card finds it by identity).
- NEW-2, KEY-4 `@` shows nothing in a fresh project: The menu offers the top level for an empty query and says "No matches" when nothing fits.
- KEY-5 Enter before the file list arrives sends a raw `@query`: Enter and Tab wait while the list loads ("Looking…").
- NEW-4, SEC-7, SEC-8 Include is unclear, Start looks ready: "Send anyway", labelled "Send X anyway"; Start and Send are disabled while a chip waits; "Agent settings file".
- NEW-5, KEY-2 held Backspace wipes the chips: A repeated Backspace does not remove chips; one press removes one.
- KEY-1 focus lost after Include or ×: Focus returns to the field.
- KEY-3 F8, then Ask agent sends a selection: A selection that is exactly a problem's range sends the problem.
- KEY-6 the menu outlives the caret: It follows arrow keys and closes on blur.
- KEY-7 chip changes not announced: Removals are announced in a polite live region.
- KEY-9 Ctrl+Enter picks an item: Modified keys never pick; what the box holds is not offered again.
- KEY-10 full-width `＠`: Accepted.
- NEW-6 Ask agent buried in the editor menu: Moved up, after the clipboard items.
- NEW-7 long names overflow the menu: Names and details are cut with an ellipsis.
- NEW-8, SEC-9 chip buttons look like default buttons: Quiet buttons; Send anyway in the warning colour.
- NEW-9 "Context sent" shows the agent's wording: It lists only the items.
- NEW-10, SEC-2 the chip does not say what goes: The tooltip shows the first lines of text; a terminal chip is named by its terminal.
- SEC-6 no trace of Send anyway: An included sensitive chip keeps an outline and says "Sent anyway" in its tooltip.
- SEC-1 a problem's source can add lines to Ovra's note: Names, titles, severity and source are put on one line, without backticks or quotes.
- SEC-3 common secret shapes pass: More names (`*.tfvars`, `service-account*`, `kubeconfig*`, `.aws/`, `.kube/`, ...) and text (`password=`, Stripe and Google keys, URLs with a password).
- SEC-5 a path starting with `~` reads as a home path: Named as `./~...`.

## Open

- SEC-4: a symlink in the project that points outside it is named like any
  file. Ovra never reads it; the agent's own reads go through the sandbox,
  which is where the boundary is enforced. A warning on the chip needs the
  fs service to resolve links (`fs-index`), so it is a follow-up there.
- NEW-3: the read-only file view has no selection source or Ask agent; it
  belongs to the viewers area.
- KEY-8: Ask agent as a palette command with a shortcut (a new command type)
  and keyboard access to the hover card's action (CodeMirror's lint card).
