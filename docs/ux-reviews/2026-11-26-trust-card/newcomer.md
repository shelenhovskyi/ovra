# Nadia, the newcomer: trust card (simplified)

Build: working tree 2026-11-26, headless, Playwright Chromium, 1280x800.
Scope: the project trust card in the Agents panel (`ui/src/agents/setup/ProjectTrust.svelte`). Fixture: a temp project with 7 allow rules (5 broad), 3 hooks, 2 plugins and 1 env var. Scripts: `target/ux-trust/trust.ts`, `target/ux-trust/newcomer2.ts`; shots in `target/ux-trust/newcomer/` and `target/ux-trust/newcomer2/`.

## Summary

Nadia sees the two amber warnings at once and understands "Claude will act without asking" and "will run commands on my computer". She does not learn whether she should trust, or what happens if she doesn't. After she clicks "Review and trust…", the lists open almost entirely below the fold, and Trust ends up 320 px off-screen in two nested scroll areas. Once the project is trusted, the folded line is tidy and well aligned, and the warnings come back word for word in Details.

## Findings

### NEW-1 major: "Review and trust…" opens the lists off-screen, and Trust is hidden behind two scroll areas

- **Area:** ui/src/agents/setup
- **Steps:** 1. Open the project. 2. Click "Review and trust…".
- **Expected:** The lists scroll into view, and it is clear that a Trust button waits at the end.
- **Actual:** Only the first group's heading shows above the task box (`6-lists-before-scroll`). The Trust button is at y=1120 in an 800 px window. `.details` (320/632 px) scrolls inside the panel (490/780 px), and neither has a visible scrollbar. When she scrolls with the wheel to find Trust, the inner list scrolls as well, so the first group ("Whole tools or any command…") scrolls by without being seen (`7-lists-scrolled`). The rule "nobody trusts what they did not see" breaks on the very group that matters most.
- **Why it matters:** She thinks nothing happened, or she trusts after seeing only plugins and env vars.
- **Suggested fix:** Scroll the opened lists into view, as the `projects.review_trust` command already does. Drop the inner `max-height` scroll and let the panel scroll alone. Optionally show a sticky "Trust project" footer once the lists have been seen.

### NEW-2 major: the card names the risk but not the choice

- **Area:** ui/src/agents/setup
- **Steps:** 1. Open a project with settings and read the card (`1-untrusted`).
- **Expected:** One plain line such as "Trust only if you know who wrote these settings. Without trust, Claude still works with your own settings", plus a visible way to say no.
- **Actual:** The lead covers only what trusting does. There is no "Not now" or Close, so the card stays in the panel for good. Nothing says that Claude works fine untrusted.
- **Why it matters:** A beginner reads the amber text as "something is broken until I click Trust". The quickest way to make the card go away is to trust.
- **Suggested fix:** Add a "without trusting" sentence and a quiet "Not now" that folds the card to one line ("Project settings off · Review").

### NEW-3 minor: the "broad" list contradicts its own entries

- **Area:** ui/src/agents/setup (texts.ts)
- **Steps:** Open the lists (`7-lists-scrolled`, the session.log "lists" note).
- **Expected:** The group "Whole tools or any command allowed without asking you" holds whole tools or any command.
- **Actual:** It lists `Bash(python3 tool/app.py:*)` and `mcp__xcode__RenderPreview`, which are one script and one tool. The warning says "5 tools and commands", and the name `mcp__…` means nothing to her.
- **Suggested fix:** Use a label that matches the entries ("Commands and tools Claude may run without asking you"), and show MCP entries as "xcode tool: RenderPreview".

### NEW-4 minor: the folded "⚠ 2" chip has no visible word or tooltip

- **Area:** ui/src/agents/setup
- **Steps:** Trust the project, then hover the chip (`3-trusted-folded`, `10-chip-hover`).
- **Expected:** "2 warnings", or a tooltip that names them.
- **Actual:** The word "warnings" is visually hidden, and the button has no title. The heading "Project settings are on" is grey 13 px text. The chip contrast is fine (#7F5300 on a soft amber background, about 5:1).
- **Suggested fix:** Show "⚠ 2 warnings", or set a title with the two warning lines.

### NEW-5 minor: the card has two different names

- **Area:** ui/src/agents/setup
- **Steps:** Compare `1-untrusted` with `3-trusted-folded`.
- **Actual:** Before trust the heading is "This project has its own Claude settings". After trust it is "Project settings are on", which reads like Ovra's own settings (the gear icon). Nadia does not connect the two.
- **Suggested fix:** Keep the word "Claude": "Project's Claude settings are on".

### NEW-6 polish: the path to Trust looks like a minor disclosure

- **Area:** ui/src/agents/setup
- **Actual:** On a card full of amber text, the only way to trust is a grey chevron link, "Review and trust…" (`1-untrusted`). It is findable, but it does not look like the next step.
- **Suggested fix:** Use a secondary-button style with the chevron.

## What worked well

- Both warnings sit in front of her as plain sentences, marked with ⚠ as well as colour, and the trusted card repeats them in the present tense (`4-trusted-open`).
- The Hide and Details toggle stays in the same spot in the heading row, open or folded, and lines up with the chip.
- An early Trust click explains itself: "Ready in a moment — read the card first." (`8-early-press`).
- "Stop trusting" is one click and brings back the full untrusted card with its warnings (`12-after-stop`).
