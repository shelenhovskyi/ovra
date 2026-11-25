# Ovra on your phone (LAN companion)

Status: research, 2026-11-25. Nothing here is approved; the decisions at the
end need the owner. This opens a new network door into Ovra, so it deviates
from ADR 0003 (loopback only) and needs a new ADR and a spec before any code.

The owner wants a simple, very convenient phone app (a PWA) to follow and
steer Ovra over the home network: see agents and tasks, answer approvals,
and keep the computer awake while agents work.

## 1. What others do

| Tool                                               | How the phone reaches the computer                                                                                                                                            | What the phone can do                                                                                             | Notes for Ovra                                                                                                          |
| -------------------------------------------------- | ----------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ----------------------------------------------------------------------------------------------------------------- | ----------------------------------------------------------------------------------------------------------------------- |
| Claude Code Remote Control (`claude --rc`)         | Through Anthropic's servers. The local process only makes outbound HTTPS; no inbound port. Session found by name in the Claude app, or by a QR code in the terminal.          | Full chat with the local session, approvals. One connection at a time. Computer and process must stay on.         | Proof that "computer stays on" is the main pain. Relay means no LAN setup, but a vendor server sees the transcript.     |
| OpenAI Codex in the ChatGPT app (preview, 2026-05) | A vendor relay keeps trusted machines reachable without exposing them to the internet.                                                                                        | All threads, review output, diffs, terminal output, screenshots, approve commands, change models, start new work. | Same shape: phone as a remote for a local agent, approvals as the core action.                                          |
| Cursor web and iOS app                             | Cloud agents through cursor.com/agents (installable as a PWA on iOS); a Remote Control mode for agents on the user's computer.                                                | Start, steer and review agents. Deliberately no editor, terminal or file browser.                                 | Good scope line: direct and review, do not edit.                                                                        |
| Happy / Happier (open source, MIT)                 | A relay server (hosted or self-hosted). Phone and CLI pair by QR code; messages are end-to-end encrypted with keys that never leave the devices (TweetNaCl).                  | Chat with Claude Code and Codex sessions, approvals, push notifications.                                          | QR pairing with device keys is the right model. The relay is the part Ovra does not want by default.                    |
| VS Code tunnels / Remote Tunnels                   | Microsoft dev tunnels relay, signed in with GitHub or Microsoft.                                                                                                              | Full VS Code in the browser.                                                                                      | Full IDE on a phone is not what people want; account-based relay is a big dependency.                                   |
| Plex (`*.plex.direct`)                             | Direct LAN connection over HTTPS. Each server gets a real wildcard certificate for `*.<hash>.plex.direct`; names like `192-168-1-5.<hash>.plex.direct` resolve to the LAN IP. | -                                                                                                                 | The cleanest way to real HTTPS in a LAN, but it needs a vendor DNS zone and a certificate service. Ovra has no servers. |

Pattern: every vendor product uses a relay it runs. Nobody ships a pure LAN
mode, because browsers make LAN HTTPS hard (section 3.3). Ovra has no
servers and a promise to keep code local, so LAN-first is the right fit, with
a documented path for people who already run Tailscale.

## 2. Mobile UX

