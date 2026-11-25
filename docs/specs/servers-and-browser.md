# Managed servers and the agent browser

Status: draft for the owner (2026-11-25). The owner asked for both features
and approved loosening the netproxy for them, only as far as this spec
says. Decisions: ADR 0008 (servers and loopback grants) and ADR 0009 (the
agent browser). Research: `research/` in the project files, "Local servers
and the embedded browser" (2026-11-25). Builds on `docs/specs/security.md`.

## Goals

- The person and agents run a project's dev servers (`pnpm dev`,
  `cargo run -p api`) as managed processes: start, stop, restart, status,
  logs, ready or not.
- Agents reach those servers, and only those, while they run.
- Agents drive a browser on those servers: open, click, type, read the
  console and network, take screenshots.
- The person watches the same browser, takes it over, and picks an element
  to show the agent what they mean.

Not goals for v1: servers or the browser outside Linux (other systems answer
`unavailable` until their sandbox exists), a browser the person uses for
their own browsing, `evaluate` or arbitrary page scripts for agents, file
upload, cookies or storage tools, downloading a browser.

## Servers

### Definitions

The person defines servers in `.ovra/servers.json` in the project:

```json
{
  "servers": [
    {
      "name": "web",
      "command": ["pnpm", "dev"],
      "cwd": "ui",
      "env": { "VITE_API": "http://localhost:8080" },
      "port": 5173,
      "ready": { "log": "ready in" }
    },
    { "name": "api", "command": ["cargo", "run", "-p", "api"], "port": 8080 }
  ]
}
```

- `name`: `^[a-z0-9][a-z0-9-]{0,31}$`, unique.
- `command`: a program and its arguments, run without a shell. `cwd` is
  relative to the work dir (checked by `RelPath::parse`), default `.`.
- `port`: what the server listens on in its pod, 1024 to 65535. Ovra sets
  `PORT` to it and `HOST=127.0.0.1`. Servers of one pod need distinct ports.
- `ready`: `log` (a substring of a line of output) and/or `http` (a path
  that must answer, through the bridge). Without `ready` a server is ready
  when its port accepts a connection.
- `.ovra/` becomes a protected path (`PROTECTED_IN_WORK_DIR`): an agent
  cannot add a command that a later start runs. An agent that needs a new
  server says so; the person edits the file. `servers.list` reads the file
  of the work dir it is asked about; a broken file is an error the UI shows.
- Agents start only defined servers. Ad-hoc commands stay what they are
  today: Bash in the agent's own sandbox.

### Pods

- A pod is one sandbox (Linux: one bubblewrap with its own network and PID
  namespaces) per work dir: the main checkout, or one worker's worktree.
  All servers of a pod share its loopback, so `web` reaches `api` at
  `localhost:8080`. Pods of different worktrees never see each other, so
  two worktrees both run `web` on 5173 without a clash.
- Its profile is the agent's for that work dir (`work_dir` writable except
  the protected paths, the same hidden paths), with its own netproxy for
  the way out: the project's allowlist, refusals reported for the pod.
  Servers run the project's code, not `claude`, so a pod gets none of
  `claude`'s state (`~/.claude` and `~/.claude.json` stay hidden: no
  login, transcripts or history) and the language servers' environment
  allowlist (`SERVER_ENV_KEPT`: no `ANTHROPIC_*`, `CLAUDE_*` or
  cloud-provider names; `docs/specs/security.md`).
- Inside the pod its supervisor (`ovra-servers` pod-init, the pod's only
  program) exposes each defined port: it listens on
  `<pod dir>/<port>.sock` and connects each connection to
  `127.0.0.1:<port>` inside the pod. `<pod dir>` is a private folder Ovra
  makes per pod, like the netproxy's socket folders, and is in the
  profile's `write`. The pods' folders sit in `ovra-srv-<random>` (0700)
  under `$XDG_RUNTIME_DIR` when it is in `/run` and short enough, else
  under `/var/tmp`: both are empty in the sandbox and no path servers
  write, since servers may make no entry in any folder on the way to
  their pod's (Linux: a Landlock layer pod-init puts on each server).
  Every socket path must stay under 100 bytes, or the pod does not start.
  If the servers' `TMPDIR` holds the pod's folder anyway, pod-init gives
  them a new `TMPDIR` of their own. Without Landlock that layer is
  missing, and the pods' status names it as a gap. Nothing in the pod listens on the host. On Linux the
  sandbox's part is what every session gets (its own PID and network
  namespaces, `--die-with-parent`; Landlock and seccomp allow TCP listeners
  inside the network namespace and Unix sockets in writable folders),
  plus a refusal to run a pod with an open network.
