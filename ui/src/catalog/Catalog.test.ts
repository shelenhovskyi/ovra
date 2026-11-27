import { flushSync, mount, unmount } from "svelte";
import { afterEach, expect, it } from "vitest";
import type { CatalogItem, CoreEvent, Request } from "../lib/protocol";
import type { Transport } from "../lib/transport";
import { mainView } from "../shell/main-view.svelte";
import { Workspace } from "../shell/workspace.svelte";
import TestHost from "./TestHost.svelte";

const SKILL: CatalogItem = {
  kind: "skill",
  name: "release",
  description: "Cuts a release when asked.",
  path: ".claude/skills/release/SKILL.md",
  agents: ["claude"],
  loads: "when_trusted",
  fields: [{ name: "allowed-tools", value: "Bash" }],
  files: [".claude/skills/release/tag.sh"],
  problem: null,
};
const RULES: CatalogItem = {
  ...SKILL,
  kind: "rules",
  name: "AGENTS.md",
  description: "Shared rules.",
  path: "AGENTS.md",
  agents: ["claude", "codex"],
  loads: "agent",
  fields: [],
  files: [],
};
const HOOK: CatalogItem = {
  ...SKILL,
  kind: "hook",
  name: "PreToolUse",
  description: "./check.sh",
  path: ".claude/settings.json",
  fields: [{ name: "matcher", value: "Bash" }],
  files: [],
};

let cleanup: (() => void)[] = [];

afterEach(() => {
  mainView.close(false);
  for (const off of cleanup) off();
  cleanup = [];
});

async function settle(): Promise<void> {
  for (let i = 0; i < 3; i++) {
    flushSync();
    await new Promise((resolve) => setImmediate(resolve));
  }
  flushSync();
}

function setup(items: CatalogItem[]) {
  const calls: Request[] = [];
  const listeners = new Set<(event: CoreEvent) => void>();
  let answer = items;
  const transport: Transport = {
    call: ((request: Request) => {
      calls.push(request);
      if (request.method === "catalog.list") return Promise.resolve({ type: "catalog.items", data: { items: answer } });
      if (request.method === "fs.read_file")
        return Promise.resolve({
          type: "fs.file",
          data: {
            path: request.params.path,
            size: 1,
            mtime_ms: 1,
            body: { kind: "text", text: "---\nname: release\n---\n# Release steps\n\nTag it." },
          },
        });
      return Promise.reject(new Error(`unexpected ${request.method}`));
    }) as Transport["call"],
    onEvent: (listener) => {
      listeners.add(listener);
      return () => listeners.delete(listener);
    },
  };
  const workspace = new Workspace();
  workspace.project = { id: "p1", name: "demo", root: "/demo", last_opened_ms: null, trusted: false };
  const target = document.createElement("div");
  document.body.append(target);
  const app = mount(TestHost, { target, props: { transport, workspace } });
  cleanup.push(() => {
    void unmount(app);
    target.remove();
  });
  mainView.show("catalog");
  return {
    target,
    calls,
    workspace,
    emit: (event: CoreEvent) => {
      for (const listener of listeners) listener(event);
    },
    answer: (next: CatalogItem[]) => {
      answer = next;
    },
  };
}

const text = (target: HTMLElement, id: string) => target.querySelector(`[data-testid=${id}]`)?.textContent?.trim();

it("lists items by kind and shows the first one's details and text", async () => {
  const { target } = setup([RULES, SKILL, HOOK]);
  await settle();
  const headings = [...target.querySelectorAll("nav h3")].map((h) => h.textContent?.replace(/\s+/g, " ").trim());
  expect(headings).toEqual(["Instructions 1", "Skills 1", "Hooks 1"]);
  expect(text(target, "catalog-detail")).toContain("Instructions file · Claude, Codex");
  expect(text(target, "catalog-text")).toContain("Release steps");
  expect(target.querySelector("[data-testid=catalog-loads]")).toBeNull();
});

it("shows a skill's fields, files and when Claude loads it, and opens its files", async () => {
  const { target, workspace } = setup([RULES, SKILL]);
  await settle();
  const skill = [...target.querySelectorAll<HTMLButtonElement>("nav button")].find(
    (b) => b.textContent?.trim() === "release",
  );
  skill?.click();
  await settle();
  const detail = text(target, "catalog-detail") ?? "";
  expect(detail).toContain("Tools it may use without asking");
  expect(detail).toContain(".claude/skills/release/tag.sh");
  expect(text(target, "catalog-loads")).toMatch(/only once you trust this project/);
  expect(text(target, "catalog-text")).not.toContain("name: release");
  target.querySelector<HTMLButtonElement>("[data-testid=catalog-open]")?.click();
  flushSync();
  expect(workspace.openPath).toBe(".claude/skills/release/SKILL.md");
  expect(mainView.current).toBeNull();
});

it("moves with arrow keys, names hook events in words and filters", async () => {
  const { target } = setup([RULES, SKILL, HOOK]);
  await settle();
  const first = target.querySelector<HTMLButtonElement>("nav button");
  first?.focus();
  first?.dispatchEvent(new KeyboardEvent("keydown", { code: "End", bubbles: true }));
  await settle();
  expect(document.activeElement?.textContent?.trim()).toBe("Before a tool runs");
  expect(text(target, "catalog-detail")).toContain("./check.sh");
  const filter = target.querySelector<HTMLInputElement>("[data-testid=catalog-filter]");
  if (filter) {
    filter.value = "release";
    filter.dispatchEvent(new Event("input"));
  }
  await settle();
  expect([...target.querySelectorAll("nav button")].map((b) => b.textContent?.trim())).toEqual(["release"]);
});

it("explains what goes here when there is nothing, and reads again when an agent file changes", async () => {
  const { target, calls, emit, answer } = setup([]);
  await settle();
  expect(text(target, "catalog-empty")).toMatch(/AGENTS\.md, CLAUDE\.md and \.claude\/skills/);
  emit({ type: "fs.changed", data: { project: "p1", paths: ["src/main.rs"] } });
  await settle();
  expect(calls.filter((c) => c.method === "catalog.list")).toHaveLength(1);
  answer([SKILL]);
  emit({ type: "fs.changed", data: { project: "p1", paths: [".claude/skills/release/SKILL.md"] } });
  await settle();
  expect(calls.filter((c) => c.method === "catalog.list")).toHaveLength(2);
  expect(target.querySelector("nav button")?.textContent?.trim()).toBe("release");
});

it("opens Teach the project from the empty state and from the list, and Cancel closes it", async () => {
  const { target } = setup([]);
  await settle();
  const action = [...target.querySelectorAll<HTMLButtonElement>("[data-testid=catalog-empty] button")].find(
    (b) => b.textContent?.trim() === "Teach the project",
  );
  action?.click();
  await settle();
  expect(target.querySelector("[data-testid=catalog-teach]")).not.toBeNull();
  [...target.querySelectorAll<HTMLButtonElement>("[data-testid=catalog-teach] button")]
    .find((b) => b.textContent?.trim() === "Cancel")
    ?.click();
  flushSync();
  expect(target.querySelector("[data-testid=catalog-empty]")).not.toBeNull();
});

it("shows Teach the project in place of the detail", async () => {
  const { target } = setup([RULES]);
  await settle();
  target.querySelector<HTMLButtonElement>("[data-testid=catalog-teach-open]")?.click();
  await settle();
  expect(target.querySelector("[data-testid=catalog-detail]")).toBeNull();
  expect(target.querySelector("[data-testid=catalog-teach]")).not.toBeNull();
  expect(target.querySelector("[data-testid=catalog-teach-open]")).toBeNull();
});