Principle: the phone is a remote, not an IDE. It answers "does anything need
me?" in one glance, and every action is one tap with no typing. No editor,
terminal or file browser (same line as Cursor's mobile app).

### Screens

1. **Home ("Needs you" + "Working")**
   - Header: computer name, connection dot, and the **Keep awake** switch
     with its state ("Awake while agents run", "Plugged in, 64%").
   - "Needs you": approvals, questions from agents, review items waiting.
     Newest first, with the project name and how long it has waited.
   - "Working": one card per running agent or task: name, project, status
     word (Thinking, Running a command, Waiting), last line of output,
     elapsed time, workers done/total for a task.
   - Empty state: "Nothing needs you. 2 agents working."
2. **Approval** (full screen from a card or a notification)
   - What the agent wants in plain words, the command or path in monospace,
     the folder, and a risk line where it applies ("Outside the project
     folder", "Session is not sandboxed", "New network host").
   - Two large buttons in the thumb zone: **Allow** and **Deny**. Deny opens
     optional quick reasons ("Do something else", "Ask me on the computer").
   - Buttons arm after 500 ms, like the desktop cards, so a tap from the
     previous screen never approves. No swipe-to-approve.
   - No "Always allow" (section 4.3); a line says "Always allow is on the
     computer".
3. **Agent**
   - Compact transcript (messages and tool calls collapsed to one line
     each), a reply box with dictation from the system keyboard, **Stop**.
4. **Task**
   - Coordinator status and its workers, each with status and a link to its
     agent screen; review items listed read-only with their diff summary
     (files, +/- lines). Keep and Discard stay on the computer in the MVP.
5. **Computer** (from the header)
   - Keep awake mode, power and battery, CPU and RAM (`resources.sample`),
     running agents against the cap, where usage is billed (ADR 0006 rule).
   - **Stop all agents** (red, confirm).
   - This phone: name, paired since, **Forget this phone**.

### Behaviour

- Live over one WebSocket; reconnects quietly; when the computer cannot be
  reached: "Can't reach MacBook. Same Wi-Fi?" with the last known state
  greyed out.
- Notifications (section 3.4) open straight to the approval or agent.
- Light and dark from the system; touch targets of at least 44 pt; text
  sizes follow the system.
- English only, like the desktop UI.

### Other useful features (beyond the MVP)

- "Let the computer sleep when all agents are done" (keep awake until idle).
- Notification when an agent waits longer than N minutes, or fails.
- Reply templates ("Continue", "Run the tests", "Explain first").
- Start a new agent or task from a short prompt and a project, within the
  agent cap (an explicit person's action, as ADR 0006 asks).
- Resume a stopped session.
- Screenshots from the agent browser (ADR 0009) and dev server status.
- Read-only diff of a review item, then Keep/Discard once that is approved.
- Away from home: through the person's own Tailscale (section 3.3).

## 3. Technical design

### 3.1 Shape

```
 Phone (Safari / Chrome, installed PWA)
      |  HTTPS + WSS, LAN only, device key auth
      v
 crates/remote (ovra-remote): rustls listener, pairing, device store,
      |  serves ui-mobile build, Web Push sender
      |  frames over the same transport, client role = Companion
      v
 crates/core: method allowlist per client role (enforced here, not in the UI)

 crates/power (ovra-power): KeepAwake trait impl per OS
```

- **Where it runs.** Today the Tauri app has no network listener (IPC only)
  and the daemon binds 127.0.0.1. The companion adds one more listener,
  started only when the person turns it on, in both `app` and `daemon`,
  through a new `RemoteAccess` service so `app` stays wiring only.
- **Same frames, smaller surface.** The phone speaks the same JSON frames as
  the UI. Each connection carries a client role; the core keeps an explicit
  allowlist of methods and events for `Companion` (section 4.3) and refuses
  everything else with `forbidden`. The check lives in the core so a bug in
  the mobile UI cannot widen it. Events are filtered the same way (no
  terminal output, no file contents beyond what an approval shows).
- **Mobile UI.** A second Vite entry, `ui/src/mobile/`, reusing
  `lib/transport.ts` and `lib/protocol/`. Small bundle, its own manifest and
  service worker. It never loads the desktop shell.
- **Discovery.** The QR code carries the address. Optional mDNS
  (`_ovra._tcp`, name `<computer>.local`) so the phone finds the computer
  again after a DHCP change.
- **New crates and dependencies** (all permissive): `rustls` and
  `tokio-rustls` (Apache-2.0/MIT/ISC), `rcgen` for certificates (MIT/Apache),
  `qrcode` (MIT/Apache), `web-push` or a small VAPID + RFC 8291 sender
  (MIT/Apache), `keepawake` (MIT) or direct OS calls, `mdns-sd`
  (Apache/MIT, optional). `scripts/crate-graph.json` gets `remote` and
  `power`.

### 3.2 Pairing with a QR code

1. On the computer: Settings > Phone > **Add a phone**. Ovra starts the
   listener (if off), opens a pairing window of 5 minutes, and shows a QR
   code with `https://<host>:<port>/pair#<secret>`. The secret is 128 random
   bits, single use, and sits in the URL fragment so it never reaches logs.
2. First time only: install the certificate (section 3.3). The QR screen
   walks through it.
3. The phone opens the page, makes a **non-extractable** WebCrypto P-256
   key pair (kept in IndexedDB), and sends the secret, its public key and a
   device name ("iPhone").
4. **Confirm on the computer.** Ovra shows "iPhone wants to control Ovra.
   Code 48 21" and the phone shows the same code (derived from the secret
   and the device public key). Nothing is paired until the person clicks
   **Allow** on the computer. A second phone that scanned the same QR gets
   nothing: the secret is spent.
5. The computer stores the device: id, name, public key, paired at, last
   seen. The pairing window closes.

**Sessions.** Each connection starts with a challenge that the phone signs
with its device key. The server answers with an access token valid for 15
minutes and bound to that connection; renewal needs a new signature. No
long-lived bearer secret exists on the phone, and the key cannot be copied
out of the browser.

**Revoke.** Forget a device on the computer (or on the phone itself): its
key is deleted and its open connections close at once. Devices not seen for
30 days expire. At most 5 devices. Turning the companion off closes all
connections and the port.

### 3.3 TLS in a LAN

Browsers only allow service workers, installing a PWA, Web Push and
WebCrypto on a secure context. `http://192.168.x.y` is not one, and a
self-signed certificate with a warning page does not count either (service
worker registration fails on certificate errors). So the phone must trust
the certificate. Options:

| Option                                                                      | Works offline in LAN    | Setup for the person                                                                                                                                                       | Infrastructure                        | Verdict                                                   |
| --------------------------------------------------------------------------- | ----------------------- | -------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ------------------------------------- | --------------------------------------------------------- |
| A. Ovra-made local CA, installed on the phone once                          | Yes                     | iOS: download profile, install, then turn on full trust in Settings > General > About > Certificate Trust Settings. Android: install CA in settings. About 1 minute, once. | None                                  | **Recommended default**                                   |
| B. The person's Tailscale (`tailscale serve`, real `*.ts.net` certificates) | Needs Tailscale on both | Already done for people who use it; also works away from home.                                                                                                             | Tailscale                             | **Supported advanced option**                             |
| C. Vendor DNS + per-install certificates (Plex model)                       | Yes (after issue)       | None                                                                                                                                                                       | Ovra DNS zone, ACME service, a domain | Later, only if Ovra ever runs servers                     |
| D. Plain HTTP, app-level encryption (Happy model without relay)             | Yes                     | None                                                                                                                                                                       | None                                  | Rejected: no PWA, no push, page can be swapped on the LAN |

Details for A:

- One CA per Ovra install, EC P-256, created on first use. The CA carries
  X.509 **name constraints** limiting it to Ovra's names (`.local` host name
  of this computer, a `home.arpa` name) and private IP ranges, so even a
  stolen CA key cannot impersonate other sites. Chrome enforces name
  constraints; iOS behaviour must be tested on a device before we rely on it
  (not confirmed in research).
- A short-lived leaf certificate (30 days) for the current LAN IP and host
  name is re-issued automatically when the IP changes; the phone keeps
  trusting the CA, nothing to reinstall.
- The CA private key lives in the OS keychain (`crates/secrets`), never in a
  plain file; its folder is on the protected path list so agents cannot read
  it. Removing the phone feature offers to delete the CA.
- The CA download is the one plain-HTTP step. It is served only while a
  pairing window is open, and the computer shows the CA fingerprint so it
  can be compared on the phone's profile screen. This is the weakest point
  of the design (an active attacker on the LAN at exactly that minute);
  section 4 lists it.

For B, Ovra binds loopback and the person runs `tailscale serve`; Ovra only
needs the `*.ts.net` origin on its allowed list. Documented, not automated.

### 3.4 Push notifications

- Web Push from an installed PWA works on Android Chrome and on iOS 16.4+
  only when the app is added to the Home Screen; permission must be asked
  from a tap.
- Push always goes through the browser vendor's service (Apple, Google,
  Mozilla). So the computer makes **outbound** HTTPS calls to that service,
  signed with a VAPID key; nothing inbound. Payloads are encrypted for the
  phone (RFC 8291), so the push service sees only timing and size.