- A pod starts with its first server and ends with its last. Stop sends
  SIGTERM to the server's process group, SIGKILL after `grace_ms`
  (default 5000). Ending a pod kills its PID namespace, so no process
  survives it. A pod ends when Ovra quits. In the main checkout the
  session or person that started a server does not own it. A worker's
  pod does not outlive the worker: every server of its worktree is
  stopped when the worker's session ends, and before its review is
  approved or its files are copied over the person's (`review.take`), so
  nothing writes the worktree while its changes land. Both, and removing
  the worktree once its task ended, wait for the pod's process to exit
  too (`DevServers::pod_ended`); a pod that has not after 5 s refuses the
  step, which is tried again later.
- **Pod-init.** A pod's first process is Ovra itself in pod mode
  (`<ovra> --ovra-pod <pod dir>`, `ovra_servers::pod_init_if_asked`). It
  prints `ovra-pod: listening` once `<pod dir>/control.sock` listens,
  takes the first connection only, and speaks JSON lines: requests
  `spawn { id, argv, env, cwd, port }` and `signal { id, sig: term|kill }`;
  events `output { id, line }`, `listening { id, port }`,
  `exited { id, code }` and `failed { id, reason }` (it never ran).
  `listening` comes once `127.0.0.1:<port>` accepts a connection inside the
  pod, so the port check runs there. Each server is its own process group;
  when its main process ends, the rest of its group is killed. When the
  control connection closes, pod-init kills every server and exits.
- The pod's folder is writable inside the pod, so Ovra trusts no name in
  it. It holds the folder open and opens each socket there without
  following links (Linux: `O_PATH|O_NOFOLLOW`, then connects through
  `/proc/self/fd/N`); the file must be a socket, and its listener must run
  as the user. The listener of `control.sock` must be the process Ovra
  started or below it (Linux), and is pod-init from then on: only
  pod-init's own `<port>.sock` is ever connected (`SO_PEERCRED` pid).
- A `ready.http` check passes on any answer below 500. There is no ready
  timeout: a server stays `starting` until it is ready or ends.

### States

`starting` -> `ready` -> `exited { code }` or `failed { reason }`;
`stopping` while a stop waits. Each start gets a new `generation` (u64).
`restart` is stop then start, with a new generation.

### Logs

Each server keeps its last 1 MiB of output (stdout and stderr merged, ANSI
removed for agents, kept for the UI) in a ring buffer, numbered by line.
`servers.logs { since, limit, grep }` pages it; `grep` is a substring.

### The netproxy rule (the loosening)

A session's netproxy lets a connection reach a server when all hold:

1. The target is exactly `localhost:<P>` or `127.0.0.1:<P>`. No name is
   resolved. `[::1]`, `0.0.0.0`, `*.localhost` and every other loopback,
   private or link-local address stay refused as today.
2. The core holds a grant `(session, server, P, generation)`. The core
   makes it when a server of the session's work dir is `ready` and lists
   `P`, and only for sessions whose work dir is that pod's: a worker
   reaches its own worktree's servers, a direct session and the coordinator
   the main checkout's.
3. The server still runs with that generation. Exit, stop, restart and the
   end of the session drop the grant.
