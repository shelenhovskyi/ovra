import { flushSync, mount, unmount } from "svelte";
import { afterEach, beforeEach, describe, expect, it } from "vitest";
import { Workspace } from "../shell/workspace.svelte";
import { bash, entry, FakeAuditCore } from "./fake-audit";
import TestHost from "./TestHost.svelte";

let app: Record<string, unknown> | null = null;
let target: HTMLElement;

beforeEach(() => {
  target = document.createElement("div");
  document.body.append(target);
});

afterEach(() => {
  if (app) unmount(app);
  app = null;
  target.remove();
});

async function settle() {
  for (let i = 0; i < 6; i++) {
    flushSync();
    await Promise.resolve();
  }
  flushSync();
}

async function setup() {
  const core = new FakeAuditCore();
  core.entries = [
    entry(1, { kind: "session_started", mode: "direct", trusted: false, sandbox: null }),
    entry(2, bash(true)),
    entry(3, bash(false)),
  ];
  const workspace = new Workspace();
  workspace.project = { id: "p1", name: "demo", root: "/demo", last_opened_ms: null, trusted: false };
  app = mount(TestHost, { target, props: { transport: core, workspace } });
  await settle();
  return core;
}

const titles = () =>
  [...target.querySelectorAll("[data-testid=audit-entry] strong")].map((el) => el.textContent?.trim());

