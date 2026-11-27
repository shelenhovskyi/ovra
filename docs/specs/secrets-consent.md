# Keys agents use with consent

Status: option A approved by the owner on 2026-11-27, with Always allow
on the consent card. Builds on
`docs/specs/secrets.md` (4.D, built) and `docs/specs/security.md`.

**Goal.** The person can let an agent use a key (a GitHub token, an API
key) for a task, says yes when it is used, and the agent never sees the
value. Setting a key up takes a beginner under a minute.

## What exists today

4.D already keeps the value away from the agent: the value lives in the
OS keychain, the netproxy terminates TLS for the key's one host and adds
the header on the way out, and the audit log records `SecretUsed`. What it
lacks for this goal:

1. **No consent.** A bound key is added to every request to its host from
   every sandboxed session in the project. The person never says yes.
2. **Hard to set up.** The form sits in the Network pane and asks for a
   host, a header and a prefix. The host must also be added to the
   allowlist by hand, or the key is never used.
3. **Command-line tools refuse to start.** `gh`, the OpenAI and Stripe
   SDKs and most others check for `GH_TOKEN`, `OPENAI_API_KEY`, ... before
   they send anything. Without the variable they never reach the proxy.
4. **Only `<prefix><value>` headers.** `git` over HTTPS and package
   registries send `Authorization: Basic base64(user:token)`.
5. **macOS has no sandbox yet** (`VERIFIED = false`), and the proxy adds
   keys only to sandboxed sessions, so on a Mac keys do nothing until the
   sandbox is verified there. Windows waits for 5.A the same way.

## Options

### A. Proxy adds the key, with consent and presets (recommended)

Keep 4.D's delivery and add what is missing:

- **Settings > Keys.** Its own pane, out of Network. "Add key" offers
  presets and "Other API". A preset fills host, header, format and the variable name,
  links to the page that makes a narrow token, and adds the host to the
  project's allowlist in the same step. Presets: GitHub (`GH_TOKEN`),
  GitLab (`GITLAB_TOKEN`), Stripe (`STRIPE_API_KEY`), npm (`NPM_TOKEN`),
  Hugging Face (`HF_TOKEN`). OpenAI is left out: its hosts are Codex's
  own, which no key is sent to. The person only pastes the value.
  "Other API" shows the current fields under "Advanced".
- **Consent on first use.** When a session first sends a request that
  would carry a key, the proxy holds that request and the agent's
  approval card shows: "Agent wants to use your GitHub key (api.github.com)
  for this task." Buttons: **Allow for this task**, **Deny**, and a
  quieter **Always allow** (owner, 2026-11-27) with one honest line:
  "Every agent in this project will use this key without asking. You can
  turn this off in Settings > Keys." The answer lasts for the
  session (resumes included); a worker asks on its own. Deny sends the
  request without the key, so the tool gets a plain 401 and the agent can
  say what it needs. A held request waits up to 5 minutes, then counts as
  Deny. The phone companion can Allow and Deny, never Always (as today).
- **Per-key setting**, chosen in Settings > Keys: Ask each task
  (default), Always allow, Off. These are project policy rules for the
  tool `UseKey` with the key's name as the match, as `Push` rules decide
  Ovra's pushes: `auto` is Always allow, `deny` is Off, no rule (or `ask`)
  asks. "Always allow" on the card adds the `auto` rule, the way it does
  for tool calls, and like every `auto` rule it counts only in a
  sandboxed session with a limited network (the only sessions keys reach
  anyway). An agent tool that happens to be called `UseKey` never matches
  these rules.
- **Fresh rules each time.** The proxy asks the core on every request it
  would add a key to; the core reads the policy then, so turning Always
  allow off in Settings applies at once. A session's own answer (Allow
  for this task, Deny) is kept with the session in memory.
- **Placeholder variables.** For a key with a variable name, the sandbox
  sets it to a placeholder, `ovra-key-<name>`, which is not secret. Tools
  start and send the placeholder; the proxy drops the agent's copy of the
  key's header and adds the real one. The variable name must end in
  `_TOKEN`, `_KEY`, `_SECRET` or `_PAT`, and may not start with a prefix
  an agent's own login reads (`ANTHROPIC_`, `CLAUDE_`, `OPENAI_`,
  `GEMINI_`, `GOOGLE_`, `AWS_`, `AZURE_`, `CODEX_`, `OVRA_`), so a
  placeholder never replaces an agent's own credentials.
