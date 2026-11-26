// Facts about Claude that only Claude's setup says (ADR 0011, "UI wording"):
// how to install and sign in to the claude program, and who pays, including
// the ANTHROPIC_API_KEY warning ADR 0006 asks for. Generic places say
// "agent"; the setup card's titles take the provider's display name.

import type { AgentSetup, Billing } from "../../lib/protocol";

/** Where to read about installing; opened through `open.url`. */
export const SETUP_DOCS = "https://code.claude.com/docs/en/setup";

/** Install commands for every OS: the UI does not know which one it runs on. */
export const INSTALL_COMMANDS: { label: string; command: string }[] = [
  { label: "macOS, Linux, WSL", command: "curl -fsSL https://claude.ai/install.sh | bash" },
  { label: "Windows PowerShell", command: "irm https://claude.ai/install.ps1 | iex" },
  { label: "npm", command: "npm install -g @anthropic-ai/claude-code" },
];

const PROVIDERS: Record<string, string> = {
  bedrock: "Amazon Bedrock",
  vertex: "Google Vertex AI",
  foundry: "Microsoft Foundry",
};

/** One sentence on who pays for `billing`. */
export function paidText(billing: Billing): string {
  if (billing.kind === "subscription") return "Usage counts against your Claude plan's limits.";
  if (billing.kind === "api_key") return "Usage is billed to an Anthropic API key, at API prices.";
  const provider = PROVIDERS[billing.provider] ?? billing.provider;
  return provider
    ? `Usage is billed to your ${provider} account.`
    : "Usage is billed to your cloud provider's account.";
}

/**
 * Who pays for agents' usage. With ANTHROPIC_API_KEY set (and no cloud
 * provider) the key decides, so the warning replaces the plan line instead
 * of contradicting it.
 */
export function billingText(setup: AgentSetup): { paid: string | null; warning: string | null } {
  const billing = setup.billing;
  if (keyBills(setup)) {
    const may = billing?.kind === "api_key" ? "is" : "may be";
    return {
      paid: null,
      warning: `Usage ${may} billed to the API key in ANTHROPIC_API_KEY, at API prices, not to your Claude plan. To use your plan, unset it and restart Ovra.`,
    };
  }
  return { paid: billing ? paidText(billing) : null, warning: null };
}

/** Who pays, as the tail of the one-line ready card; `null` when the API key warning says it. */
export function paidShort(setup: AgentSetup): string | null {
  const billing = setup.billing;
  if (!billing || keyBills(setup)) return null;
  if (billing.kind === "subscription") return "uses your Claude plan";
  if (billing.kind === "api_key") return "billed to an API key";
  return `billed to ${PROVIDERS[billing.provider] ?? (billing.provider || "your cloud provider")}`;
}

/** Who pays, in two or three words, for a session's line. */
export function billedShort(billing: Billing): string {
  if (billing.kind === "subscription") return "Claude plan";
  if (billing.kind === "api_key") return "API key, at API prices";
  return PROVIDERS[billing.provider] ?? (billing.provider || "Cloud provider");
}

/** `ANTHROPIC_API_KEY` is set and no cloud provider takes precedence over it. */
function keyBills(setup: AgentSetup): boolean {
  return setup.api_key_in_env && setup.billing?.kind !== "cloud";
}

export type SetupAdvice = {
  /** One line for the card's heading. */
  title: string;
  /** What to do, in plain words; `null` when the billing warning says it all. */
  detail: string | null;
  /** A command to run in a terminal, if one fixes it. */
  command?: string;
  /** Whether to show the install commands. */
  install?: boolean;
  /** The saved path leads nowhere: offer to search for claude instead. */
  findAutomatically?: boolean;
};

/** The usual mix-up: the command line program keeps its own sign-in. */
const OWN_SIGN_IN =
  "Signing in to the Claude desktop app or claude.ai does not sign in the claude command line program that Ovra uses.";

/** What the setup card says; `name` is the provider's display name. */
export function advice(setup: AgentSetup, name: string): SetupAdvice {
  const version = setup.version ? ` ${setup.version}` : "";
  const state = setup.state;
  switch (state.state) {
    case "ready":
      return {
        title: `${name}${version} is ready`,
        // With the key paying, the sign-in line would only distract from the warning.
        detail: keyBills(setup) ? null : "Sessions use your own Claude sign-in and plan.",
      };
    case "not_found":
      if (setup.path) {
        return {
          title: `${name} was not found`,
          detail: `There is no program at ${setup.path}.`,
          findAutomatically: true,
        };
      }
      return {
        title: `${name} is not installed`,
        detail: `Ovra uses the claude command line program. Install it, then run claude once in a terminal and sign in there. ${OWN_SIGN_IN}`,
        install: true,
      };
    case "outdated":
      return {
        title: `${name}${version} is too old`,
        detail: `Ovra needs version ${state.minimum} or newer. Update it in a terminal, then check again.`,
        command: "claude update",
      };
    case "too_new":
      return {
        title: `${name}${version} is newer than Ovra supports`,
        detail: `This build of Ovra supports versions below ${state.below}. Update Ovra.`,
      };
    case "signed_out":
      return {
        title: `Sign in to ${name}`,
        detail: `Run claude once in a terminal and sign in there (or run the command below), then check again. ${OWN_SIGN_IN} Ovra never sees your sign-in.`,
        command: "claude auth login",
      };
    case "broken":
      return { title: `${name} could not be checked`, detail: state.message };
  }
}

/** The terminal command that signs in to an account; Ovra never sees the sign-in. */
export function signInCommand(configDir: string | null): string {
  return configDir ? `CLAUDE_CONFIG_DIR=${quote(configDir)} claude auth login` : "claude auth login";
}

function quote(path: string): string {
  return /^[\w@%+=:,./~-]+$/.test(path) ? path : `'${path.replaceAll("'", `'\\''`)}'`;
}

/** Where the default account keeps its settings and sign-in. */
export const DEFAULT_FOLDER = "Claude Code's own folder (~/.claude)";

/** What an account's folder is, under its field. */
export const FOLDER_HINT = "The folder you give Claude Code as CLAUDE_CONFIG_DIR for this account. Use the full path.";
