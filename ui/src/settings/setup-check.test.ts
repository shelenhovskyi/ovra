import { expect, it } from "vitest";
import type { AgentSetup, SetupReport } from "../lib/protocol";
import { checkRows, summary } from "./setup-check";

const setup = (state: AgentSetup["state"], more: Partial<AgentSetup> = {}): AgentSetup => ({
  path: null,
  executable: "/usr/local/bin/claude",
  version: "2.1.331",
  state,
  billing: null,
  api_key_in_env: false,
  ...more,
});

const fine: SetupReport = {
  agents: [
    { id: "claude", name: "Claude", setup: setup({ state: "ready" }) },
    { id: "gemini", name: "Gemini", setup: setup({ state: "not_found" }, { executable: null, version: null }) },
  ],
  git: { state: "checked", found: { installed: true, version: "2.47.1" } },
  gh: {
    state: "checked",
    found: { installed: true, version: "2.60.0", host: "github.com", login: "octo", signed_in: true },
  },
  sandbox: { state: "available", name: "bubblewrap", gaps: [] },
};

const short = (report: SetupReport) => checkRows(report).map((r) => `${r.level} ${r.id}: ${r.title}`);

it("says everything is here and leaves out agents that are not set up", () => {
  expect(short(fine)).toEqual([
    "ok agent:claude: Claude 2.1.331 is ready",
    "ok git: git 2.47.1 is installed",
    "ok sandbox: Agents run in a sandbox (bubblewrap)",
    "ok gh: gh 2.60.0 is signed in to github.com as octo",
  ]);
  expect(summary(checkRows(fine))).toBe("Everything Ovra needs is here.");
});

it("gives each problem one fix, with the commands that do it", () => {
  const broken: SetupReport = {
    agents: [
      { id: "claude", name: "Claude", setup: setup({ state: "signed_out" }) },
      { id: "gemini", name: "Gemini", setup: setup({ state: "not_found" }, { path: "/opt/gemini" }) },
    ],
    git: { state: "failed", message: "git --version failed: xcrun: error: invalid active developer path" },
    gh: {
      state: "checked",
      found: { installed: true, version: "2.60.0", host: "github.com", login: null, signed_in: false },
    },
    sandbox: { state: "unavailable", reason: "install bubblewrap (bwrap) to sandbox agents" },
  };
  const rows = checkRows(broken);
  expect(short(broken)).toEqual([
    "problem agent:claude: Sign in to Claude",
    "problem agent:gemini: Gemini was not found",
    "problem git: git needs Apple's developer tools",
    "problem sandbox: No sandbox: agents can change anything you can",
    "warning gh: gh is not signed in to github.com",
  ]);
  const by = (id: string) => rows.find((r) => r.id === id);
  expect(by("agent:claude")?.commands).toEqual([{ command: "claude auth login" }]);
  expect(by("agent:gemini")?.fix).toBe("There is no program at /opt/gemini. Choose it again in Settings > Agents.");
  expect(by("git")?.commands).toEqual([{ label: "macOS", command: "xcode-select --install" }]);
  expect(by("git")?.link?.url).toBe("https://git-scm.com/downloads");
  expect(by("sandbox")?.commands[0]).toEqual({ label: "Debian, Ubuntu", command: "sudo apt install bubblewrap" });
  expect(by("gh")?.commands).toEqual([{ command: "gh auth login --hostname github.com" }]);
  expect(summary(rows)).toBe("4 problems and 1 warning to look at.");
});

it("explains what was not or could not be checked", () => {
  const report: SetupReport = {
    agents: [
      {
        id: "claude",
        name: "Claude",
        setup: setup({ state: "not_found" }, { executable: null, version: null }),
      },
    ],
    git: { state: "checked", found: { installed: false, version: null } },
    gh: { state: "skipped" },
    sandbox: { state: "unavailable", reason: "the macOS sandbox has not been tested on a Mac yet" },
  };
  const rows = checkRows(report);
  expect(short(report)).toEqual([
    "problem agent:claude: Claude is not installed",
    "problem git: git is not installed",
    "warning sandbox: No sandbox: agents can change anything you can",
    "skipped gh: Open a project to check its GitHub sign-in",
  ]);
  // Claude's install commands for every OS, as its setup card shows them.
  expect(rows[0]?.commands.length).toBe(3);
  expect(summary(rows)).toBe("2 problems and 1 warning to look at.");

  const noOrigin: SetupReport = {
    ...report,
    gh: { state: "checked", found: { installed: true, version: "2.60.0", host: null, login: null, signed_in: false } },
  };
  expect(checkRows(noOrigin).at(-1)?.fix).toBe(
    "This project has no remote named origin, so its sign-in was not checked.",
  );
  const missing: SetupReport = {
    ...report,
    gh: {
      state: "checked",
      found: { installed: false, version: null, host: "github.com", login: null, signed_in: false },
    },
  };
  expect(checkRows(missing).at(-1)?.link?.url).toBe("https://cli.github.com");
});

it("warns when an API key pays instead of the Claude plan", () => {
  const report = {
    ...fine,
    agents: [{ id: "claude" as const, name: "Claude", setup: setup({ state: "ready" }, { api_key_in_env: true }) }],
  };
  const [claude] = checkRows(report);
  expect(claude?.level).toBe("warning");
  expect(claude?.fix).toContain("ANTHROPIC_API_KEY");
});
