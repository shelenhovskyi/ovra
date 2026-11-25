import { expect, describe as group, it } from "vitest";
import type { AuditEvent } from "../lib/protocol";
import { describe, shows, when } from "./describe";
import { bash, entry } from "./fake-audit";

const line = (event: AuditEvent) => describe(entry(1, event));

group("describe", () => {
  it("says who decided a tool call and what it was", () => {
    expect(line(bash(true))).toEqual({
      title: "Allowed Bash",
      detail: "Allowed by a person.",
      input: '{"command":"ls"}',
      tone: "plain",
    });
    expect(line(bash(false))).toMatchObject({
      title: "Refused Bash",
      detail: "Refused by the project's policy (Bash(rm)).",
      tone: "refused",
    });
    const write = line({
      kind: "tool_decided",
      tool: "Write",
      input: { kind: "file", path: "src/a.rs", sha256: "abc" },
      allowed: false,
      by: { kind: "session_ended" },
    });
    expect(write.input).toBe("src/a.rs\nSHA-256 of what it writes: abc");
    expect(write.detail).toBe("Refused because the session stopped or ended.");
    expect(
      line({
        kind: "tool_decided",
        tool: "mcp__ovra__spawn_agent",
        input: { kind: "json", text: "{}" },
        allowed: true,
        by: { kind: "role" },
      }).detail,
    ).toBe("Allowed by Ovra's rules for its own tools.");
  });

  it("warns about sessions that ran without a sandbox", () => {
    const bare = line({ kind: "session_started", mode: "direct", trusted: false, sandbox: null });
    expect(bare).toMatchObject({ tone: "warn" });
    expect(bare.detail).toBe("In the main checkout, without the project's agent settings, no sandbox.");
    const boxed = line({
      kind: "session_started",
      mode: "isolated",
      trusted: true,
      sandbox: { name: "bwrap", network: "proxy", gaps: [] },
    });
    expect(boxed.detail).toBe("In its own worktree, with the project's agent settings, sandbox bwrap, network proxy.");
    expect(boxed.tone).toBe("plain");
  });

  it("describes settings, reviews, the network and secrets", () => {
    expect(line({ kind: "trust_changed", trusted: true, fingerprint: "9f2c" }).title).toBe("Project settings trusted");
    expect(line({ kind: "trust_changed", trusted: false, fingerprint: null }).detail).toBe(
      "Agents no longer load them.",
    );
    const policy = line({
      kind: "policy_changed",
      policy: { rules: [{ tool: "Bash", match: "npm test", action: "auto" }] },
    });
    expect([policy.detail, policy.input]).toEqual(["1 rule.", "auto Bash npm test"]);
    expect(line({ kind: "policy_changed", policy: { rules: [] } }).detail).toBe("No rules: every tool call asks.");
    expect(line({ kind: "review_decided", review: "r1", action: "partly_approved" }).title).toBe(
      "Review partly approved",
    );
    expect(line({ kind: "network_refused", host: "evil.test", reason: "not allowed" })).toMatchObject({
      title: "Network refused: evil.test",
      tone: "refused",
    });
    expect(line({ kind: "secret_used", name: "GH", host: "api.github.com" }).detail).toBe(
      "Sent to api.github.com. The log never keeps its value.",
    );
  });

  it("describes servers and loopback grants", () => {
    const by = { kind: "agent", session: "s1" } as const;
    expect(line({ kind: "server_started", server: "srv1", name: "web", generation: 2, by })).toMatchObject({
      title: "Server web started",
      detail: "Run 2, started by session s1.",
    });
    expect(line({ kind: "server_stopped", server: "srv1", name: "web", generation: 2, code: null }).detail).toBe(
      "Run 2, ended by a signal.",
    );
    expect(line({ kind: "loopback_granted", server: "srv1", port: 5173, generation: 2 }).title).toBe(
      "Server reachable on localhost:5173",
    );
    const revoked = entry(4, { kind: "loopback_revoked", server: "srv1", port: 5173, generation: 2 });
    expect(shows("servers", false, revoked)).toBe(true);
  });

  it("describes language servers, and warns about one without a sandbox", () => {
    const started = { kind: "language_server_started", server: "l1", name: "rust-analyzer", language: "rust" } as const;
    expect(line({ ...started, sandbox: "bubblewrap" })).toMatchObject({
      title: "Language server rust-analyzer started",
      detail: "For rust files, in the sandbox (bubblewrap).",
      tone: "plain",
    });
    expect(line({ ...started, sandbox: null }).tone).toBe("warn");
    const stopped = { kind: "language_server_stopped", server: "l1", name: "rust-analyzer", language: "rust" } as const;
    expect(line({ ...stopped, reason: null }).title).toBe("Language server rust-analyzer stopped");
    expect(line({ ...stopped, reason: "the server exited with code 2" })).toMatchObject({
      title: "Language server rust-analyzer failed",
      detail: "the server exited with code 2",
    });
    expect(shows("servers", false, entry(5, { ...stopped, reason: null }))).toBe(true);
  });

  it("describes browser navigations and counts refusals", () => {
    const refused = entry(5, { kind: "browser_navigation", origin: "https://x.test", allowed: false, by: "no_answer" });
    expect(describe(refused)).toMatchObject({
      title: "Browser may not open https://x.test",
      detail: "The session's browser was refused because nobody answered in time.",
      tone: "refused",
    });
    expect(shows("network", true, refused)).toBe(true);
    const allowed = line({ kind: "browser_navigation", origin: "https://x.test", allowed: true, by: "person" });
    expect(allowed.detail).toBe("The session's browser was allowed by a person.");
  });

  it("filters by kind and by refusals", () => {
    const allowed = entry(1, bash(true));
    const refused = entry(2, bash(false));
    const trust = entry(3, { kind: "trust_changed", trusted: true, fingerprint: null });
    expect([allowed, refused, trust].filter((e) => shows("tools", false, e))).toEqual([allowed, refused]);
    expect([allowed, refused, trust].filter((e) => shows("all", true, e))).toEqual([refused]);
    expect([allowed, refused, trust].filter((e) => shows("settings", false, e))).toEqual([trust]);
  });

  it("writes local times without a locale", () => {
    expect(when(new Date(2026, 0, 2, 3, 4, 5).getTime())).toBe("2026-01-02 03:04:05");
  });
});