- Ovra sends **no content** in a push: "Ovra: an agent needs you" or "Task
  ready for review", plus an opaque id. The app fetches details over the LAN
  when opened. A locked phone never shows commands or code.
- Push is optional and off until the person allows notifications on the
  phone. Without it the app updates live while open, and the desktop keeps
  its own notifications.
- Away from home the push still arrives; opening it says "Connect to your
  home Wi-Fi" (or works through Tailscale).

### 3.5 Keep the computer awake

A `KeepAwake` trait in `crates/core/src/services.rs` with a fake, and a real
implementation in `crates/power`. It holds one assertion with a reason
string, prevents **idle system sleep** only (the display may sleep), and
releases it when Ovra quits or crashes (the OS ties each assertion to the
process).

| OS      | API                                                                                                                                                                                     | Check it with            | Limits                                                                                   |
| ------- | --------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ------------------------ | ---------------------------------------------------------------------------------------- |
| macOS   | `IOPMAssertionCreateWithName(kIOPMAssertionTypePreventUserIdleSystemSleep, ...)`, released with `IOPMAssertionRelease` (what `caffeinate -i` does)                                      | `pmset -g assertions`    | Closing the lid still sleeps unless on power with an external display. Say so in the UI. |
| Windows | `PowerCreateRequest` + `PowerSetRequest(PowerRequestSystemRequired)` (reason shows in `powercfg /requests`); `SetThreadExecutionState(ES_CONTINUOUS \| ES_SYSTEM_REQUIRED)` as fallback | `powercfg /requests`     | Modern Standby laptops on battery may ignore it; lid close follows power settings.       |
| Linux   | logind `Inhibit("idle:sleep", "Ovra", reason, "block")` over D-Bus (what `systemd-inhibit` does); hold the returned fd; desktop portal `org.freedesktop.portal.Inhibit` inside Flatpak  | `systemd-inhibit --list` | Without logind (some distros, containers) report "not available" rather than pretend.    |

