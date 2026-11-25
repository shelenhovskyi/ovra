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
    expect($("[data-item=r1]")?.classList.contains("flagged")).toBe(false);
    expect($("[data-testid=inbox-diff]")?.textContent).toContain("the readme");
    expect([...target.querySelectorAll("[data-testid=file-marker]")].map((m) => m.textContent)).toEqual([
      "Not changed by the agent's file tools",
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

  it("keeps a direct item's protected files only after each one's diff was shown", async () => {
    const core = await setup([
      review("r1", {
        files: [
          change("README.md"),
          change(".claude/settings.json", { protected: true }),
          change(".envrc", { protected: true, by: "outside" }),
        ],
      }),
    ]);
    await open();
    // The first protected file is what opens, so its diff is on screen.
    expect($("[data-file='.claude/settings.json']")?.getAttribute("aria-current")).toBe("true");
    expect($("[data-file='.claude/settings.json'] .marker.protected")?.textContent).toBe(
      "Can run code or change agent rules",
    );
    expect($("[data-file='.envrc'] [data-testid=file-reason]")?.textContent).toBe(
      "runs commands when you enter the folder with direnv",
    );
    expect($("[data-file='README.md'] .marker")).toBeNull();
    expect($("[data-item=r1] [data-testid=item-protected]")?.textContent).toBe(
      "2 files can run code or change agent rules",
    );
    expect($("[data-item=r1]")?.classList.contains("flagged")).toBe(true);
    expect($("[data-testid=inbox-protected]")?.textContent).toContain(
      "Reject leaves .envrc as it is: changed outside the agent's file tools",
    );
    expect($("[data-testid=inbox-protected-why]")?.textContent).toContain("each of these: .envrc.");
    expect(button(/^Approve$/)?.getAttribute("aria-disabled")).toBe("true");
    expect(button(/^Approve$/)?.getAttribute("aria-describedby")).toBe("inbox-protected inbox-protected-why");
    button(/^Approve$/)?.click();
    await settle();
    expect(core.sent("review.approve")).toEqual([]);

    $<HTMLButtonElement>("[data-file='.envrc']")?.click();
    await settle();
    expect($("[data-testid=inbox-protected-why]")).toBeNull();
    expect(button(/^Approve$/)?.getAttribute("aria-disabled")).toBe("false");
    expect(button(/^Approve$/)?.getAttribute("aria-describedby")).toBe("inbox-protected");
    button(/^Approve$/)?.click();
    await settle();
    expect(core.sent("review.approve").map((c) => c.params)).toEqual([
      { id: "r1", commit: null, include_protected: true },
    ]);
  });

  it("shows an item with only changed git hooks and config, and warns before Approve", async () => {
    await setup([
      review("r1", { files: [], git_changes: [{ path: ".git/hooks/pre-commit", kind: "added" }] }),
      review("r2", {
        state: "approved",
        git_changes: [{ path: "/home/me/.gitconfig", kind: "modified" }],
        git_unchecked: true,
      }),
    ]);
    expect($("[data-testid=inbox-button]")?.textContent).toContain("(1 waiting)");
    await open();
    expect($("[data-item=r1] [data-testid=item-git]")?.textContent).toBe("1 git hook or config file changed");
    expect($("[data-item=r1]")?.textContent).toContain("No changed files");
    expect($("[data-item=r1]")?.classList.contains("flagged")).toBe(true);
    expect($("[data-testid=inbox-git]")?.textContent).toContain(".git/hooks/pre-commit (added)");
    expect($("[data-testid=inbox-git]")?.textContent).toContain("neither applies nor undoes them");
    expect(button(/^Approve$/)?.getAttribute("aria-describedby")).toBe("inbox-git");
    expect($("[data-testid=inbox-diff]")?.textContent).toContain("Only git hooks or config changed");

    // A decided item keeps the list it was decided with, and says what was not checked.
    $<HTMLButtonElement>("[data-item=r2]")?.click();
    await settle();
    expect($("[data-testid=inbox-git]")?.textContent).toContain("/home/me/.gitconfig (modified)");
    expect($("[data-testid=inbox-git]")?.textContent).toContain(
      "Ovra could not check git hooks and config for this item",
    );
  });

  const guarded = () => [
    change("NOTES.md"),
    change(".envrc", { protected: true }),
    change(".mcp.json", { protected: true }),
  ];
  const include = () => $<HTMLInputElement>("[data-testid=inbox-include-protected]");

  it("leaves a worker's protected files out unless included", async () => {
    const core = await setup([review("r1", { session: "s2", mode: "isolated", fingerprint: "f1", files: guarded() })]);
    await open();
    expect(include()?.disabled).toBe(true);
    expect(include()?.parentElement?.textContent).toContain("Also apply protected files: .envrc, .mcp.json");
    expect($("[data-testid=inbox-include-why]")?.textContent).toContain("look at the diff of each first: .mcp.json.");
    // Approve without them: they are not copied, and the outcome says so.
    button(/^Approve$/)?.click();
    await settle();
    expect(core.sent("review.approve").map((c) => c.params)).toEqual([{ id: "r1", commit: null, fingerprint: "f1" }]);
    expect($("[data-testid=inbox-outcome]")?.textContent).toContain(
      "Not copied, as files that can run code or change agent rules: .envrc, .mcp.json.",
    );
    expect($("[data-testid=inbox-not-copied]")?.textContent).toContain(
      "because they can run code or change agent rules: .envrc, .mcp.json",
    );
    // Replacing them with the worker's version waits for a look at each too.
    expect($("[data-testid=inbox-take]")?.getAttribute("aria-disabled")).toBe("true");
    expect($("[data-testid=inbox-take-why]")?.textContent).toContain(".mcp.json");
    $<HTMLButtonElement>("[data-testid=inbox-take]")?.click();
    await settle();
    expect(core.sent("review.take")).toEqual([]);
  });

  it("includes a worker's protected files once each one's diff was shown", async () => {
    const core = await setup([review("r2", { session: "s3", mode: "isolated", fingerprint: "f2", files: guarded() })]);
    await open();
    $<HTMLButtonElement>("[data-file='.mcp.json']")?.click();
    await settle();
    expect(include()?.disabled).toBe(false);
    include()?.click();
    flushSync();
    button(/^Approve$/)?.click();
    await settle();
    expect(core.sent("review.approve")[0]?.params).toEqual({
      id: "r2",
      commit: null,
      fingerprint: "f2",
      include_protected: true,
    });
  });

  it("replaces a protected file that waited at approve only after its diff was shown, and says so", async () => {
    const core = await setup([
      review("r3", {
        session: "s3",
        mode: "isolated",
        state: "partly_approved",
        files: [change(".mcp.json", { protected: true }), change(".envrc", { protected: true, blocked: true })],
        not_copied: [".envrc", ".mcp.json"],
      }),
    ]);
    // `.mcp.json` opens first, so its diff is seen; `.envrc` waited for the
    // direct agent at approve, and nobody has looked at it yet.
    await open();
    expect($("[data-testid=inbox-take]")?.getAttribute("aria-disabled")).toBe("true");
    expect($("[data-testid=inbox-take-why]")?.textContent).toContain(".envrc");
    $<HTMLButtonElement>("[data-testid=inbox-take]")?.click();
    await settle();
    expect(core.sent("review.take")).toEqual([]);

    $<HTMLButtonElement>("[data-file='.envrc']")?.click();
    await settle();
    expect($("[data-testid=inbox-take]")?.getAttribute("aria-disabled")).toBe("false");
    $<HTMLButtonElement>("[data-testid=inbox-take]")?.click();
    await settle();
    expect(core.sent("review.take").map((c) => c.params)).toEqual([{ id: "r3", include_protected: true }]);
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

  it("does not approve a worker's files when the diff on screen is older than them", async () => {
    const core = await setup([review("r1", { mode: "isolated", fingerprint: "f2" })]);
    await open();
    // The item moved on to f2 while the diff on screen was read at f1.
    core.diffFingerprint = "f1";
    core.emit({ type: "review.changed", data: { project: "p1", id: "r1" } });
    await settle();
    button(/^Approve$/)?.click();
    await settle();
    expect(core.sent("review.approve")).toEqual([]);
    expect($("[data-testid=inbox-stale]")?.textContent).toContain("nothing was approved");
    const diffs = core.sent("review.diff").length;
    expect(diffs).toBeGreaterThan(2);
    core.diffFingerprint = null;
    core.emit({ type: "review.changed", data: { project: "p1", id: "r1" } });
    await settle();
    button(/^Approve$/)?.click();
    await settle();
    expect(core.sent("review.approve").map((c) => c.params)).toEqual([{ id: "r1", commit: null, fingerprint: "f2" }]);
    expect($("[data-testid=inbox-stale]")).toBeNull();
  });

  it("does not approve a worker's files without their diff on screen", async () => {
    const core = await setup([review("r1", { mode: "isolated", fingerprint: "f1" })]);
    core.diffGone.set("README.md", "gone");
    core.emit({ type: "review.changed", data: { project: "p1", id: "r1" } });
    await open();
    expect(button(/^Approve$/)?.getAttribute("aria-disabled")).toBe("true");
    expect($("[data-testid=inbox-diff-why]")?.textContent).toContain("did not load");
    button(/^Approve$/)?.click();
    await settle();
    expect(core.sent("review.approve")).toEqual([]);
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
    expect($("#inbox-reject-hint")?.textContent).toContain(
      "Files changed any other way (by you, another program or the agent's commands) stay as they are",
    );
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
    expect($("[data-testid=inbox-outcome]")?.textContent?.trim()).toBe("Marked done. The files stay as they are now.");
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
    core.conflicts = [];
    $<HTMLButtonElement>("[data-testid=inbox-undo-take]")?.click();
    await settle();
    expect(core.sent("review.undo_take").map((c) => c.params)).toEqual([{ id: "r1" }]);
    expect($("[data-testid=inbox-outcome]")?.textContent?.trim()).toBe("Your versions are back: NOTES.md.");

    expect($("[data-testid=inbox-undo-take]")).toBeNull();
  });

  it("names the files an undo left because they changed after the replace", async () => {
    const core = await setup([review("r1", { mode: "isolated", state: "approved", can_undo_take: true })]);
    core.conflicts = ["README.md"];
    await open();
    $<HTMLButtonElement>("[data-testid=inbox-undo-take]")?.click();
    await settle();
    expect($("[data-testid=inbox-outcome]")?.textContent?.trim()).toBe(
      "Not put back, because you changed them after the replace: README.md.",
    );
  });

  it("keeps a full replace", async () => {
    const core = await setup([
      review("r1", {
        mode: "isolated",
        state: "partly_approved",
        files: [change("NOTES.md")],
        not_copied: ["NOTES.md"],
      }),
      review("r2", {
        mode: "isolated",
        state: "partly_approved",
        files: [change("a.rs", { blocked: true })],
        not_copied: ["a.rs"],
      }),
    ]);
    await open();
    $<HTMLButtonElement>("[data-testid=inbox-take]")?.click();
    await settle();
    $<HTMLButtonElement>("[data-testid=inbox-keep-take]")?.click();
    await settle();
    expect(core.sent("review.resolve").map((c) => c.params)).toEqual([{ id: "r1" }]);
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
      "Rejected, but nothing was restored: no file here was changed only by the agent's file tools, so all of them stay as they are.",
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
