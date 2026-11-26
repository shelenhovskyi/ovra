# 0014. A phone companion over the LAN

- Status: accepted (owner, 2026-11-25, with the recommended option of every
  decision in `docs/research/mobile-companion.md`)
- Date: 2026-11-25

## Context

The owner wants to follow and steer agents from a phone on the home network:
see what runs, answer approvals, reply, stop, and keep the computer awake.
ADR 0003 keeps the core on loopback only. Vendor products (Claude Code
Remote Control, Codex, Cursor, Happy) relay through their own servers; Ovra
has none and keeps code on the person's computer.

## Decision

- A second listener, `crates/remote` (`ovra-remote`), serves a mobile PWA
  and a WebSocket for paired phones. It is **off by default**; the person
  turns it on in Settings > Phone. The choice is kept across restarts,
  and the listener turns itself off after 7 days with no paired phone
  connecting.
- It binds to **one private address** of this computer (RFC 1918, IPv6
  ULA, or `100.64.0.0/10` for Tailscale), never `0.0.0.0`, and refuses
  every other address.
- **TLS only** for everything but the CA download. Ovra makes its own CA
  per install, limited by X.509 name constraints to this computer's
  private addresses, and a short-lived leaf certificate for the bound
  address. The phone installs the CA once. The CA certificate is served
  over plain HTTP on the same port only while a pairing window is open.
- **Pairing** by QR code: a single-use secret valid for 5 minutes, a
  non-extractable WebCrypto P-256 key made on the phone, and a click on
  the computer that sees the same short code. No bearer tokens: each
  WebSocket starts with a challenge the phone signs, and lasts at most
  15 minutes before it must sign again. Forgetting a device closes its
  connections at once. At most 5 devices.
- A phone connection is a **companion client**: the core keeps an explicit
  allowlist of the methods and events it may use and refuses everything
  else. Allowed: reading projects, agents, tasks and approvals; Allow or
  Deny one request; reply; interrupt and stop; the keep-awake mode;
  forgetting itself. Never: Always allow, Keep/Discard, starting agents,
  terminals, files, policies, network, secrets, settings, pairing.
- **Push** (later step) goes through the browser vendor's push service,
  outbound only, with no content in the message.
- **Keep awake** is a `KeepAwake` service (`crates/power`) the core holds
  while agents work, off at every start.

## Consequences

- Ovra listens on the LAN while the companion is on; the desktop says so.
  A person on an untrusted network should leave it off.
- The CA download is the one unauthenticated step; an active attacker on
  the LAN during that minute could swap the certificate. Documented.
- The CA key is kept with Ovra's data in a protected path; agents in the
  sandbox cannot read it and cannot reach LAN addresses (netproxy).
- Every connect, pairing, forget and every answer from a phone goes to the
  audit log with the device name.
- Code in `crates/remote`, the companion allowlist and the CA are security
  code: an independent reviewer checks them before they land.
