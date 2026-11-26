// What the UI says about agents other than Claude, per vendor: how to get
// one and how to sign in, in the vendor's own words. Ovra never installs an
// agent or signs in for the person (ADR 0006, ADR 0011).

import type { AgentKind, AgentSetup } from "../../lib/protocol";

type Texts = { program: string; install: string; signIn: string };

const TEXTS: Partial<Record<AgentKind, Texts>> = {
  gemini: {
    program: "gemini",
    install: "Install Gemini CLI: npm install -g @google/gemini-cli",
    signIn: "Run gemini in a terminal once and sign in with Google, or set GEMINI_API_KEY.",
  },
  codex: {
    program: "codex-acp",
    install: "Install Codex for editors: npm install -g @zed-industries/codex-acp",
    signIn: "Run codex login in a terminal (from npm install -g @openai/codex), or set OPENAI_API_KEY.",
  },
};

/** The program Ovra runs for `id`. */
export function programOf(id: AgentKind): string {
  return TEXTS[id]?.program ?? id;
}

/** One line on `setup`, and what to do about it when something is wrong. */
export function providerState(
  id: AgentKind,
  name: string,
  setup: AgentSetup | undefined,
): { line: string; fix?: string } {
  const texts = TEXTS[id];
  switch (setup?.state.state) {
    case undefined:
      return { line: `Checking ${name}…` };
    case "ready":
      return { line: `${name} is ready${setup.version ? ` (${setup.version})` : ""}. It works only on its own copy.` };
    case "signed_out":
      return { line: `${name} is not signed in.`, ...(texts ? { fix: texts.signIn } : {}) };
    case "not_found":
      return { line: `${name} is not installed.`, ...(texts ? { fix: texts.install } : {}) };
    case "broken":
      return { line: `${name} could not be checked: ${setup.state.message}` };
    default:
      return { line: `${name} needs an update.` };
  }
}
