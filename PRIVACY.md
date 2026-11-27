# Privacy

Ovra collects no telemetry, analytics or crash reports. It has no servers and
no accounts.

- **No phoning home.** Ovra makes no network requests of its own. Network
  traffic happens only when you cause it, for example a `git fetch` in the
  terminal or an agent session you start.
- **AI features use your own `claude` CLI.** When you start an agent session,
  Ovra runs the Claude Code CLI installed on your machine, signed in with your
  own account. Your prompts and the files the agent reads go to Anthropic under
  your account's terms and settings, exactly as when you use `claude` yourself.
  See [Anthropic's privacy policy](https://www.anthropic.com/legal/privacy).
  Ovra never sees or stores your Claude credentials.
- **Headless mode stays local.** `ovra-daemon` listens on `127.0.0.1` only and
  requires a per-launch token ([SECURITY.md](SECURITY.md)).
- **Local data.** Ovra keeps its project list and session data on your
  machine, and earlier versions of the files you open in its editor (local
  history: up to 50 per file, never for names like `.env*`, `*.pem` or `*.key`)
  in the app's own storage.

## Our commitment

If Ovra ever adds telemetry, it will be **opt-in**, off by default, documented
here with the exact data it sends, and announced in the changelog before the
release that adds it.
