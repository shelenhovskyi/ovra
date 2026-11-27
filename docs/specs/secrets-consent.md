# Keys agents use with consent

Status: proposal, waiting for the owner's choice. Builds on
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
  presets: GitHub, GitLab, OpenAI, Stripe, npm, Hugging Face, and
  "Other API". A preset fills host, header, format and the variable name,
  links to the page that makes a narrow token, and adds the host to the
  project's allowlist in the same step. The person only pastes the value.
  "Other API" shows the current fields under "Advanced".
- **Consent on first use.** When a session first sends a request that
  would carry a key, the proxy holds that request and the agent's
  approval card shows: "Agent wants to use your GitHub key (api.github.com)
  for this task." Buttons: **Allow for this task**, **Deny**. Under a
  disclosure: "Always allow in this project". The answer lasts for the
  session (resumes included); a worker asks on its own. Deny sends the
  request without the key, so the tool gets a plain 401 and the agent can
  say what it needs. A held request waits up to 5 minutes, then counts as
  Deny. The phone companion can Allow and Deny, never Always (as today).
- **Per-key setting**, chosen in Settings > Keys: Ask each task
  (default), Always in this project, Off.
- **Placeholder variables.** For a key with a variable name, the sandbox
  sets it to a placeholder, `ovra-key-<name>`, which is not secret. Tools
  start, send the placeholder, and the proxy replaces the whole header
  with the real one (it already drops the agent's own copy of the header).
  The placeholder is never sent upstream: a request to an unbound host
  that contains it is refused and logged, so a misrouted call fails
  loudly.
- **Basic format.** A format field: `bearer` (prefix + value, today),
  `basic` with a fixed user (`x-access-token` for GitHub). The proxy
  encodes after reading the value.
- **Shown where it happens.** The transcript gets a muted line "Used
  GitHub key on api.github.com" (once per session and host), the audit
  log keeps `SecretUsed`, and Settings > Keys shows "last used".
- **Answer redaction.** The proxy searches answers from bound hosts for
  the value (and its base64 form) and replaces it with `[key hidden]`, so
  an echoing endpoint cannot hand it back. Cheap, and a second wall.
- **One key, several projects.** Keys stay per project (the 4.D rule
  against an untrusted repository using a token). "Use in another
  project" copies the binding without the person pasting again, from
  Settings only.

Cost: about three days. Core: a held-request path from the netproxy to
`permissions.*` (a new request kind, `key`), the per-key setting, the
format field. Sandbox crates: set the placeholder variables. UI: the new
pane, presets, the card. Protocol change (`secrets.rs`,
`permissions.rs`) lands first in its own commit.

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

## Limits (same as 4.D)

- The agent cannot read the key but can use it for anything the key
  allows on its host while consent lasts. Presets link to the narrowest
  token type and the pane says so.
- Tools that pin their own roots or ignore the CA variables fail TLS to
  bound hosts. Go on macOS ignores `SSL_CERT_FILE`, so `gh` there needs a
  check when the macOS sandbox is verified (added to its checklist).

## Tests

- Core: consent held, allowed, denied, timed out, "always"; a worker asks
  on its own; resume keeps the answer. Fake netproxy, fake agents.
- netproxy: placeholder replaced, placeholder to an unbound host refused,
  basic format, answer redaction (plain and base64), held request
  released on answer.
- UI: presets fill fields and the allowlist, the card's buttons.
- e2e: add a GitHub preset key, an agent's request shows the card, Allow.
- 4.F: an attack that tries to read the key back through an echoing host
  and through the placeholder.

## Decision for the owner

1. A (recommended), B, or C.
2. Default per key: Ask each task (recommended) or Always in this project.
