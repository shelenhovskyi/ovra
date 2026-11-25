# Secrets broker (4.D)

Status: task 4.D. Reviewed by an independent reviewer agent in place of the
owner (owner's rule, 2026-11-24). Builds on `docs/specs/security.md`
(section "Secrets"). Part 1 (the keychain store and `secrets.*`) and part 2
(delivery through TLS for bound hosts, in `crates/netproxy`) are built. The
owner approved the CA bundle variables on 2026-11-25; the Linux sandbox
binds the bundle and sets them (4.A).

**Goal.** An agent can call an API that needs a token, such as GitHub's,
without the token ever entering its sandbox: not in its environment, its
files or its memory. The netproxy adds the token on the way out.

## What a secret is

A secret belongs to one project. It has:

| Field    | Meaning                                                                    |
| -------- | -------------------------------------------------------------------------- |
| `name`   | 1 to 64 of `A-Z a-z 0-9 _ . -`, starting with a letter or digit.           |
| `host`   | The one host it is sent to: a DNS name with a dot, optionally `:port`.     |
| `header` | The request header that carries it, for example `Authorization`.           |
| `prefix` | Text before the value in that header, for example `Bearer `. May be empty. |
| value    | Never returned by any protocol method; only `Secrets::value` reads it.     |

- **One project.** A token set for one project is never sent for another.
  The person sets it again for a second project. This keeps a repository
  the person does not trust from using a token meant for another one.
- **Exact host.** No wildcards and no IP addresses. `api.github.com` does
  not cover `uploads.github.com`.
- **Never `claude`'s own hosts.** Hosts under `anthropic.com`, `claude.ai`
  and `claude.com`, and the Bedrock (any `bedrock*` label under
  `amazonaws.com` or `amazonaws.com.cn`), Vertex AI and Azure AI Foundry
  endpoints, are refused,
  so the proxy never decrypts `claude`'s traffic (ADR 0006). The netproxy
  also refuses to bind any host on its base allowlist.
- **Refused headers.** `Host`, `Content-Length`, `Transfer-Encoding`,
  `Connection`, `Keep-Alive`, `Upgrade`, `TE`, `Trailer` and every
  `Proxy-*` header, because they change how the request is framed or
  routed.
- **One binding per host and header.** A second secret for the same host
  and header in the same project is refused (`conflict`).
- **Value.** 1 to 16 KiB (Windows' Credential Manager holds at most 2560
  bytes; a longer value is refused there with `bad_request`), no control characters (no CR, LF, NUL, tab or
  DEL), so it cannot split a header. The prefix follows the same rule.
  Header names are RFC 9110 tokens.
- **A binding moves only with a new value.** No method changes a secret's
  host, header or prefix without the person typing the value again, so a
  client that can reach the core cannot point an existing token at its own
  host.

## Storage

- **Values** live in the OS keychain only: Keychain Services on macOS,
  Credential Manager on Windows, the Secret Service (D-Bus) on Linux,
  through `keyring-core` and its platform stores. The item's service is
  `ovra` and its account is `<install id>/<project>/<name>/<generation>`:
  two Ovra data folders never share an item, and each write gets a new
  random generation, so a new value never sits under the old binding.
- **Names and bindings** live in `secrets.json` in Ovra's data folder,
  with the random install id and each secret's generation. It holds no
  values. The keychain cannot list items on every platform, so this file
  is the list. It is written atomically (a temporary file created 0600 on
  Unix, synced, then renamed). If it exists but cannot be read, every call
  fails with `unavailable`; Ovra never starts over with a new install id.
- **No fallback.** If the keychain cannot be reached (a Linux server
  without a Secret Service), `set` fails with `unavailable` and says why.
  Ovra never writes a value to a file.
- **Order.** `set` writes the new item, then the list, then deletes the
  old item. `remove` moves the item to a `pending_delete` list in
  `secrets.json` first, then deletes it from the keychain, then drops the
  pending entry. `set` names its new item as pending before writing it,
  and takes it off once the list names it. Pending deletes are retried
  after each change. A crash or a keychain error leaves at worst an item that the
  list still names, never a value sent to a host it was not set for, and
  never an item nothing can find. `set` and `remove` hold one lock, and
  the core holds its own lock across its checks, `set`, `remove` and
  removing a project. One process (the daemon or the app) uses a data
  folder at a time.
- **Removing a project** removes its secrets too. Project ids are never
  reused (they are random), so the UI could never list them again; leaving
  them would strand tokens in the keychain.
- The sandbox already hides the data folder and, on Linux,
  `/run/user/<uid>` (where D-Bus reaches the keyring), so an agent reaches
  neither (`docs/specs/security.md`).

## Contract

`Secrets` in `crates/core/src/services.rs`, implemented by
`ovra-secrets` (`KeychainSecrets`), fake `MemorySecrets`:

```rust
pub trait Secrets: Send + Sync + Debug {
    /// Whether values can be stored, and where.
    fn status(&self) -> SecretStoreStatus;
    /// The project's secrets without values, by name.
    fn list(&self, project: &ProjectId) -> Result<Vec<SecretInfo>>;
    /// Stores or replaces one. The core has checked the fields.
    fn set(&self, project: &ProjectId, secret: SecretInfo, value: SecretValue) -> Result<()>;
    /// `not_found` if there is none.
    fn remove(&self, project: &ProjectId, name: &str) -> Result<()>;
    /// Removes every secret of a project the person removed.
    fn remove_project(&self, project: &ProjectId) -> Result<()>;
    /// For the netproxy, and for the audit log's redaction (4.E). No
    /// protocol method reaches it.
    fn value(&self, project: &ProjectId, name: &str) -> Result<SecretValue>;
}
```

`SecretValue` is wiped from memory when dropped and prints as `<secret>` in `Debug`, so a logged request never
shows one, and its own `Deserialize` reports errors without quoting the
input (the transport answers bad frames with the parser's message). Calls may block on the keychain, so the core runs them on a
blocking thread.

## Protocol (`protocol/secrets.rs`)

- `secrets.list { project }` answers `secrets.list { store, secrets }`:
  `store` is `{ state: "available", name }` or `{ state: "unavailable", reason }`.
- `secrets.set { project, secret, value }` answers `ok`. The core checks
  every field first (`bad_request`, the message never quotes the value).
- `secrets.remove { project, name }` answers `ok`.
- `secrets.changed { project }` tells other windows to list again.

Only the person sets secrets, through the UI. No IDE tool and no agent
reaches `secrets.*`. In headless mode the existing access checks
(ADR 0003) guard these methods like every other.

## Who can read a value

The goal holds only for sessions the runtime reports as sandboxed with a
`Proxy` network. The netproxy adds secrets only for such sessions.

- An **unsandboxed** session (every session until 4.A/4.B) runs as the
  person, and any program the person runs can ask the keychain for Ovra's
  items (`secret-tool`, `security find-generic-password -s ovra`). The UI
  says so beside the secrets list while the project has unsandboxed
  sessions. This is not new exposure: the same holds for every token the
  person keeps in their keychain today.
- **Linux:** the sandbox hides `/run/user/<uid>`; 4.A also clears
  `DBUS_SESSION_BUS_ADDRESS` and gives the sandbox its own network
  namespace, so abstract D-Bus sockets are out of reach.
- **macOS:** 4.B gives Ovra's items an access list limited to Ovra's own
  binary.
- **Windows:** Credential Manager has no per-application access list; 5.A
  must keep agents away from it.
- 4.F adds a "read the keychain" attack.

## Delivery: TLS for bound hosts (part 2, in the netproxy)

`NetProxy::start` takes the project's secret bindings (never values) and
returns the CA bundle's path, which `ProfileNetwork::Proxy` carries to the
sandbox. The netproxy gets the `Secrets` service from `daemon` and `app`
(no new crate edge: both depend on `ovra-core` only) and reads a value
only when it adds it.

The netproxy (4.C) tunnels CONNECT without looking inside. For a host that
has a secret bound in the session's project, it must see the request to add
the header, so it terminates TLS for that host only:

1. **A CA per session.** When a session's proxy starts, the netproxy makes
   a new CA key pair in memory. The key never touches disk and dies with
   the proxy. The CA certificate carries critical X.509 name constraints
   that permit only the session's bound hosts and exclude every IP address
   (`0.0.0.0/0`, `::/0`), so even a stolen key cannot sign for another
   name. A dNSName constraint also permits subdomains; the proxy itself
   issues leaves for bound hosts only. Keys and cached values are zeroized
   when dropped. The CA is never added to the system's trust store.
   Bound hosts are fixed when the proxy starts: a secret set or removed
   mid-session applies to the next session. Binding a host does not add it
   to the allowlist; the person adds it there too.
2. **Trust inside the sandbox only.** The netproxy writes a CA bundle (the
   system roots plus this CA certificate, no key) to the session's private
   temporary folder. Every bundle the person's environment already names
   (`SSL_CERT_FILE`, `CURL_CA_BUNDLE`, `GIT_SSL_CAINFO`,
   `REQUESTS_CA_BUNDLE`, `NODE_EXTRA_CA_CERTS`) is merged into it, so
   corporate roots keep working. `wrap` points those variables at it.
   The sandbox also unsets `SSL_CERT_DIR`. The owner approved this change
   to the environment ADR 0006 describes on 2026-11-25.
3. **Only bound hosts.** For a CONNECT to a bound host and port, the proxy
   answers the TLS handshake with a leaf certificate for that name, valid
   for seven days and signed by the session CA. The agent must wait for the
   CONNECT answer before it starts TLS; bytes sent sooner are refused. Every other host is tunnelled as before,
   so `claude`'s own traffic to Anthropic is never inspected.
4. **Upstream as a normal client.** The proxy opens its own TLS connection
   to the real host and verifies it against the system roots plus the
   person's configured bundles (with `rustls`), through the chained
   upstream proxy if there is one, after the same address checks as any connection (no
   loopback, private or metadata addresses). A failed check refuses the
   connection and logs `NetworkRefused`.
5. **Parsing strictly.** Request smuggling would put the header into the
   body of the next request, where the agent can read it back (a gist, an
   issue). So the proxy parses each request with a strict HTTP/1.1 server
   parser (`hyper`) and writes a new request upstream; it never forwards
   raw bytes. It serves one request per tunnel and sends `Connection: close`
   upstream; whatever the agent sends after that request is dropped. It
   accepts only HTTP/1.1 with an origin-form target, and refuses: `Content-Length` together with
   `Transfer-Encoding`, more than one `Content-Length`, any
   `Transfer-Encoding` but `chunked`, folded header lines, bare LF line
   ends, `Upgrade` and `101` answers, `CONNECT` inside the tunnel, an
   absolute-form target for another authority, and a missing or repeated
   `Host` or a `Host` that names another host. 4.F adds a smuggling attack.
6. **Adding the header.** On each request the proxy removes every header of
   the bound name (case-insensitively) that the agent sent, adds
   `<header>: <prefix><value>` with the value from `Secrets::value`, and
   logs `SecretUsed { name, host }` once per connection. If `value` fails
   (removed, keychain error) it refuses the request rather than send it
   without the header. It never adds a secret to plain HTTP. It follows no
   redirects. HTTP/2 is not offered (ALPN `http/1.1` only) to keep the
   rewrite small.
7. **No inspection beyond that.** The proxy does not log bodies or other
   headers.

## Limits

- An agent cannot read the token, but it can use it: anything the token
  allows on its host, the agent can do. Bind tokens with the narrowest
  scope the service offers. The UI says so when a secret is set.
- A host that echoes request headers back (a debugging endpoint) would hand
  the value to the agent. Bind secrets only to real API hosts.
- A program in the sandbox that ignores the CA variables (its own pinned
  roots) fails TLS to bound hosts. It still cannot reach the value.
- Without a Secret Service on Linux, secrets are unavailable. That is by
  design: there is no plaintext fallback.

## Tests

- `ovra-secrets` runs against `keyring-core`'s mock store and a temporary
  data folder, never the real keychain.
- The core's checks and `secrets.*` run against `MemorySecrets`.
- Part 2 adds netproxy tests with a local TLS server and the fake sandbox;
  4.F adds an attack test that searches the sandbox for the value.

## Assumptions

- Secrets are per project, not shared between projects (safer; costs the
  person a second paste).
- Removing a project removes its secrets (they could never be listed
  again).
- The UI to set secrets is a follow-up task in its own UI area.
