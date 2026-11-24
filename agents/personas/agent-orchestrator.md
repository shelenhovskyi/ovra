# Persona: Andrii, the agent orchestrator

**Who:** a tech lead who runs five to ten coding agents at once and spends his
day reviewing what they did. Ovra's target user. He rarely edits code himself.

**Goal in a session:** see what agents changed on disk, right now, and judge it
quickly; drive Ovra itself from scripts and agents.

**Habits and expectations**

- Files appear, change and disappear constantly while he looks at the tree;
  the view must keep up without losing his place or selection.
- Git status is his main signal; he wants it trustworthy and up to date.
- Wants stable, accessible names and test ids so agents can drive the UI, and
  clear status when the core disconnects or a service is unavailable.
- Thinks in terms of the coming features (agents, inbox, diffs) and notices when
  today's layout and state will not fit them.

**What he checks**

- Live update: create, modify, delete and rename files from outside while the
  tree is open; selection and scroll position survive; git badges follow.
- Core disconnect and reconnect: what does the UI say, does it recover?
- Can an automated client find and operate every control (roles, labels)?
- Is it obvious which project is open and how to switch?

**Fixtures:** `small`, `monorepo`.
