# Trust card round (2026-11-26)

Scope: the simplified project-settings trust card (`ui/src/agents/setup/ProjectTrust.svelte`).
Personas: newcomer, security-reviewer.

## Fixed in the same change

- NEW-1, SEC-3: the lists no longer have a scroll box of their own (only the
  panel scrolls) and are scrolled into view when opened; while a decision is
  pending every group is open, so Trust comes after all of it.
- NEW-2: the untrusted lead adds "Trust only a project whose authors you
  trust; Claude works without it."
- NEW-3: the broad group is labelled "Broad rules: allowed without asking you".
- SEC-4 (part): the folded ⚠ chip has a tooltip ("2 warnings: see Details").
- Heading once trusted: "Project's own settings are on" (not Ovra's settings).

## Follow-ups (pre-existing, outside the card)

- SEC-1: a changed env value (or hook script body) is re-asked with no ⚠ and
  no file named; the core lists env names only (`crates/agent-runtime/src/project.rs`).
- SEC-2: the core's `broad()` treats out-of-project paths and network or
  publish commands (`Edit(~/.bashrc)`, `Bash(curl:*)`, `Bash(git push:*)`) as narrow.
- SEC-5: risky environment variables are named, their values never shown.
- NEW: "Review and trust…" reads like a small expand link, not the next step; no "Not now".
