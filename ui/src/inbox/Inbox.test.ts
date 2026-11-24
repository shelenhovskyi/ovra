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
let workspace: Workspace;
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
  workspace = new Workspace();
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
    expect($("[data-testid=inbox-item]")?.textContent).toContain("into your project folder");

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
    const message = $<HTMLInputElement>("[data-testid=inbox] input[type=text]");
    expect(message?.value).toBe("Fix the typo");
    // An empty message says why approve waits.
    if (message) message.value = " ";
    message?.dispatchEvent(new Event("input", { bubbles: true }));
    flushSync();
    expect(button(/^Approve and commit$/)?.getAttribute("aria-describedby")).toBe("inbox-commit-why");
    expect($("#inbox-commit-why")?.textContent).toBe("Write a commit message to approve and commit.");
    button(/^Approve and commit$/)?.click();
    await settle();
    expect(core.sent("review.approve")).toEqual([]);
    if (message) message.value = "Fix the typo";
    message?.dispatchEvent(new Event("input", { bubbles: true }));
    flushSync();
    button(/^Approve and commit$/)?.click();
    await settle();
    expect(core.sent("review.approve").map((c) => c.params)).toEqual([{ id: "r1", commit: "Fix the typo" }]);
    expect($("[data-testid=inbox-outcome]")?.textContent?.trim()).toBe("Approved. Committed as “Fix the typo”.");
    expect(button(/^Approve/)).toBeUndefined();
    expect(document.activeElement?.id).toBe("inbox-item-title");
  });

  it("rejects after a confirmation that says what stays, and lists what was left", async () => {
    const core = await setup([review("r1", { files: [change("README.md", { by: "both" })] })]);
    core.conflicts = ["README.md"];
    core.restored = ["src/a.rs"];
    await open();
    button(/^Reject…$/)?.click();
    await settle();
    expect(core.sent("review.reject")).toEqual([]);
    // The step starts on the safe choice; Escape leaves the step, not the inbox.
    expect(document.activeElement?.textContent).toBe("Keep reviewing");
    expect($("#inbox-reject-hint")?.textContent).toContain("Changes made outside the agent stay as they are");
    press("Escape");
    await settle();
    expect($("[data-testid=inbox]")).not.toBeNull();
    expect($("#inbox-reject-hint")).toBeNull();
    expect(document.activeElement?.textContent).toBe("Reject…");
    button(/^Reject…$/)?.click();
    await settle();
    button(/^Reject the changes$/)?.click();
    await settle();
    expect(core.sent("review.reject")).toHaveLength(1);
    expect($("[data-testid=inbox-outcome]")?.textContent?.trim()).toBe(
      "Rejected. Restored: src/a.rs. Kept as you changed them: README.md.",
    );
  });

  it("says which of a worker's files were not copied, keeps the item waiting, and marks it done", async () => {
    const core = await setup([review("r1", { mode: "isolated", files: [change("NOTES.md"), change("a.rs")] })]);
    core.conflicts = ["NOTES.md"];
    await open();
    button(/^Approve$/)?.click();
    await settle();
    expect($("[data-testid=inbox-outcome]")?.textContent?.trim()).toBe(
      "Approved, not all copied. Not copied: NOTES.md.",
    );
    expect($("[data-item=r1]")?.textContent).toContain("Approved, not all copied");
    expect($("[data-item=r1]")?.textContent).toContain("1 not copied");
    expect($("[data-file='NOTES.md']")?.textContent).toContain("Not copied: your version changed");
    expect($("[data-testid=inbox-not-copied]")?.textContent).toContain(
      "The worker's version of each is in its diff below.",
    );
    $<HTMLButtonElement>("[data-testid=inbox-resolve]")?.click();
    await settle();
    expect(core.sent("review.resolve").map((c) => c.params)).toEqual([{ id: "r1" }]);
    expect($("[data-testid=inbox-outcome]")?.textContent?.trim()).toBe(
      "Marked done. Your versions of the files that were not copied stay.",
    );
    expect($("[data-testid=inbox-resolve]")).toBeNull();
    expect($("[data-file='NOTES.md']")?.textContent).toContain("Not copied (your version changed); marked done");
  });

  it("replaces my version with the worker's and undoes it", async () => {
    const core = await setup([
      review("r1", {
        mode: "isolated",
        state: "partly_approved",
        files: [change("NOTES.md"), change("a.rs")],
        not_copied: ["NOTES.md", "a.rs"],
      }),
    ]);
    core.conflicts = ["a.rs"];
    await open();
    $<HTMLButtonElement>("[data-testid=inbox-take]")?.click();
    await settle();
    expect(core.sent("review.take").map((c) => c.params)).toEqual([{ id: "r1" }]);
    expect($("[data-testid=inbox-outcome]")?.textContent?.trim()).toBe(
      "Replaced with the worker's version: NOTES.md. Not replaced, because the worker changed them after you approved: a.rs.",
    );
    core.restored = ["NOTES.md"];
    $<HTMLButtonElement>("[data-testid=inbox-undo-take]")?.click();
    await settle();
    expect(core.sent("review.undo_take").map((c) => c.params)).toEqual([{ id: "r1" }]);
    expect($("[data-testid=inbox-outcome]")?.textContent?.trim()).toBe("Your versions are back: NOTES.md.");
    expect($("[data-testid=inbox-undo-take]")).toBeNull();
  });

  it("says a worker's reject discarded files from its own copy", async () => {
    const core = await setup([review("r1", { mode: "isolated", files: [change("NOTES.md")] })]);
    core.restored = ["NOTES.md"];
    await open();
    button(/^Reject…$/)?.click();
    await settle();
    button(/^Reject the changes$/)?.click();
    await settle();
    expect($("[data-testid=inbox-outcome]")?.textContent?.trim()).toBe(
      "Rejected. Discarded from the worker's copy: NOTES.md.",
    );
  });

  it("says when a reject restored nothing, and undoes a reject", async () => {
    const core = await setup([review("r1", { files: [change("README.md", { by: "outside" })] })]);
    await open();
    button(/^Reject…$/)?.click();
    await settle();
    button(/^Reject the changes$/)?.click();
    await settle();
    expect($("[data-testid=inbox-outcome]")?.textContent?.trim()).toBe(
      "Rejected, but nothing was restored: no file here is known to be the agent's alone, so all of them stay as they are.",
    );
    // The pressed button is gone; focus lands on the undo instead of the page.
    expect(document.activeElement?.textContent).toBe("Undo reject");
    core.restored = ["src/a.rs"];
    button(/^Undo reject$/)?.click();
    await settle();
    expect(core.sent("review.undo_reject").map((c) => c.params)).toEqual([{ id: "r1" }]);
    expect($("[data-testid=inbox-outcome]")?.textContent?.trim()).toBe("Reject undone. Put back: src/a.rs.");
    expect(button(/^Undo reject$/)).toBeUndefined();
    expect(document.activeElement?.id).toBe("inbox-item-title");
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

    // While the person reviews, an agent cannot switch the item under their Approve.
    core.emit({ type: "orchestration.show", data: { project: "p1", target: { kind: "review", id: "r1" } } });
    await settle();
    expect($("[data-item][aria-current='true']")?.getAttribute("data-item")).toBe("r2");
    expect($("[data-testid=inbox] [role=status]")?.textContent).toContain("An agent asks you to look at");
  });

  it("says when nothing waits", async () => {
    await setup([]);
    await open();
    expect($("[data-testid=inbox]")?.textContent).toContain("No agent changes wait for review.");
    expect(document.activeElement?.id).toBe("inbox-title");
  });

  it("keeps the shortcuts of the panels behind it from acting, and shows a decided item's diffs", async () => {
    const core = await setup([review("r1", { files: [change("README.md"), change("src/a.ts")] })]);
    core.diffGone.set("src/a.ts", "the diff of a decided review is kept for at most 50 files");
    await open();
    press("KeyP", { altKey: true });
    await settle();
    expect(workspace.paletteOpen).toBe(false);
    button(/^Approve$/)?.click();
    await settle();
    expect($("[data-item=r1]")?.textContent).toContain("Approved");
    expect($("[data-testid=inbox-diff]")?.textContent).toContain("the readme");
    $("[data-file='src/a.ts']")?.click();
    await settle();
    expect($("[data-testid=inbox-diff] [role=alert]")?.textContent).toBe(
      "Could not load the diff of src/a.ts: the diff of a decided review is kept for at most 50 files",
    );
  });
});
