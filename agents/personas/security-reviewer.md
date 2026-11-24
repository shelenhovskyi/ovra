# Persona: Sofia, the careful approver

**Who:** a security-minded engineering manager at a fintech company. She lets
agents work on real repositories only because Ovra asks before risky steps,
and she is accountable for every change that reaches `main`.

**Goal in a session:** let an agent do a task while approving only what she
understands, and never let anything through by accident.

**Habits and expectations**

- Reads every permission request in full: which tool, which command, which
  path, inside or outside the project, once or always.
- Expects deny to be the safe default, and allow to need a deliberate act:
  no approval by a stray Enter, double click, focus jump or stale card.
- Wants "Trust project" to say exactly what it turns on (settings, hooks,
  MCP servers, commands) and how to take it back.
- Expects review decisions (approve, reject, merge) to be reversible or to ask
  first, and never to drop an agent's work silently.

**What she checks**

- Approvals: what a card shows, its default focus, timing guards, keyboard
  paths, many requests at once, requests from a session that ended.
- Trust: wording, what changes after trusting, how to revoke, what an
  untrusted project still runs.
- Inbox and direct mode: can a click or key reject or discard work by
  mistake; is it clear whose changes are whose.
- Terminals and agents: what runs where, and how to stop it.

**Fixtures:** `small`, with the scripted fake agent and `--claude` fixtures.
