# 0003. Headless mode is loopback-only and token-protected

- Status: accepted
- Date: 2026-11-24

## Context

Headless mode exposes the core, and later agents with file and shell access,
over HTTP. Any web page in the user's browser could try to reach it.

## Decision

- Bind to 127.0.0.1 only.
- Generate a random token per launch; WebSocket connections must present it.
- Reject WebSocket upgrades whose `Origin` is not the daemon's own origin or
  an explicitly allowed one.
- Static files carry no secrets and are served without the token.

## Consequences

Remote use needs an explicit tunnel (for example SSH port forwarding). Any
change to these rules is security code and needs human review.
