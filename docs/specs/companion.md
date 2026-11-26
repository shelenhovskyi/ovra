# Phone companion

Status: approved by the owner on 2026-11-25 (recommended options of
`docs/research/mobile-companion.md`, section 6). Architecture: ADR 0014.
Security code: every step that touches the listener, pairing, the CA or the
companion allowlist gets an independent security review before it lands.

## Steps

Each lands on its own, in this order.

1. **Keep awake** (`power.*`, `crates/power`): done first, no network.
2. **Companion clients in the core**: a client role and the allowlist.
3. **`crates/remote`**: listener, CA and leaf certificates, pairing,
   device keys, revoke; `remote.*` methods for the desktop.
4. **Settings > Phone** in the desktop UI: switch, QR code, pairing
   prompt, devices.
5. **`ui/src/mobile`**: the PWA.
6. **Web Push** without content.

## 1. Keep awake

`power.get`, `power.set_mode { mode }` (`off`, `while_agents_work`,
`always`), event `power.changed`. Every start is `off`. The core holds the
`KeepAwake` service while the mode is `always`, or while any session is
queued, preparing, starting, working or waiting for approval in
`while_agents_work`. Only idle system sleep is held off. A system that
cannot hold reports `problem` and keeps the mode. UI: a switch in the
Agents panel header (`off` and `while_agents_work`), all three modes in
Settings > Claude.

## 2. Companion clients

- `Core::connect_phone(device)` returns a `Client` whose requests go
  through `companion::allows(&Request)`; its transport skips events for
  which `Client::shows` (`companion::shows`) is false. Anything not listed
  answers `forbidden` ("this is not available from a phone"). `device` is
  the name kept at pairing, never one the phone sends later.
- The matches name every method of the domains a phone may use, so a new
  method there does not compile until someone decides; other domains are
  refused whole.
- Allowed requests: `info.get`, `projects.list`, `agents.list`,
  `agents.transcript`, `agents.capacity`, `agents.limit`,
  `permissions.pending`, `permissions.answer` (allow or deny, never with
  `always`), `agents.send`, `agents.interrupt`, `agents.stop`,
  `orchestration.list`, `review.list`, `power.get`, `power.set_mode`.
- Shown events: `info.ready`, `info.resync`, `agents.status`,
  `agents.message`, `agents.limit_changed`, `agents.billing`,
  `agents.sandbox`, `permissions.requested`, `permissions.resolved`,
  `orchestration.changed`, `review.changed`, `power.changed`. Never
  `terminals.*`, `fs.*`, `lsp.*`, `search.*`, `secrets.*`, `browser.*`,
  `remote.*`, text deltas, `orchestration.show`.
- Answers from a phone, and what its interrupts refuse, go to the audit
  log as `DecidedBy::Phone { device }`.

## 3. `crates/remote`

**Listener.** `RemoteAccess` service (trait in `services.rs`, fake in
`fakes.rs`), started by the core when the person turns it on. Binds to the
chosen private address and a port kept in the data directory (picked once
from 49152 to 65535). Each connection's first byte decides: TLS
(`0x16`) goes to rustls, anything else gets plain HTTP that only answers
`GET /ca.crt` (and `/ca.mobileconfig` for iOS) while a pairing window is
open, and otherwise redirects to `https://`.

**Certificates.** `rcgen`: an EC P-256 CA, 10 years, name-constrained to
`iPAddress` ranges of private networks and to `<host>.local`. Its key is
kept in the keychain through `Secrets` when available, else in
`<data>/remote/ca.key` with mode 0600; `<data>/remote` is a protected
path. Leaf: 30 days, SAN = bound address and `<host>.local`, renewed when
it has under 10 days or the address changes.

**HTTPS routes.**

- `GET /` and assets: the mobile build (`ui/dist/mobile`), strict CSP
  (`default-src 'self'; connect-src 'self' wss://<host>:<port>`).
- `POST /pair` `{secret, key, name}`: `key` is the phone's P-256 public
  key (SPKI, base64url). Checks the secret in constant time, spends it,
  emits `remote.pairing` with a 4-digit code
  (`SHA-256(secret || key)` mod 10000) and waits up to 2 minutes for the
  person. Answers `{device, code}` or `403`.
- `GET /ws`: `Origin` must be `https://<host>:<port>`, `Host` must match.
  The server sends `{"challenge": <32 random bytes, base64url>}`; the
  phone answers `{"device", "signature"}`, an ECDSA P-256 / SHA-256
  signature over `ovra-companion-v1\n<origin>\n<challenge>`. Then frames
  as in `crates/transport`, through a companion client. The server closes
  the socket after 15 minutes; the phone reconnects and signs again.
- Rate limits: 10 pairing attempts and 30 handshakes a minute per address.

**Desktop methods (`remote.*`).** `remote.status`, `remote.enable
{address?}`, `remote.disable`, `remote.start_pairing` (returns the URL for
the QR code; 5 minutes), `remote.answer_pairing {id, allow}`,
`remote.devices`, `remote.forget {device}`; events `remote.changed`,
`remote.pairing`. Devices: id, name, public key, paired at, last seen; at
most 5; not seen for 30 days are forgotten.

## 4. Settings > Phone

A switch ("Phone access on this network"), the address it listens on,
**Add a phone** (QR code, the two steps: install the certificate, then
open the app), the pairing prompt ("iPhone wants to control Ovra. Code
4821. Allow / Deny"), the device list with Forget. While the listener is
on, the title bar shows a small "Phone access on" item.

## 5. Mobile PWA

`ui/src/mobile/` is its own Vite entry with its own manifest and service
worker, reusing `lib/transport.ts`, `lib/protocol/` and Focus's view model
(`ui/src/focus/model.ts`, `away.svelte.ts`; `docs/specs/focus-mode.md`). Screens: Home
(Needs you, Working, keep-awake switch), Approval (Allow, Deny with
reasons, armed after 500 ms), Agent (compact transcript, reply, Stop),
Task (workers, review items read-only), Computer (keep awake, agents
against the cap, Forget this phone). e2e on a phone viewport against
headless mode with a fake device key.

## 6. Web Push

VAPID keys per install; subscriptions stored per device; messages carry
only a kind ("approval", "review", "failed") and an opaque id, encrypted
per RFC 8291. Opt-in from a tap on the phone.

## Not in this spec

Always allow, Keep/Discard, starting or resuming agents from the phone,
use away from home other than through the person's own Tailscale.