The `keepawake` crate (MIT) wraps exactly these calls; using it is simpler
than three FFI modules, but it also inhibits the screensaver on Linux by
default, so check its options or call the APIs directly.

Modes, shown on both the desktop status bar and the phone header:

- **Off** (default).
- **While agents run**: held while any session is running or waiting for a
  person, released 2 minutes after the last one ends. Recommended when the
  person turns the switch on.
- **Always**, until turned off or Ovra quits.

Battery: on battery below 20% Ovra drops to Off and tells the phone.

## 4. Threat model and safe defaults

### 4.1 What we protect

The agents can run commands and change files, so anything that can answer an
approval as the person can run code on their computer. The companion must
never be an easier way in than sitting at the keyboard.

### 4.2 Attackers and answers

| Threat                                                                     | Answer                                                                                                                                                                                                         |
| -------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| Another device on the same Wi-Fi (guest, café, a compromised IoT box)      | Off by default. When on, every request needs a paired device key; unpaired clients get only the pairing page, and only during a pairing window. Rate limits on pairing and handshakes.                         |
| Public or untrusted network                                                | Bind only to one chosen private interface (RFC 1918, IPv6 ULA, link-local, or `100.64/10` for Tailscale); never `0.0.0.0` by default. Turn the companion off when the network changes; ask again.              |
| Somebody scans or photographs the QR code                                  | Secret is single use, 5 minutes, and pairing still needs a click on the computer with a matching code.                                                                                                         |
| Active attacker on the LAN during the first CA download                    | Served only in the pairing window; fingerprint shown on the computer; name-constrained CA limits the damage. Known residual risk, documented.                                                                  |
| Lost or stolen phone                                                       | Key is non-extractable; access tokens live 15 minutes; Forget on the computer cuts it off at once. "Always allow", policies, Keep and settings are not reachable from the phone at all.                        |
| A web page in a browser on the phone or the computer (CSRF, DNS rebinding) | WebSocket checks `Origin` against the exact companion origin; HTTP checks `Host`; no cookies, auth only by signed challenge; strict CSP on the mobile UI.                                                      |
| A prompt-injected agent trying to approve itself                           | Sandboxed sessions cannot reach LAN or loopback (netproxy). Unsandboxed sessions can reach the port but have no device key; the CA key and device store are protected paths and the CA key is in the keychain. |
| Leaks through push                                                         | No content in push payloads; payloads encrypted; push is opt-in.                                                                                                                                               |
| A bug in the mobile UI showing or sending too much                         | Method and event allowlist enforced in the core per client role, with tests that every method not on the list is refused.                                                                                      |
| Forgotten open port                                                        | The desktop shows "Phone access on" in the status bar while listening; turns off after 7 days without any paired device connecting.                                                                            |

Every connect, pairing, revocation and every approval answered from a phone
goes to the audit log with the device name.

### 4.3 What the phone may do