- **Answer redaction.** The proxy asks bound hosts for an uncompressed
  answer (`Accept-Encoding: identity`) and masks every copy of the value
  in it with `*`, keeping its length, so an echoing endpoint cannot hand
  it back.
- **Keys stay per project** (the 4.D rule against an untrusted repository
  using a token).

- **Details that keep consent exact.** The proxy asks only for a host
  the allowlist lets through. A session's answer belongs to the key's
  binding (name, host, header, prefix): a key pointed elsewhere asks
  again, and the core drops its `UseKey` rules when the key is removed or
  rebound. A deny from interrupting the turn or stopping the session is
  not kept. A question asked while the session is idle (a background
  process) leaves it idle. Requests that waited for an unanswered question
  go without the key rather than each ask in turn.

Left for later, each its own small task: a "Used GitHub key" line in the
transcript and "last used" in Settings (the audit log has `SecretUsed`
now), "Use in another project", a `basic` format (no preset needs it:
GitHub, GitLab, Stripe, npm and Hugging Face all take a bearer token,
and pushes go through the person's own `gh`), masking base64 copies; tests for a worker asking on its own, a resumed
session keeping its answer, two keys on one host with one allowed, a 4.F
attack that reads a key back through an echoing host, and an e2e run from
preset to card to Allow; a mark on the core's own `UseKey` and `Push`
requests, so an agent tool of that name cannot borrow their card's
wording (it gets no key and no Always allow); a test across the provider
crates that no variable an agent's login reads passes the variable check.

Cost: about three days. Protocol: `SecretInfo.variable`, the variables
in `ProfileNetwork::Proxy`, `KeyConsent` on `NetReporter`; lands first in
its own commit. Core: `UseKey` rules and asking through the existing
`permissions.*` requests (tool `UseKey`, input `{ key, host }`). netproxy:
consent before adding, identity encoding, masking. Sandbox crates: set the
variables. UI: the new pane, presets, the card.

### B. A broker runs commands for the agent

An IDE tool `run_with_key(command)` runs `gh` or `curl` outside the
sandbox with the key in its environment, after the person allows it.

- Works for keys that are not headers (SSH, database passwords).
- But the command runs outside the sandbox, with arguments the agent
  writes: `gh api` can post the token anywhere `gh` can reach, `curl` can
  print its environment. Making that safe means a parser per command.
  It is a security loosening and a new MCP tool, and the agent sees the
  answer, which may echo the key. Not recommended.

### C. Inject the value into the sandbox's environment

Simplest, and every tool works. But the agent can print the variable, so
it sees the key. That is exactly what the owner wants to avoid. Rejected.

## What stays out

- `claude`'s own login is never brokered or injected (ADR 0006); the
  macOS sandbox keeps `claude` on its own file.
- Keys that are not HTTP headers (SSH keys, database URLs, query-string
  keys) are out of scope for A. A later spec can add a query-parameter
  format if a real service needs it.
- Pushes and pull requests keep going through the person's own `gh`
  outside the sandbox (ADR 0010); a GitHub key here is for the agent's
  own API calls (issues, CI logs).

## Limits

- Masking catches whole copies of the value. It misses values under 8
  bytes, a host that compresses although asked not to, and partial,
  escaped or encoded copies (a Range answer, JSON escapes, base64).

## Limits from 4.D

- The agent cannot read the key but can use it for anything the key
  allows on its host while consent lasts. Presets link to the narrowest
  token type and the pane says so.
- Tools that pin their own roots or ignore the CA variables fail TLS to
  bound hosts. Go on macOS ignores `SSL_CERT_FILE`, so `gh` there needs a
  check when the macOS sandbox is verified (added to its checklist).

## Tests

- Core: consent held, allowed, denied, timed out, "always"; a worker asks
  on its own; resume keeps the answer. Fake netproxy, fake agents.
- netproxy: no key without consent, the agent's placeholder header
  replaced, answer masking across reads, identity encoding.
- UI: presets fill fields and the allowlist, the card's buttons.
- e2e: add a GitHub preset key, an agent's request shows the card, Allow.
- 4.F: an attack that tries to read the key back through an echoing
  host.

## Decision

The owner chose A on 2026-11-27 and asked for Always allow on the card:
quieter than Allow for this task, with the line about every agent in the
project, revocable in Settings > Keys, never from the phone.
