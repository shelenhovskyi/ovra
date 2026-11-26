import { describe, expect, it } from "vitest";
import type { Capabilities } from "../lib/protocol";
import { attemptAgents, modesFor, rememberAgent, rememberedAgent, rememberedMode, rememberMode } from "./task-mode";

describe("task mode", () => {
  it("remembers the last mode per project and survives a blocked storage", () => {
    const items = new Map<string, string>();
    const store = {
      getItem: (key: string) => items.get(key) ?? null,
      setItem: (key: string, value: string) => void items.set(key, value),
    } as Storage;
    expect(rememberedMode("p1", store)).toBe("here");
    rememberMode("p1", "split", store);
    expect(rememberedMode("p1", store)).toBe("split");
    expect(rememberedMode("p2", store)).toBe("here");

    const blocked = {
      getItem: () => {
        throw new Error("denied");
      },
      setItem: () => {
        throw new Error("full");
      },
    } as unknown as Storage;
    rememberMode("p1", "split", blocked);
    expect(rememberedMode("p1", blocked)).toBe("here");
  });

  it("remembers the agent per project", () => {
    const items = new Map<string, string>();
    const store = {
      getItem: (key: string) => items.get(key) ?? null,
      setItem: (key: string, value: string) => void items.set(key, value),
    } as Storage;
    expect(rememberedAgent("p1", store)).toBe("claude");
    rememberAgent("p1", "codex", store);
    expect(rememberedAgent("p1", store)).toBe("codex");
    rememberMode("p1", "own", store);
    expect(rememberedMode("p1", store)).toBe("own");
  });

  it("offers an agent only the modes it can run in", () => {
    const none: Capabilities = {
      approvals: true,
      enforced_policy: false,
      ide_tools: false,
      read_only_role: false,
      resume: false,
      fork: false,
      interrupt: true,
      text_deltas: true,
      subagents: false,
      usage: false,
      cost: false,
      billing: false,
      accounts: false,
    };
    expect(modesFor("claude", undefined)).toEqual(["here", "own", "split", "research"]);
    expect(modesFor("gemini", none)).toEqual(["own"]);
    // Until its capabilities are known another agent gets only its own copy.
    expect(modesFor("gemini", undefined)).toEqual(["own"]);
    expect(modesFor("codex", { ...none, enforced_policy: true })).toEqual(["here", "own"]);
  });

  it("lets different agents try a task before one tries it twice", () => {
    expect(attemptAgents("codex", ["claude", "codex"], 3)).toEqual(["codex", "claude", "codex"]);
    expect(attemptAgents("claude", ["claude"], 2)).toEqual(["claude", "claude"]);
    expect(attemptAgents("claude", ["claude", "gemini", "codex"], 1)).toEqual(["claude"]);
  });
});