4. The proxy connects to that pod's `<P>.sock`, never to a host port.
5. Plain HTTP only: an absolute-form request (`GET http://localhost:5173/`)
   or a request through the forwarder below; `CONNECT` to loopback is
   refused. One request per connection, sent with `Connection: close`,
   except a WebSocket upgrade, which is relayed both ways after the first
   request head (Vite's HMR needs it).
6. The `Host` header must name the target (`localhost:P` or
   `127.0.0.1:P`). An `Origin` header, if present, must be
   `http://localhost:P` or `http://127.0.0.1:P` for a granted `P`;
   `Origin: null` is refused.

"Exactly" is literal: `localhost` in any case or `127.0.0.1`, a decimal
port or none (80), nothing else (`localhost.:P` stays refused). An
endpoint the user configured where Ovra runs (`ANTHROPIC_BASE_URL` on
`localhost`, the base list) keeps its own path; a grant never widens it.

**The forwarder.** Clients that skip the proxy for loopback (Chromium,
some HTTP libraries) connect to `localhost:P` directly. In the agent's
sandbox the helper listens on `127.0.0.1:P` for each port in the work
dir's `servers.json` when the session starts. Each connection goes to the
session's proxy socket with the line `OVRA-LOCAL <P>\r\n` first, then the
client's bytes; the proxy applies the same rule. A port defined after the
session started is reachable only through `HTTP_PROXY` until the session
restarts.

**Audit.** `ServerStarted { server, generation, by }`,
`ServerStopped { server, generation, code }`,
`LoopbackGranted { server, port, generation }` and `LoopbackRevoked` per
session, and refusals as `NetworkRefused` with reasons `no grant`,
`stale grant`, `connect to loopback`, `bad host`, `bad origin`.
Allowed loopback requests are not logged one by one (an agent's browser
makes hundreds); the grant is.

**The person's browser.** "Open in browser" on a ready server binds
`127.0.0.1:<P>` on the host if free, else a random port, forwarding to the
pod's socket, until the server stops. Only the person's action opens one;
no agent tool can. Host processes can then reach that server, like any dev
server the person starts themselves.

### Protocol (`servers.*`)

- `servers.list { project, work_dir? }` -> definitions and running servers.
- `servers.overview { project }` -> every server started in the project,
  in any work dir.
- `servers.start { project, name, work_dir? }`, `servers.stop { id, grace_ms? }`,
  `servers.restart { id }`.
- `servers.logs { id, since?, limit?, grep? }` -> lines and `next`.
- `servers.open { id }` -> the host URL (the person's browser, above).
- Events: `servers.status { id, status, generation }`,
  `servers.output { id, lines }` (rate-limited, for the UI).

`work_dir` names a session (`session: SessionId`) or the main checkout
(absent).

### Service

```rust
pub trait DevServers: Send + Sync + Debug {
    fn status(&self) -> SandboxStatus;
    /// Starts `line` (already built from the definition) in the pod of
    /// `pod.work_dir`, making the pod if needed.
    fn start(&self, pod: &PodSpec, server: ServerSpec, sink: ServerSink) -> Result<ServerHandle>;
    fn stop(&self, id: &ServerId, grace: Duration);
    /// A connection to `port` in the pod of `work_dir`, for the netproxy
    /// and the person's forward.
    fn connect(&self, work_dir: &Path, port: u16) -> BoxFuture<'_, Result<Conn>>;
}
```

The core keeps definitions, states, logs and grants; `ovra-servers` keeps
processes and pods and asks the `Sandbox` for the pod's command line
(`SandboxProfile` gains `expose: Vec<u16>` and `forward: Vec<u16>`, the
ports a helper exposes or forwards).

### IDE tools

| Tool             | Arguments                                  | Answer                                                                        |
| ---------------- | ------------------------------------------ | ----------------------------------------------------------------------------- |
| `server_list`    | none                                       | definitions and running servers of this work dir: id, name, status, port, url |
| `server_start`   | `name`, `wait_ms?` (<= 60000)              | id, status, url, the first log lines; waits until ready, failed or `wait_ms`  |
| `server_stop`    | `id`                                       | exit code                                                                     |
| `server_restart` | `id`, `wait_ms?`                           | the new status                                                                |
| `server_logs`    | `id`, `since?`, `limit?` (<= 500), `grep?` | lines, `next`                                                                 |

Every role gets them, for its own work dir only, also the read-only
coordinator: the servers any role starts are the ones the person
defined, and they run commands that write the work dir (a dev server's
build output, a watcher's cache) in their pod, as the person's own start
would. Logs are the server's
output, which project code writes: the tool answer wraps them as data
(below).

## Agent browser

### What it is

A Chromium the user has installed (`chromium`, `chromium-browser`,
`google-chrome` on `PATH`, or a path in the settings), never downloaded,
started per agent session:

- through the `Sandbox`, in a profile of its own: its fresh profile
  folder is its `work_dir`, it reads and writes nothing else (none of the
  project), exposes and forwards no ports, and has the session's netproxy
  (the agent's `Proxy { address, ca_bundle }`) as the only way out. A
  renderer exploit gets what that profile allows, since Chromium runs
  with `--no-sandbox`. The core passes `Browser::open` this profile with
  an empty `work_dir` placeholder; the service makes the folder and puts
  it there, so a service that does not cannot start a browser. Its
  `kind` is `ProfileKind::Browser`: unlike an agent's sandbox it gets
  none of `claude`'s state (`~/.claude`, `~/.claude.json`), none of the
  home folder's tools, settings or caches, and only the variables a pod
  keeps (no Claude or cloud-provider ones). Pods are `ProfileKind::Pod`,
  and only a pod may expose ports.
  `--proxy-server` points at the forwarded proxy port,
  `--proxy-bypass-list=<-loopback>` so loopback goes through it too, QUIC
  off, WebRTC limited to proxied TCP;
- controlled over `--remote-debugging-pipe`, never a TCP port;
- with a fresh profile in the session's temporary folder: no cookies, no
  extensions, no accounts. Ended with the session.

By default its netproxy rule is the session's: its servers, plus the
project's allowlist. A navigation to an origin that is not a granted
server asks the person once per origin and session.

### What the person sees

`ui/src/browser`: a panel per session showing the page as a screencast
(`Page.startScreencast`, JPEG, at most 10 frames a second, only while the
panel is visible). The person can take over: mouse and keys go to the page
through CDP input events. In Tauri and in headless mode it is the same
panel; nothing native.

**Picking.** "Pick" turns on an overlay drawn by Ovra over the canvas, not
inside the page, so a page can neither fake nor hide it. Hovering
highlights the element under the pointer (CDP hit test and box model);
Arrow Up and Down move to the parent and child; Shift+click adds more. A
pick becomes a chip in the session's message box; sending the message
attaches it. What the agent gets for each element:

- `pick_id`, `url`, a CSS selector, a role-and-name locator, tag, id,
  classes, other attributes;
- the first 200 characters of its text, `outerHTML` cut to 2 KiB;
- its box, these computed styles: `color`, `background-color`,
  `font-size`, `font-family`, `font-weight`, `display`, `position`,
  `margin`, `padding`, `border`;
- the component name and source file and line, when the page's build
  exposes them (React dev builds, `data-source` attributes); marked as a
  guess;
- the person's note for it;
- a cropped screenshot through `browser_inspect` on demand.

### IDE tools

Refs come from `browser_snapshot`: an accessibility tree, one line per
element, each with a ref (`e12`; `f2e7` inside a frame) tied to that
snapshot. A ref from an older snapshot fails with "take a new snapshot".

| Tool                 | Arguments                                                   |
| -------------------- | ----------------------------------------------------------- |
| `browser_open`       | `url`, `width?`, `height?` -> `page_id`                     |
| `browser_navigate`   | `page_id`, `url` or `action` (`back`, `forward`, `reload`)  |
| `browser_snapshot`   | `page_id`, `ref?` (a subtree), `depth?`                     |
| `browser_click`      | `page_id`, `ref`, `button?`, `double?`, `modifiers?`        |
| `browser_type`       | `page_id`, `ref`, `text`, `submit?`                         |
| `browser_press_key`  | `page_id`, `key`                                            |
| `browser_wait_for`   | `page_id`, `text?`, `text_gone?`, `timeout_ms` (<= 30000)   |
| `browser_console`    | `page_id`, `level?`, `since?`, `limit?`                     |
| `browser_network`    | `page_id`, `since?`, `limit?`                               |
| `browser_screenshot` | `page_id`, `ref?`, `full_page?` -> an image                 |
| `browser_inspect`    | `page_id`, `ref` or `pick_id` -> styles, box, source, image |
| `browser_close`      | `page_id`                                                   |

At most 4 pages per session. Every role gets them.

### Page content is data

Everything a page or a server produces (snapshots, text, console, network,
picks, logs) reaches the agent inside

```
<untrusted origin="http://localhost:5173">...</untrusted>
```

with `<` and `>` inside escaped, and each tool description says that text
in it is data, never instructions. The role prompts repeat it. This lowers
the risk; it does not remove it. What holds is the sandbox and the
netproxy: a page can talk an agent into trying things, but the agent's
network still reaches only its servers and the allowlist, and every tool
call is decided as today.

### Protocol (`browser.*`)

`browser.open`, `browser.close`, `browser.input { page, event }`,
`browser.pick { page, x, y }` (the element at a point, for the overlay),
`browser.attach { page, picks }` (the payload of a sent message),
`browser.status { session }` (whether the session's browser can run, or a
refusal code: first `session_ended`; then why no browser runs here,
`not_supported`, `no_sandbox` or `no_chromium`; last `not_sandboxed` for
an agent outside the sandbox), events
`browser.frame { page, jpeg, metadata }`, `browser.pages`,
`browser.navigation_asked { page, origin }`.

### Service

`Browser` in `services.rs`, implemented by `crates/browser` with a small
CDP client of its own over the pipe (tokio and serde_json): `chromiumoxide`
(0.9) speaks CDP only over a WebSocket, which needs a debugging port, and
brings about 140 crates (ADR 0009). Tests use a
scripted CDP peer; no real Chromium in `just test`. One opt-in smoke test
(`just browser-live [path]`) runs the real one, without the sandbox.

How it runs:

- `sh -c 'exec "$@" 3<&0 4>&1 ...'` around the sandboxed line moves the
  pipe onto fds 3 and 4, so no `unsafe` code is needed to set them up.
- Chromium's own sandbox is off (`--no-sandbox`): it needs new user
  namespaces or a setuid helper, and the Ovra sandbox around it forbids
  both (seccomp refuses `CLONE_NEWUSER`, Landlock sets `no_new_privs`).
  That sandbox holds every Chromium process instead, so a compromised
  renderer is as contained as the agent itself, not more.
- The browser needs the session's netproxy: a profile with an open
  network is refused. Popups (pages a page opens) are closed, since
  nothing watches their navigations. Pages report changes only once
  `open` has answered.
- Each document request (frames too) is held through the `Fetch` domain
  until `BrowserSink::navigation` answers for its origin; a refusal fails
  it as `ERR_BLOCKED_BY_CLIENT`, and the action answers `forbidden`.
- Picks are read in an isolated world of Ovra's, with the browser's own
  `getComputedStyle` and `getBoundingClientRect`, so a page cannot change
  what they say by replacing them. The source guess reads only
  `data-source` (`file:line[:column]`) and `data-component`: React's
  fields live in the page's own world.
- Refs number on across a page's snapshots, so a ref of an older one is
  never reused. Frames inside the page are not in the snapshot yet (no
  `f2e7` refs).
- Known limit: Chromium does not read `SSL_CERT_FILE`, so HTTPS to a host
  whose secrets the netproxy adds fails in the browser.

## Tasks

| Task | Area                                                                                                                                                             | After    |
| ---- | ---------------------------------------------------------------------------------------------------------------------------------------------------------------- | -------- |
| S.0  | protocol `servers.rs`, `browser.rs` (core and UI), `DevServers` and `Browser` traits, fakes, `.ovra` protected, stub crates `servers` and `browser`, crate graph | spec     |
| S.A  | `crates/servers`: processes, pods, logs, readiness, stop                                                                                                         | S.0      |
| S.B  | `crates/sandbox-linux`: pod profile, `expose` and `forward` in the helper                                                                                        | S.0      |
| S.C  | `crates/netproxy`: the loopback rule, `OVRA-LOCAL`, refusals                                                                                                     | S.0      |
| S.D  | core: definitions, `servers.*`, grants, audit                                                                                                                    | S.0      |
| S.E  | `crates/mcp-server`: `server_*`                                                                                                                                  | S.D      |
| S.F  | `ui/src/servers`: panel, logs, open in browser                                                                                                                   | S.0      |
| B.A  | `crates/browser`: launch, pipe, screencast, input, snapshot                                                                                                      | S.B      |
| B.B  | core: pages, picks, untrusted wrapping, navigation asks                                                                                                          | S.0      |
| B.C  | `crates/mcp-server`: `browser_*`                                                                                                                                 | B.B      |
| B.D  | `ui/src/browser`: canvas, input, pick overlay, chips                                                                                                             | S.0      |
| B.E  | e2e with fake services; UX persona pass                                                                                                                          | S.F, B.D |