describe("AuditLog", () => {
  it("opens on the project's entries, newest first, follows new ones and returns focus on Escape", async () => {
    const core = await setup();
    const open = target.querySelector<HTMLButtonElement>("[data-testid=open-audit]");
    open?.focus();
    open?.click();
    await settle();
    expect(document.activeElement?.id).toBe("audit-title");
    expect(titles()).toEqual(["Refused Bash", "Allowed Bash", "Agent started"]);
    expect(target.querySelector("[data-testid=audit-entry]")?.className).toContain("refused");

    core.append(entry(4, { kind: "trust_changed", trusted: true, fingerprint: "9f2c" }));
    await settle();
    expect(titles()[0]).toBe("Project settings trusted");

    target
      .querySelector("[data-testid=audit]")
      ?.dispatchEvent(new KeyboardEvent("keydown", { code: "Escape", key: "Escape", bubbles: true }));
    await settle();
    expect(target.querySelector("[data-testid=audit]")).toBeNull();
    expect(document.activeElement).toBe(open);
  });

  it("keeps the input's JSON behind Details, and an empty log without filters", async () => {
    const core = await setup();
    core.entries = [];
    target.querySelector<HTMLButtonElement>("[data-testid=open-audit]")?.click();
    await settle();
    expect(target.querySelector("[data-testid=audit-empty]")?.textContent?.trim()).toMatch(
      /^Nothing yet\. Entries appear when an agent starts/,
    );
    expect(target.querySelector("[data-testid=audit-filter]")).toBeNull();

    core.append(entry(1, bash(true)));
    await settle();
    const first = target.querySelector("[data-testid=audit-entry]");
    expect(first?.querySelector("pre.input")?.textContent).toBe("ls");
    expect(first?.querySelector("details:not([open]) pre")?.textContent).toContain('"command": "ls"');
    expect(target.querySelector("[data-testid=audit-filter]")).not.toBeNull();
  });

  it("filters to refusals and by kind", async () => {
    await setup();
    target.querySelector<HTMLButtonElement>("[data-testid=open-audit]")?.click();
    await settle();
    target.querySelector<HTMLInputElement>("[data-testid=audit-refused]")?.click();
    await settle();
    expect(titles()).toEqual(["Refused Bash"]);
    const select = target.querySelector<HTMLSelectElement>("[data-testid=audit-filter]");
    if (select) select.value = "sessions";
    select?.dispatchEvent(new Event("change", { bubbles: true }));
    await settle();
    expect(target.querySelector("[data-testid=audit-empty]")?.textContent?.trim()).toBe("No entries match.");
  });

  it("opens a long log on its newest page and loads older ones on request", async () => {
    const core = await setup();
    // Refused: the first and the last; only they render, which keeps this fast.
    core.entries = Array.from({ length: 501 }, (_, i) => entry(i + 1, bash(i !== 0 && i !== 500)));
    target.querySelector<HTMLButtonElement>("[data-testid=open-audit]")?.click();
    flushSync();
    target.querySelector<HTMLInputElement>("[data-testid=audit-refused]")?.click();
    await settle();
    const seqs = () => [...target.querySelectorAll("[data-testid=audit-entry] time")].map((el) => el.textContent);
    expect(titles()).toEqual(["Refused Bash"]);
    const older = () => target.querySelector<HTMLButtonElement>("[data-testid=audit-older]");
    older()?.click();
    await settle();
    expect(titles()).toEqual(["Refused Bash", "Refused Bash"]);
    expect(new Set(seqs()).size).toBe(2);
    expect(older()).toBeNull();
    expect(target.querySelector("[data-testid=audit-older-note]")?.textContent).toBe("1 older entry loaded.");
  });

  it("counts only the older entries loaded, not ones appended meanwhile", async () => {
    const core = await setup();
    core.entries = Array.from({ length: 501 }, (_, i) => entry(i + 1, bash(true)));
    target.querySelector<HTMLButtonElement>("[data-testid=open-audit]")?.click();
    flushSync();
    target.querySelector<HTMLInputElement>("[data-testid=audit-refused]")?.click();
    await settle();
    let release = () => {};
    const held = new Promise<void>((resolve) => {
      release = resolve;
    });
    const call = core.call.bind(core);
    core.call = (async (request) => {
      await held;
      return call(request);
    }) as typeof core.call;
    target.querySelector<HTMLButtonElement>("[data-testid=audit-older]")?.click();
    await settle();
    core.append(entry(502, bash(true)));
    core.append(entry(503, bash(true)));
    release();
    await settle();
    await settle();
    expect(target.querySelector("[data-testid=audit-older-note]")?.textContent).toBe("1 older entry loaded.");
  });

  it("keeps Load older focusable while it loads and focus in the dialog after", async () => {
    const core = await setup();
    // All allowed and only refusals shown, so none render.
    core.entries = Array.from({ length: 501 }, (_, i) => entry(i + 1, bash(true)));
    let release = () => {};
    const held = new Promise<void>((resolve) => {
      release = resolve;
    });
    target.querySelector<HTMLButtonElement>("[data-testid=open-audit]")?.click();
    flushSync();
    target.querySelector<HTMLInputElement>("[data-testid=audit-refused]")?.click();
    await settle();
    const call = core.call.bind(core);
    core.call = (async (request) => {
      await held;
      return call(request);
    }) as typeof core.call;
    const older = target.querySelector<HTMLButtonElement>("[data-testid=audit-older]");
    older?.focus();
    older?.click();
    await settle();
    expect(older?.disabled).toBe(false);
    expect(older?.getAttribute("aria-busy")).toBe("true");
    expect(document.activeElement).toBe(older);
    release();
    await settle();
    await settle();
    expect(target.querySelector("[data-testid=audit-older]")).toBeNull();
    expect(document.activeElement?.id).toBe("audit-title");
  });

  it("Tab wraps past disabled controls", async () => {
    await setup();
    target.querySelector<HTMLButtonElement>("[data-testid=open-audit]")?.click();
    await settle();
    const dialog = target.querySelector<HTMLElement>("[data-testid=audit]");
    const disabled = document.createElement("button");
    disabled.disabled = true;
    dialog?.append(disabled);
    const last = [...target.querySelectorAll<HTMLElement>("[data-testid=audit] summary")].at(-1);
    last?.focus();
    dialog?.dispatchEvent(new KeyboardEvent("keydown", { code: "Tab", key: "Tab", bubbles: true }));
    expect(document.activeElement).toBe(target.querySelector("[data-testid=audit-filter]"));
    disabled.remove();
  });
});
