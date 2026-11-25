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

/** The accounts in one line: each name, with what is wrong if anything. */
export function accountsSummary(states: AccountState[]): string {
  return states
    .map((state) => {
      const status = accountStatus(state);
      return status.ok
        ? state.account.name
        : `${state.account.name} (${status.text.replace(/[.:].*$/, "").toLowerCase()})`;
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
