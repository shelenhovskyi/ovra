// What the setup and trust cards say. Kept apart from the components so the
// wording of every state is tested in one place.

import type { AgentSetup, ConfigKind } from "../../lib/protocol";

/** Where to read about installing; opened through `open.url`. */
export const SETUP_DOCS = "https://code.claude.com/docs/en/setup";

/** Install commands for every OS: the UI does not know which one it runs on. */
export const INSTALL_COMMANDS: { label: string; command: string }[] = [
  { label: "macOS, Linux, WSL", command: "curl -fsSL https://claude.ai/install.sh | bash" },
  { label: "Windows PowerShell", command: "irm https://claude.ai/install.ps1 | iex" },
  { label: "npm", command: "npm install -g @anthropic-ai/claude-code" },
];

export type SetupAdvice = {
  /** One line for the card's heading. */
  title: string;
  /** What to do, in plain words. */
  detail: string;
  /** A command to run in a terminal, if one fixes it. */
  command?: string;
  /** Whether to show the install commands. */
  install?: boolean;
};

export function advice(setup: AgentSetup): SetupAdvice {
  const version = setup.version ? ` ${setup.version}` : "";
  const state = setup.state;
  switch (state.state) {
    case "ready":
      return { title: `Claude${version} is ready`, detail: "Sessions use your own Claude sign-in and plan." };
    case "not_found":
      return {
        title: "Claude is not installed",
        detail: setup.path
          ? `Nothing was found at ${setup.path}. Install Claude Code or choose another path.`
          : "Install Claude Code, then sign in once by running claude in a terminal. If it is installed somewhere else, choose its path below.",
        install: true,
      };
    case "outdated":
      return {
        title: `Claude${version} is too old`,
        detail: `Ovra needs version ${state.minimum} or newer. Update it in a terminal, then check again.`,
        command: "claude update",
      };
    case "too_new":
      return {
        title: `Claude${version} is newer than Ovra supports`,
        detail: `This build of Ovra supports versions below ${state.below}. Update Ovra.`,
      };
    case "signed_out":
      return {
        title: "Sign in to Claude",
        detail: "Run this in a terminal and follow the steps, then check again. Ovra never sees your sign-in.",
        command: "claude auth login",
      };
    case "broken":
      return { title: "Claude could not be checked", detail: state.message };
  }
}

/** Headings of the configuration a trusted project loads. */
export const KIND_LABELS: Record<ConfigKind, string> = {
  hooks: "Hooks: commands that run on agent events",
  helpers: "Helper commands",
  plugins: "Plugins, which can bring their own hooks and servers",
  env: "Environment variables",
  allow_rules: "Tools allowed without asking you",
  settings: "Other settings",
  agents: "Custom subagents",
  commands: "Slash commands",
  skills: "Skills",
  unreadable: "Could not be read, may still load",
};

/** Whether loading this kind runs commands on the user's computer. */
export function runsCode(kind: ConfigKind): boolean {
  return kind === "hooks" || kind === "helpers" || kind === "plugins";
}
