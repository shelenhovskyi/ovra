// What Settings > Setup check says about each thing Ovra needs on this
// computer (`setup.check`): one row each, with one way to fix a problem.
// The UI does not know the OS, so a fix that differs by OS names each.

import { advice, billingText, INSTALL_COMMANDS } from "../agents/setup/claude-texts";
import type { AgentCheck, SetupReport } from "../lib/protocol";

export type CheckLevel = "ok" | "warning" | "problem" | "skipped";

export type CheckRow = {
  id: string;
  /** What is checked, such as `git`. */
  name: string;
  level: CheckLevel;
  /** One line on what was found. */
  title: string;
  /** The one thing to do about it; null when there is nothing to do. */
  fix: string | null;
  /** Commands to run in a terminal, each with what it is for when there are several. */
  commands: { label?: string; command: string }[];
  /** A web page that helps, opened through `open.url`. */
  link?: { label: string; url: string };
  /** The Settings pane where it is fixed. */
  pane?: "claude";
};

const GIT_URL = "https://git-scm.com/downloads";
const GH_URL = "https://cli.github.com";

/** The rows, in the order a new person fixes them: agents, git, sandbox, gh. */
export function checkRows(report: SetupReport): CheckRow[] {
  const agents = report.agents.filter((a) => a.id === "claude" || configured(a)).map(agentRow);
  return [...agents, gitRow(report), sandboxRow(report), ghRow(report)];
}

/** How many rows need the person, for the pane's summary. */
export function summary(rows: CheckRow[]): string {
  const problems = rows.filter((r) => r.level === "problem").length;
  const warnings = rows.filter((r) => r.level === "warning").length;
  if (problems === 0 && warnings === 0) return "Everything Ovra needs is here.";
  const parts = [
    problems > 0 ? `${problems} ${problems === 1 ? "problem" : "problems"}` : "",
    warnings > 0 ? `${warnings} ${warnings === 1 ? "warning" : "warnings"}` : "",
  ].filter(Boolean);
  return `${parts.join(" and ")} to look at.`;
}

/** Another provider is shown once the person chose its program or it is installed. */
function configured(agent: AgentCheck): boolean {
  return agent.setup.path !== null || agent.setup.state.state !== "not_found";
}

function agentRow(agent: AgentCheck): CheckRow {
  const { id, name, setup } = agent;
  const row = { id: `agent:${id}`, name, commands: [], pane: "claude" as const };
  if (id === "claude") {
    const tip = advice(setup, name);
    const warning = billingText(setup).warning;
    if (setup.state.state === "ready") {
      return { ...row, level: warning ? "warning" : "ok", title: tip.title, fix: warning };
    }
    const commands = tip.install ? INSTALL_COMMANDS : tip.command ? [{ command: tip.command }] : [];
    return { ...row, level: "problem", title: tip.title, fix: tip.detail, commands };
  }
  const version = setup.version ? ` ${setup.version}` : "";
  const state = setup.state;
  switch (state.state) {
    case "ready":
      return { ...row, level: "ok", title: `${name}${version} is ready`, fix: null };
    case "not_found":
      return {
        ...row,
        level: "problem",
        title: `${name} was not found`,
        fix: setup.path
          ? `There is no program at ${setup.path}. Choose it again in Settings > Agents.`
          : `Install ${name}, then check again.`,
      };
    case "outdated":
      return {
        ...row,
        level: "problem",
        title: `${name}${version} is too old`,
        fix: `Ovra needs version ${state.minimum} or newer. Update it, then check again.`,
      };
    case "too_new":
      return {
        ...row,
        level: "warning",
        title: `${name}${version} is newer than Ovra supports`,
        fix: `This build of Ovra supports versions below ${state.below}. Update Ovra.`,
      };
    case "signed_out":
      return {
        ...row,
        level: "problem",
        title: `Sign in to ${name}`,
        fix: `Start ${name} once in a terminal and sign in there, then check again. Ovra never sees your sign-in.`,
      };
    case "broken":
      return { ...row, level: "problem", title: `${name} could not be checked`, fix: state.message };
  }
}

