// Words for Claude accounts: how each one stands, and how to sign in to it.

import type { AccountState } from "../../lib/protocol";
import { paidShort } from "./texts";

/** The terminal command that signs in to an account; Ovra never sees the sign-in. */
export function signInCommand(configDir: string | null): string {
  return configDir ? `CLAUDE_CONFIG_DIR=${quote(configDir)} claude auth login` : "claude auth login";
}

function quote(path: string): string {
  return /^[\w@%+=:,./~-]+$/.test(path) ? path : `'${path.replaceAll("'", `'\\''`)}'`;
}

/** One line on whether the account can run sessions, and a command if one fixes it. */
export function accountStatus({ account, setup }: AccountState): { text: string; ok: boolean; command?: string } {
  switch (setup.state.state) {
    case "ready": {
      const paid = paidShort(setup);
      return { text: paid ? `Signed in, ${paid}` : "Signed in", ok: true };
    }
    case "signed_out":
      return { text: "Not signed in. Run this in a terminal:", ok: false, command: signInCommand(account.config_dir) };
    case "not_found":
      return { text: "claude was not found", ok: false };
    case "outdated":
      return { text: `claude is too old (needs ${setup.state.minimum})`, ok: false, command: "claude update" };
    case "too_new":
      return { text: "claude is newer than Ovra supports", ok: false };
    case "broken":
      return { text: setup.state.message, ok: false };
  }
}

/** An account in the picker: its name, and what to know before starting with it. */
export function pickerLabel(state: AccountState): string {
  const status = accountStatus(state);
  if (!status.ok) return `${state.account.name} (${shortProblem(status.text)})`;
  const billing = state.setup.billing;
  if (billing?.kind === "api_key") return `${state.account.name} (API key)`;
  if (billing?.kind === "cloud") return `${state.account.name} (${billing.provider || "cloud"})`;
  return state.account.name;
}

/** What the person should know before starting with the picked account; null when nothing. */
export function pickerWarning(state: AccountState): string | null {
  const status = accountStatus(state);
  const name = state.account.name;
  if (!status.ok)
    return `${name}: ${shortProblem(status.text)}. Agents started with it will fail; Manage under Claude above shows how to fix it.`;
  if (state.setup.billing?.kind === "api_key") return `${name} bills an Anthropic API key, at API prices.`;
  return null;
}

function shortProblem(text: string): string {
  return text.replace(/[.:].*$/, "").toLowerCase();
}

/** The accounts in one line: each name, with what is wrong if anything. */
export function accountsSummary(states: AccountState[]): string {
  return states
    .map((state) => {
      const status = accountStatus(state);
      return status.ok ? state.account.name : `${state.account.name} (${shortProblem(status.text)})`;
    })
    .join(", ");
}

/** A name for a found folder: `/home/me/.claude-work` is "Work". */
export function nameFromFolder(path: string): string {
  const base = path.split(/[\\/]/).filter(Boolean).pop() ?? "";
  const rest = base.replace(/^\.?claude/i, "").replace(/^[-_. ]+/, "");
  if (!rest) return "Claude";
  return rest.charAt(0).toUpperCase() + rest.slice(1);
}