| Action                                                                                                                         | MVP        | Why                                                                                          |
| ------------------------------------------------------------------------------------------------------------------------------ | ---------- | -------------------------------------------------------------------------------------------- |
| See projects, agents, tasks, status, usage, resources                                                                          | Yes        | Read only.                                                                                   |
| See transcripts (compact) and approval details                                                                                 | Yes        | Needed to decide.                                                                            |
| Allow / Deny one request                                                                                                       | Yes        | The main reason for the app; same information as the desktop card, risk lines shown.         |
| Reply to an agent, answer its question                                                                                         | Yes        | Steering is the second reason.                                                               |
| Stop one agent, Stop all                                                                                                       | Yes        | Only reduces what agents do.                                                                 |
| Keep awake switch                                                                                                              | Yes        | The owner asked for it.                                                                      |
| Forget this phone                                                                                                              | Yes        | Only reduces access.                                                                         |
| **Always allow**                                                                                                               | **No**     | Widens the project policy for good; a decision for the computer, with the full rule in view. |
| Keep / Discard review items                                                                                                    | No (later) | Keep changes the checkout, Discard deletes work. Read-only diff first.                       |
| Start or resume agents                                                                                                         | No (later) | Spends the person's limits; fine later with a confirm and within the cap.                    |
| Terminals, files, editor, policies, network allowlist, secrets, accounts, dev servers, pairing more devices, changing settings | Never      | Too much power for a small screen and a lost phone.                                          |

## 5. Plan

Each step is its own small task, landed in order (CLAUDE.md "Parallel
work").

1. ADR 0012 (LAN companion, amends ADR 0003) and `docs/specs/companion.md`
   for owner approval.
2. Stubs: `KeepAwake` and `RemoteAccess` traits with fakes;
   `crates/power`, `crates/remote`; crate graph entries.
3. Keep awake on all three OS plus the desktop status bar switch. Useful on
   its own, even without the phone.
4. Core: client roles and the Companion allowlist, with tests.
5. `crates/remote`: TLS with the local CA, pairing, device keys, revoke.
   Security review by an independent reviewer agent.
6. `ui/src/mobile`: Home, Approval, Agent, Task, Computer screens; e2e on a
   phone viewport against headless mode with a fake device.
7. Web Push (content-free).
8. Later: Tailscale docs, mDNS, start/resume agents, Keep/Discard.

Rough size: steps 2 to 7 are about a week of agent work in parallel threads.

## 6. Decisions for the owner

1. **Build it as a LAN-only companion, off by default** (new ADR 0012,
   deviates from ADR 0003). Recommended: yes.
2. **TLS**: Ovra-made, name-constrained local CA installed on the phone once
   (recommended), with Tailscale as a documented option. Alternative:
   Tailscale only (no CA, but needs Tailscale).
3. **Push through Apple/Google push services**, outbound only, no content in
   the message. Recommended: yes, opt-in on the phone.
4. **Phone actions in the MVP**: Allow/Deny, reply, Stop, Keep awake; no
   Always allow, no Keep/Discard, no starting agents. Recommended: yes.
5. **Keep awake default** when switched on: "While agents run", display may
   sleep, Off below 20% battery. Recommended: yes.
6. **Start with step 3 (keep awake on the desktop)** while the ADR and spec
   wait. Recommended: yes; it needs no new network access.

## Sources

- Claude Code Remote Control: https://code.claude.com/docs/en/remote-control
- Claude Code mobile: https://code.claude.com/docs/en/mobile
- Codex from anywhere (OpenAI, 2026-05-14): https://openai.com/index/work-with-codex-from-anywhere/
- Cursor web and mobile: https://cursor.com/docs/cloud-agent/mobile
- Happy (open source client): https://github.com/dvlkv/happy
- Plex HTTPS for all users (Filippo Valsorda): https://words.filippo.io/how-plex-is-doing-https-for-all-its-users/
- Web Push for web apps on iOS and iPadOS (WebKit): https://webkit.org/blog/13878/web-push-for-web-apps-on-ios-and-ipados/
- Sending web push in web apps and browsers (Apple): https://developer.apple.com/documentation/usernotifications/sending-web-push-notifications-in-web-apps-and-browsers
- Trust manually installed certificate profiles (Apple): https://support.apple.com/102390
- keepawake crate: https://docs.rs/crate/keepawake/latest