function gitRow({ git }: SetupReport): CheckRow {
  const row = { id: "git", name: "git", commands: [] };
  const install = {
    fix: "Ovra uses git for reviews, worktrees and the Git panel. Install it, then check again.",
    commands: [{ label: "macOS", command: "xcode-select --install" }],
    link: { label: "Download git", url: GIT_URL },
  };
  if (git.state === "checked") {
    if (git.found.installed) {
      const version = git.found.version ? ` ${git.found.version}` : "";
      return { ...row, level: "ok", title: `git${version} is installed`, fix: null };
    }
    return { ...row, level: "problem", title: "git is not installed", ...install };
  }
  if (git.state === "failed") {
    // The macOS stub that stands in for git until the developer tools are installed.
    if (/xcrun|developer (path|tools)|CommandLineTools/i.test(git.message)) {
      return { ...row, level: "problem", title: "git needs Apple's developer tools", ...install };
    }
    return { ...row, level: "problem", title: "git could not be checked", fix: git.message };
  }
  return { ...row, level: "skipped", title: "git was not checked", fix: null };
}

function sandboxRow({ sandbox }: SetupReport): CheckRow {
  const row = { id: "sandbox", name: "Sandbox", commands: [] };
  if (sandbox.state === "available") {
    const gaps = sandbox.gaps.length > 0 ? `It does not cover ${sandbox.gaps.join(", ")}.` : null;
    return { ...row, level: "ok", title: `Agents run in a sandbox (${sandbox.name})`, fix: gaps };
  }
  const reason = sandbox.reason;
  const title = "No sandbox: agents can change anything you can";
  if (/bubblewrap|bwrap/i.test(reason)) {
    return {
      ...row,
      level: "problem",
      title,
      fix: "Install bubblewrap with your system's package manager, then restart Ovra.",
      commands: [
        { label: "Debian, Ubuntu", command: "sudo apt install bubblewrap" },
        { label: "Fedora", command: "sudo dnf install bubblewrap" },
        { label: "Arch", command: "sudo pacman -S bubblewrap" },
      ],
    };
  }
  if (/macOS sandbox has not been tested/i.test(reason)) {
    return {
      ...row,
      level: "warning",
      title,
      fix: "The Mac sandbox stays off until it has been tested on a Mac. Ovra still asks before agents change files or run commands.",
    };
  }
  const fix = reason ? `${capital(reason)}.` : "Ovra still asks before agents change files or run commands.";
  return { ...row, level: "problem", title, fix };
}

function ghRow({ gh }: SetupReport): CheckRow {
  const row = { id: "gh", name: "GitHub CLI (gh)", commands: [] };
  if (gh.state === "skipped") {
    return { ...row, level: "skipped", title: "Open a project to check its GitHub sign-in", fix: null };
  }
  if (gh.state === "failed") {
    return { ...row, level: "warning", title: "gh could not be checked", fix: gh.message };
  }
  const found = gh.found;
  const version = found.version ? ` ${found.version}` : "";
  if (!found.installed) {
    return {
      ...row,
      level: "warning",
      title: "gh is not installed",
      fix: "Only needed to open pull requests from Ovra. Install it, sign in, then check again.",
      commands: [{ command: "gh auth login" }],
      link: { label: "Get GitHub CLI", url: GH_URL },
    };
  }
  if (!found.host) {
    return {
      ...row,
      level: "ok",
      title: `gh${version} is installed`,
      fix: "This project has no remote named origin, so its sign-in was not checked.",
    };
  }
  if (!found.signed_in) {
    return {
      ...row,
      level: "warning",
      title: `gh is not signed in to ${found.host}`,
      fix: "Sign in once in a terminal to open pull requests from Ovra, then check again.",
      commands: [{ command: `gh auth login --hostname ${found.host}` }],
    };
  }
  const who = found.login ? ` as ${found.login}` : "";
  return { ...row, level: "ok", title: `gh${version} is signed in to ${found.host}${who}`, fix: null };
}

function capital(text: string): string {
  return text.charAt(0).toUpperCase() + text.slice(1);
}
