import { flushSync, mount, unmount } from "svelte";
import { afterEach, beforeEach, describe, expect, it } from "vitest";
import { Workspace } from "../shell/workspace.svelte";
import { change, FakeReviewCore, review } from "./fake-review";
import TestHost from "./TestHost.svelte";

// jsdom has no ResizeObserver, which the diff's `bind:clientHeight` uses.
globalThis.ResizeObserver ??= class {
  observe() {}
  unobserve() {}
  disconnect() {}
};

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
  for (let i = 0; i < 4; i++) {
    flushSync();
    await Promise.resolve();
  }
  await new Promise((resolve) => setTimeout(resolve, 0));
  flushSync();
}

async function setup(items = [review("r1")]) {
  const core = new FakeReviewCore();
  core.items = items;
  const workspace = new Workspace();
  workspace.project = { id: "p1", name: "demo", root: "/demo", last_opened_ms: null, trusted: false };
  app = mount(TestHost, { target, props: { transport: core, workspace } });
  await settle();
  return core;
}

function press(code: string, init: KeyboardEventInit = {}) {
  const key = code.startsWith("Key") ? code.slice(3).toLowerCase() : code;
  (document.activeElement ?? document.body).dispatchEvent(
    new KeyboardEvent("keydown", { code, key, bubbles: true, cancelable: true, ...init }),
  );
}

const $ = <T extends HTMLElement = HTMLElement>(selector: string) => target.querySelector<T>(selector);
const button = (name: RegExp) =>
  [...target.querySelectorAll<HTMLButtonElement>("[data-testid=inbox] button")].find((b) =>
    name.test(b.textContent ?? ""),
  );

async function open() {
  $("[data-testid=elsewhere]")?.focus();
  press("KeyR", { altKey: true, shiftKey: true });
  await settle();
}

describe("Inbox", () => {
  it("opens from the keyboard on the newest open item, shows its diff and returns focus on Escape", async () => {
    await setup([
      review("r1", { files: [change("README.md"), change("notes.md", { by: "outside" })] }),
      review("r2", { session: "s2", mode: "isolated", files: [change("src/a.ts", { blocked: true })] }),
    ]);
    expect($("[data-testid=inbox-button]")?.textContent).toContain("(2 waiting)");
    await open();
    expect($("[role=dialog]")?.getAttribute("aria-modal")).toBe("true");
    expect(document.activeElement?.getAttribute("data-item")).toBe("r1");
    expect($("[data-testid=inbox-diff]")?.textContent).toContain("the readme");
    expect([...target.querySelectorAll("[data-testid=file-marker]")].map((m) => m.textContent)).toEqual([
      "Changed outside the agent",
    ]);

    press("ArrowDown");
    await settle();
    expect(document.activeElement?.getAttribute("data-item")).toBe("r2");
    expect($("[data-testid=inbox-item] h3")?.textContent).toBe("Worker: Session s2");
    expect($("[data-file='src/a.ts']")?.textContent).toContain("Waits for the direct agent");
    expect($("[data-testid=inbox-item]")?.textContent).toContain("Merging its branch stays with you.");

    press("Escape");
    await settle();
    expect($("[data-testid=inbox]")).toBeNull();
    expect(document.activeElement).toBe($("[data-testid=elsewhere]"));
  });

  it("approves only on a press, and commits with the message given", async () => {
    const core = await setup([review("r1", { summary: "Fix the typo" })]);
    await open();
    expect(core.sent("review.approve")).toEqual([]);
    $<HTMLInputElement>("[data-testid=inbox] input[type=checkbox]")?.click();
    flushSync();
    expect($<HTMLInputElement>("[data-testid=inbox] input[type=text]")?.value).toBe("Fix the typo");
    button(/^Approve and commit$/)?.click();
    await settle();
    expect(core.sent("review.approve").map((c) => c.params)).toEqual([{ id: "r1", commit: "Fix the typo" }]);
    expect($("[data-testid=inbox-outcome]")?.textContent?.trim()).toBe("Approved.");
    expect(button(/^Approve/)).toBeUndefined();
  });

  it("rejects after a confirmation that says what stays, and lists what was left", async () => {
    const core = await setup([review("r1", { files: [change("README.md", { by: "both" })] })]);
    core.conflicts = ["README.md"];
    await open();
    button(/^Reject…$/)?.click();
    await settle();
    expect(core.sent("review.reject")).toEqual([]);
    expect(document.activeElement?.textContent).toBe("Reject the changes");
    expect($("#inbox-reject-hint")?.textContent).toContain("Changes made outside the agent stay as they are");
    (document.activeElement as HTMLElement).click();
    await settle();
    expect(core.sent("review.reject")).toHaveLength(1);
    expect($("[data-testid=inbox-outcome]")?.textContent?.trim()).toBe("Rejected. Left for you: README.md.");
  });

  it("sends a comment with Mod+Enter and keeps the item open", async () => {
    const core = await setup();
    await open();
    const field = $<HTMLTextAreaElement>("#inbox-comment");
    field?.focus();
    if (field) field.value = "Add a test";
    field?.dispatchEvent(new Event("input", { bubbles: true }));
    press("Enter", { ctrlKey: true });
    await settle();
    expect(core.sent("review.comment").map((c) => c.params)).toEqual([{ id: "r1", text: "Add a test" }]);
    expect(field?.value).toBe("");
    expect(button(/^Approve$/)).toBeDefined();
  });

  it("chooses the review an agent asks to show without taking focus, and follows changes", async () => {
    const core = await setup([review("r1"), review("r2", { files: [change("b.md")] })]);
    $("[data-testid=elsewhere]")?.focus();
    core.emit({ type: "orchestration.show", data: { project: "p1", target: { kind: "review", id: "r2" } } });
    await settle();
    expect($("[data-testid=inbox]")).toBeNull();
    expect(document.activeElement).toBe($("[data-testid=elsewhere]"));
    expect($("[data-testid=inbox-asked]")?.textContent).toContain("An agent asks you to review its changes.");
    await open();
    expect(document.activeElement?.getAttribute("data-item")).toBe("r2");
    expect($("[data-testid=inbox-asked]")?.textContent).toBe("");
    const diffs = core.sent("review.diff").length;
    core.emit({ type: "review.changed", data: { project: "p1", id: "r2" } });
    await settle();
    expect(core.sent("review.diff").length).toBe(diffs + 1);
  });

  it("says when nothing waits", async () => {
    await setup([]);
    await open();
    expect($("[data-testid=inbox]")?.textContent).toContain("No agent changes wait for review.");
    expect(document.activeElement?.id).toBe("inbox-title");
  });
});
