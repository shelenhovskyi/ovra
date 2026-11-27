import { flushSync, mount, unmount } from "svelte";
import { afterEach, beforeEach, describe, expect, it } from "vitest";
import { agentSessions } from "../agents/sessions.svelte";
import type { SessionInfo, Task } from "../lib/protocol";
import { CoreError } from "../lib/transport";
import { mainView } from "../shell/main-view.svelte";
import { Workspace } from "../shell/workspace.svelte";
import { change, FakeReviewCore, review } from "./fake-review";
import { openInboxFor } from "./open";
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
  mainView.close(false);
  if (app) unmount(app);
  app = null;
  target.remove();
});

async function settle() {
  for (let i = 0; i < 4; i++) {
    flushSync();
    await Promise.resolve();
  }
  await new Promise((resolve) => setImmediate(resolve));
  flushSync();
}

async function setup(items = [review("r1")], before?: (core: FakeReviewCore) => Promise<void>) {
  const core = new FakeReviewCore();
  core.items = items;
  await before?.(core);
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
    expect($("[data-testid=inbox-button]")?.getAttribute("aria-label")).toBe("Reviews, 2 waiting");
    await open();
    expect($("[data-testid=inbox-button]")?.getAttribute("aria-pressed")).toBe("true");
    expect(document.activeElement?.getAttribute("data-item")).toBe("r1");
    expect($("[data-item=r1]")?.classList.contains("flagged")).toBe(false);
    expect($("[data-testid=inbox-diff]")?.textContent).toContain("the readme");
    expect([...target.querySelectorAll("[data-testid=file-marker]")].map((m) => m.textContent)).toEqual([
      "Not changed by the agent's file tools",
    ]);

    press("ArrowDown");
    await settle();
    expect(document.activeElement?.getAttribute("data-item")).toBe("r2");
    // Short titles: the worker's own name; its task heads the group in the list.
    expect($("[data-testid=inbox-item] h3")?.textContent).toBe("Session s2");
    expect($("[data-file='src/a.ts']")?.textContent).toContain("Waits for the direct agent");
    expect($("[data-testid=inbox-item]")?.textContent).toContain("into your project folder");

    press("Escape");
    await settle();
    expect($("[data-testid=inbox]")).toBeNull();
    expect(document.activeElement).toBe($("[data-testid=elsewhere]"));
  });

  it("shows the check before review as one quiet line, with its log behind a click", async () => {
    const core = await setup([
      review("r1", {
        session: "s2",
        mode: "isolated",
        check: { state: "failed", command: "just check", detail: "exit code 1" },
      }),
    ]);
    core.checkLogs.set("r1", ["\u001b[31merror\u001b[0m: 1 test failed", "done"]);
    await open();
    const line = $("[data-testid=inbox-check]");
    expect(line?.textContent?.replace(/\s+/g, " ").trim()).toBe("Check failed: exit code 1 Show log Run again");
    expect(line?.querySelector("span")?.title).toBe("just check");
    expect($("[data-testid=inbox-check-log]")).toBeNull();
    button(/^Show log$/)?.click();
    await settle();
    expect($("[data-testid=inbox-check-log]")?.textContent).toBe("error: 1 test failed\ndone");
    expect(core.calls.filter((c) => c.method === "review.check_log")).toHaveLength(1);

    // Passed on the next run: the core says the item changed, the open log is read again.
    core.items = [{ ...core.items[0], check: { state: "passed", command: "just check" } } as (typeof core.items)[0]];
    core.checkLogs.set("r1", ["ok"]);
    core.emit({ type: "review.changed", data: { project: "p1", id: "r1" } });
    await settle();
    expect($("[data-testid=inbox-check]")?.textContent).toContain("Check passed");
    expect($("[data-testid=inbox-check-log]")?.textContent).toBe("ok");
    button(/^Hide log$/)?.click();
    await settle();
    expect($("[data-testid=inbox-check-log]")).toBeNull();
  });

  it("offers to run a check again that judged earlier files, or failed", async () => {
    const core = await setup([review("r1", { check: { state: "passed", command: "just check", stale: true } })]);
    await open();
    const line = $("[data-testid=inbox-check]");
    expect(line?.textContent).toContain("Check passed, on earlier files");
    button(/^Run again$/)?.click();
    await settle();
    expect(core.calls.filter((c) => c.method === "review.run_check")).toEqual([
      { method: "review.run_check", params: { id: "r1" } },
    ]);

    // Passed on these files: nothing to run again.
    core.items = [{ ...core.items[0], check: { state: "passed", command: "just check" } } as (typeof core.items)[0]];
    core.emit({ type: "review.changed", data: { project: "p1", id: "r1" } });
    await settle();
    expect(button(/^Run again$/)).toBeFalsy();
  });

  it("points at the tests and checks the agent weakened, and opens their diff", async () => {
    const core = await setup([review("r1", { files: [change("README.md"), change("src/a.test.ts")] })], async (c) => {
      c.warnings.set("r1", [
        { kind: "test_skipped", path: "src/a.test.ts", line: 4, text: "it.skip('adds', () => {" },
        { kind: "check_config", path: "tsconfig.json" },
      ]);
    });
    await open();
    const closer = $("[data-testid=inbox-closer]");
    expect(closer?.querySelector("h4")?.textContent).toBe("Look closer");
    expect(closer?.textContent).toContain("The agent changed tests and checks.");
    const rows = [...(closer?.querySelectorAll("li") ?? [])].map((li) => li.textContent?.replace(/\s+/g, " ").trim());
    expect(rows).toEqual([
      "Skipped a test a.test.ts:4 it.skip('adds', () => {",
      "Changed how the project is checked tsconfig.json",
    ]);
    closer?.querySelector<HTMLButtonElement>("li button")?.click();
    await settle();
    expect(core.calls.some((c) => c.method === "review.diff" && c.params.path === "src/a.test.ts")).toBe(true);
  });

  it("shows nothing to look closer at when the diff leaves tests and checks alone", async () => {
    await setup([review("r1")]);
    await open();
    expect($("[data-testid=inbox-closer]")).toBeNull();
  });

  it("shows nothing about a check when the project has none", async () => {
    await setup([review("r1", { session: "s2", mode: "isolated" })]);
    await open();
    expect($("[data-testid=inbox-check]")).toBeNull();
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
      "Discard leaves .envrc as it is: changed outside the agent's file tools",
    );
    expect($("[data-testid=inbox-protected-why]")?.textContent).toContain("each of these: .envrc.");
    expect(button(/^Keep changes$/)?.getAttribute("aria-disabled")).toBe("true");
    expect(button(/^Keep changes$/)?.getAttribute("aria-describedby")).toBe("inbox-protected inbox-protected-why");
    button(/^Keep changes$/)?.click();
    await settle();
    expect(core.sent("review.approve")).toEqual([]);

    $<HTMLButtonElement>("[data-file='.envrc']")?.click();
    await settle();
    expect($("[data-testid=inbox-protected-why]")).toBeNull();
    expect(button(/^Keep changes$/)?.getAttribute("aria-disabled")).toBe("false");
    expect(button(/^Keep changes$/)?.getAttribute("aria-describedby")).toBe("inbox-protected");
    button(/^Keep changes$/)?.click();
    await settle();
    expect(core.sent("review.approve").map((c) => c.params)).toEqual([
      { id: "r1", commit: null, include_protected: true },
    ]);
  });

  it("counts a direct item's protected file as seen only in the version the list shows", async () => {
    const core = await setup([review("r1", { fingerprint: "d2", files: [change("CLAUDE.md", { protected: true })] })]);
    // The diff on screen was read before the file changed again.
    core.diffFingerprint = "d1";
    core.emit({ type: "review.changed", data: { project: "p1", id: "r1" } });
    await open();
    expect($("[data-testid=inbox-protected-why]")?.textContent).toContain("CLAUDE.md");
    button(/^Keep changes$/)?.click();
    await settle();
    expect(core.sent("review.approve")).toEqual([]);
    core.diffFingerprint = null;
    core.emit({ type: "review.changed", data: { project: "p1", id: "r1" } });
    await settle();
    expect($("[data-testid=inbox-protected-why]")).toBeNull();
    button(/^Keep changes$/)?.click();
    await settle();
    expect(core.sent("review.approve").map((c) => c.params)).toEqual([
      { id: "r1", commit: null, fingerprint: "d2", include_protected: true },
    ]);
  });

  it("shows an item with only changed git hooks and config, and warns before Keep changes", async () => {
    await setup([
      review("r1", { files: [], git_changes: [{ path: ".git/hooks/pre-commit", kind: "added" }] }),
      review("r2", {
        state: "approved",
        git_changes: [{ path: "/home/me/.gitconfig", kind: "modified" }],
        git_unchecked: true,
      }),
    ]);
    expect($("[data-testid=inbox-button]")?.getAttribute("aria-label")).toBe("Reviews, 1 waiting");
    await open();
    expect($("[data-item=r1] [data-testid=item-git]")?.textContent).toBe("1 git hook or config file changed");
    expect($("[data-item=r1]")?.textContent).toContain("No changed files");
    expect($("[data-item=r1]")?.classList.contains("flagged")).toBe(true);
    expect($("[data-testid=inbox-git]")?.textContent).toContain(".git/hooks/pre-commit (added)");
    expect($("[data-testid=inbox-git]")?.textContent).toContain("leave them as they are");
    expect(button(/^Keep changes$/)?.getAttribute("aria-describedby")).toBe("inbox-git");
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
    expect(include()?.getAttribute("aria-disabled")).toBe("true");
    expect(include()?.disabled).toBe(false);
    expect(include()?.getAttribute("aria-describedby")).toBe("inbox-include-why");
    // Ticking it before each diff was seen does nothing.
    include()?.click();
    await settle();
    expect(include()?.checked).toBe(false);
    expect(include()?.parentElement?.textContent).toContain("Also apply protected files: .envrc, .mcp.json");
    expect($("[data-testid=inbox-include-why]")?.textContent).toContain("look at the diff of each first: .mcp.json.");
    // Approve without them: they are not copied, and the note below says why, once.
    button(/^Keep changes$/)?.click();
    await settle();
    expect(core.sent("review.approve").map((c) => c.params)).toEqual([{ id: "r1", commit: null, fingerprint: "f1" }]);
    expect($("[data-testid=inbox-outcome]")?.textContent).toContain("Some files were not copied: see why below.");
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

  it("names both paths of a rename away from a protected path as left out for that reason", async () => {
    const core = await setup([
      review("r1", {
        session: "s2",
        mode: "isolated",
        fingerprint: "f1",
        files: [change("notes.md", { kind: "renamed", from: "CLAUDE.md", protected: true })],
      }),
    ]);
    // Like the core, which leaves both paths of the rename.
    core.conflicts = ["CLAUDE.md"];
    await open();
    button(/^Keep changes$/)?.click();
    await settle();
    expect($("[data-testid=inbox-outcome]")?.textContent?.trim()).toBe(
      "Kept. Some files were not copied: see why below.",
    );
    expect($("[data-testid=inbox-not-copied]")?.textContent).toContain(
      "because they can run code or change agent rules: CLAUDE.md, notes.md.",
    );
  });

  it("includes a worker's protected files once each one's diff was shown", async () => {
    const core = await setup([review("r2", { session: "s3", mode: "isolated", fingerprint: "f2", files: guarded() })]);
    await open();
    $<HTMLButtonElement>("[data-file='.mcp.json']")?.click();
    await settle();
    expect(include()?.getAttribute("aria-disabled")).toBe("false");
    include()?.click();
    flushSync();
    button(/^Keep changes$/)?.click();
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

  it("shows the three-line summary from the item, else from the agent's last answer", async () => {
    const rows = () =>
      [...target.querySelectorAll("[data-testid=task-summary] .row")].map((r) =>
        r.textContent?.replace(/\s+/g, " ").trim(),
      );
    const core = await setup([
      review("r1", { summary: "Done: fixed the typo\nChecked: tests passed" }),
      review("r2", { session: "s2" }),
    ]);
    core.transcripts.set("s2", [{ kind: "assistant_text", text: "Done: wrote docs\nLeft: the screenshots" }]);
    await open();
    expect(rows()).toEqual(["Done fixed the typo", "Checked tests passed", "Left Not said"]);
    expect(target.textContent).not.toContain("Agent's summary");
    // The commit message offered is what was done.
    $<HTMLInputElement>("[data-testid=inbox] input[type=checkbox]")?.click();
    flushSync();
    expect($<HTMLInputElement>("[data-testid=inbox] input[type=text]")?.value).toBe("Fixed the typo");
    $<HTMLButtonElement>("[data-item=r2]")?.click();
    await settle();
    expect(rows()).toEqual(["Done wrote docs", "Checked Not said", "Left the screenshots"]);
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
    expect(button(/^Keep and commit$/)?.getAttribute("aria-describedby")).toBe("inbox-commit-why");
    expect($("#inbox-commit-why")?.textContent).toBe("Write a commit message to keep and commit.");
    button(/^Keep and commit$/)?.click();
    await settle();
    expect(core.sent("review.approve")).toEqual([]);
    if (message) message.value = "Fix the typo";
    message?.dispatchEvent(new Event("input", { bubbles: true }));
    flushSync();
    button(/^Keep and commit$/)?.click();
    await settle();
    expect(core.sent("review.approve").map((c) => c.params)).toEqual([{ id: "r1", commit: "Fix the typo" }]);
    expect($("[data-testid=inbox-outcome]")?.textContent?.trim()).toBe("Kept. Committed as “Fix the typo”.");
    expect(button(/^Keep/)).toBeUndefined();
    expect(document.activeElement?.id).toBe("inbox-item-title");
  });

  it("moves on to the next waiting item after Keep changes, focus too, and says Kept once", async () => {
    const core = await setup([review("r1"), review("r2", { session: "s2" })]);
    await open();
    expect(document.activeElement?.getAttribute("data-item")).toBe("r1");
    // Left and right move too, for the narrow view's row of items.
    press("ArrowRight");
    flushSync();
    expect(document.activeElement?.getAttribute("data-item")).toBe("r2");
    press("ArrowLeft");
    await settle();
    expect(document.activeElement?.getAttribute("data-item")).toBe("r1");
    button(/^Keep changes$/)?.click();
    await settle();
    expect(core.sent("review.approve").map((c) => c.params.id)).toEqual(["r1"]);
    expect($("[data-item][aria-current=true]")?.getAttribute("data-item")).toBe("r2");
    expect(document.activeElement?.getAttribute("data-item")).toBe("r2");
    expect($("[data-item=r1]")?.textContent).toContain("Kept");
    expect($("[data-testid=inbox-outcome]")).toBeNull();
    expect($("[role=status]")?.textContent).toMatch(/^Kept /);

    // The last one stays, its state said beside its title only.
    button(/^Keep changes$/)?.click();
    await settle();
    expect($("[data-item][aria-current=true]")?.getAttribute("data-item")).toBe("r2");
    expect($("[data-testid=inbox-outcome]")).toBeNull();
    expect(document.activeElement?.id).toBe("inbox-item-title");
  });

  it("undoes a keep in one press, from the kept item or from the next one", async () => {
    const core = await setup([review("r1"), review("r2", { session: "s2" })]);
    await open();
    button(/^Keep changes$/)?.click();
    await settle();
    // On to r2: one line still offers to take the keep of r1 back.
    expect($("[data-testid=inbox-kept-line]")?.textContent).toMatch(/^\s*Kept .*\.\s*Undo keep\s*$/);
    button(/^Undo keep$/)?.click();
    await settle();
    expect(core.sent("review.undo_keep").map((c) => c.params)).toEqual([{ id: "r1" }]);
    expect($("[data-item][aria-current=true]")?.getAttribute("data-item")).toBe("r1");
    expect($("[data-testid=inbox-outcome]")?.textContent?.trim()).toBe("Keep undone. The changes wait for you again.");
    expect(button(/^Keep changes$/)).toBeDefined();
    expect($("[data-testid=inbox-kept-line]")).toBeNull();

    // Kept again, and undone from the item itself.
    button(/^Keep changes$/)?.click();
    await settle();
    $("[data-item=r1]")?.click();
    await settle();
    expect($("[data-testid=inbox-kept-line]")).toBeNull();
    $<HTMLButtonElement>("[data-testid=inbox-undo-keep]")?.click();
    await settle();
    expect(core.sent("review.undo_keep")).toHaveLength(2);
  });

  it("offers no undo for a keep that committed", async () => {
    await setup([review("r1", { summary: "Fix the typo" })]);
    await open();
    $<HTMLInputElement>("[data-testid=inbox] input[type=checkbox]")?.click();
    flushSync();
    button(/^Keep and commit$/)?.click();
    await settle();
    expect($("[data-testid=inbox-undo-keep]")).toBeNull();
    expect($("[data-testid=inbox-write-tests]")).not.toBeNull();
  });

  it("starts an agent that writes tests for the kept files, and shows it", async () => {
    const core = await setup([review("r1", { files: [change("src/a.ts"), change("old.ts", { kind: "deleted" })] })]);
    await open();
    button(/^Keep changes$/)?.click();
    await settle();
    $<HTMLButtonElement>("[data-testid=inbox-write-tests]")?.click();
    await settle();
    const [start] = core.sent("agents.start");
    expect(start?.params).toMatchObject({ project: "p1", mode: "direct" });
    expect(start?.params.prompt).toContain("- src/a.ts");
    expect(start?.params.prompt).not.toContain("old.ts");
    expect(mainView.current).toBeNull();
  });

  it("lists the files to look at most carefully first, and names the step that wrote each part", async () => {
    await setup(
      [review("r1", { files: [change("README.md"), change("src/a.ts"), change("src/old.ts", { kind: "deleted" })] })],
      async (core) => {
        core.transcripts.set("s1", [
          { kind: "assistant_text", text: "Fix the readme typo." },
          {
            kind: "tool_use",
            id: "t1",
            name: "Edit",
            input: { file_path: "/demo/README.md", old_string: "teh readme", new_string: "the readme" },
            call: { kind: "edit", paths: ["/demo/README.md"] },
          },
        ]);
      },
    );
    await open();
    const listed = [...target.querySelectorAll<HTMLElement>("[data-file]")].map((b) => b.dataset.file);
    expect(listed).toEqual(["src/old.ts", "src/a.ts", "README.md"]);
    expect($("[data-file='src/old.ts'] [data-testid=file-risk]")?.textContent).toBe("Deletes a file");
    expect($("[data-file='src/old.ts']")?.getAttribute("aria-current")).toBe("true");
    $("[data-file='README.md']")?.click();
    await settle();
    expect($("[data-testid=hunk-step]")?.textContent).toBe("Step 1 (Edit): “Fix the readme typo.”");
    $("[data-file='src/a.ts']")?.click();
    await settle();
    expect($("[data-testid=hunk-step]")).toBeNull();
  });

  it("says an item is one of several attempts, and after Keep discards the others", async () => {
    const solo = (id: string, session: string, attempt_of?: string): Task => ({
      id,
      project: "p1",
      coordinator: null,
      workers: [{ session, name: "agent", branch: `ovra/${id}/agent`, worktree: `/wt/${id}`, kept: null }],
      limits: { max_workers: 1, budget_micro_usd: null },
      spent_micro_usd: 0,
      ...(attempt_of ? { attempt_of } : {}),
    });
    const agent = (id: string, kind: SessionInfo["agent"]): SessionInfo => ({
      id,
      project: "p1",
      agent: kind,
      mode: "isolated",
      status: { state: "idle" },
      title: "Fix the typo",
      usage: { input_tokens: 0, output_tokens: 0, cost_micro_usd: null },
      resumable: false,
      billing: null,
      sandbox: null,
      sandbox_reported: false,
      account: null,
    });
    const items = [
      review("r1", { session: "s1", mode: "isolated" }),
      review("r9", { session: "s9", mode: "direct" }),
      review("r2", { session: "s2", mode: "isolated" }),
    ];
    const core = await setup(items, async (c) => {
      c.listed = [agent("s1", "claude"), agent("s2", "codex"), agent("s3", "claude")];
      c.tasks = [solo("t1", "s1"), solo("t2", "s2", "t1"), solo("t3", "s3", "t1")];
      await agentSessions(c).load("p1");
    });
    await open();
    // Attempts stay together under the task's name, with who made each.
    const names = [...target.querySelectorAll("[data-item] .name")].map((e) => e.textContent);
    expect(names).toEqual(["Attempt 1 (Claude)", "Attempt 2 (Codex)", "Session s9"]);
    expect($("[data-testid=inbox-attempts]")?.textContent).toContain("One of 3 attempts at this task");

    button(/^Keep changes$/)?.click();
    await settle();
    // On to the next attempt, which offers to discard the rest: it, and the third that never changed a file.
    expect($("[data-item][aria-current=true]")?.getAttribute("data-item")).toBe("r2");
    expect($("[data-testid=inbox-attempts]")?.textContent).toContain("You kept Attempt 1 (Claude).");
    expect($("[data-testid=inbox-discard-attempts]")?.textContent).toBe("Discard the other 2 attempts");
    $<HTMLButtonElement>("[data-testid=inbox-discard-attempts]")?.click();
    await settle();
    expect(core.sent("review.reject").map((c) => c.params.id)).toEqual(["r2"]);
    expect(core.sent("agents.stop").map((c) => c.params.session)).toEqual(["s2", "s3"]);
    expect($("[role=status]")?.textContent).toBe("Discarded the other 2 attempts.");
  });

  it("does not approve a worker's files when the diff on screen is older than them", async () => {
    const core = await setup([review("r1", { mode: "isolated", fingerprint: "f2" })]);
    await open();
    // The item moved on to f2 while the diff on screen was read at f1.
    core.diffFingerprint = "f1";
    core.emit({ type: "review.changed", data: { project: "p1", id: "r1" } });
    await settle();
    button(/^Keep changes$/)?.click();
    await settle();
    expect(core.sent("review.approve")).toEqual([]);
    expect($("[data-testid=inbox-stale]")?.textContent).toContain("nothing was kept");
    const diffs = core.sent("review.diff").length;
    expect(diffs).toBeGreaterThan(2);
    core.diffFingerprint = null;
    core.emit({ type: "review.changed", data: { project: "p1", id: "r1" } });
    await settle();
    button(/^Keep changes$/)?.click();
    await settle();
    expect(core.sent("review.approve").map((c) => c.params)).toEqual([{ id: "r1", commit: null, fingerprint: "f2" }]);
    expect($("[data-testid=inbox-stale]")).toBeNull();
  });

  it("does not approve a worker's files without their diff on screen", async () => {
    const core = await setup([review("r1", { mode: "isolated", fingerprint: "f1" })]);
    core.diffGone.set("README.md", "gone");
    core.emit({ type: "review.changed", data: { project: "p1", id: "r1" } });
    await open();
    expect(button(/^Keep changes$/)?.getAttribute("aria-disabled")).toBe("true");
    expect($("[data-testid=inbox-diff-why]")?.textContent).toContain("did not load");
    button(/^Keep changes$/)?.click();
    await settle();
    expect(core.sent("review.approve")).toEqual([]);
  });

  it("discards after a confirmation that says what stays, and lists what was left", async () => {
    const core = await setup([review("r1", { files: [change("README.md", { by: "both" })] })]);
    core.conflicts = ["README.md"];
    core.restored = ["src/a.rs"];
    await open();
    button(/^Discard…$/)?.click();
    await settle();
    expect(core.sent("review.reject")).toEqual([]);
    // The step starts on the safe choice; Escape leaves the step, not the inbox.
    expect(document.activeElement?.textContent).toBe("Cancel");
    expect($("#inbox-reject-hint")?.textContent).toContain(
      "Files changed any other way (by you, another program or the agent's commands) stay as they are",
    );
    press("Escape");
    await settle();
    expect($("[data-testid=inbox]")).not.toBeNull();
    expect($("#inbox-reject-hint")).toBeNull();
    expect(document.activeElement?.textContent).toBe("Discard…");
    // Closing Reviews by its frame ends the step too: it is not waiting, armed, when the view comes back.
    button(/^Discard…$/)?.click();
    await settle();
    mainView.close(false);
    await settle();
    expect($("[data-testid=inbox]")).toBeNull();
    await open();
    expect($("#inbox-reject-hint")).toBeNull();
    expect(button(/^Discard the changes$/)).toBeUndefined();
    button(/^Discard…$/)?.click();
    await settle();
    button(/^Discard the changes$/)?.click();
    await settle();
    expect(core.sent("review.reject")).toHaveLength(1);
    expect($("[data-testid=inbox-outcome]")?.textContent?.trim()).toBe(
      "Discarded. Restored: src/a.rs. Kept as you changed them: README.md.",
    );
  });

  it("warns that the project moved on, brings its changes in, and asks the agent to resolve conflicts", async () => {
    const behind = { files: 3, clashes: ["src/a.ts"] };
    const core = await setup([review("r1", { mode: "isolated", files: [change("src/a.ts")], behind })]);
    await open();
    expect($("[data-testid=inbox-behind-text]")?.textContent).toContain("also in a.ts, which the agent changed too");
    $<HTMLButtonElement>("[data-testid=inbox-update]")?.click();
    await settle();
    expect(core.sent("review.update").map((c) => c.params)).toEqual([{ id: "r1" }]);
    expect($("[data-testid=inbox-updated]")?.textContent).toContain("a.ts changed in the same place on both sides");

    $<HTMLButtonElement>("[data-testid=inbox-update-conflicts]")?.click();
    await settle();
    expect(core.sent("review.update").map((c) => c.params)).toEqual([{ id: "r1" }, { id: "r1", conflicts: true }]);
    expect($("[data-testid=inbox-updated]")?.textContent).toContain("a.ts now has conflict markers");
    expect($("[data-testid=inbox-behind-text]")).toBeNull();
    $<HTMLButtonElement>("[data-testid=inbox-ask-resolve]")?.click();
    await settle();
    expect($<HTMLTextAreaElement>("#inbox-comment")?.value).toContain("Resolve the conflict markers in src/a.ts");
    expect(core.sent("review.comment")).toHaveLength(0);
  });

  it("says which of a worker's files were not copied, keeps the item waiting, and marks it done", async () => {
    const core = await setup([review("r1", { mode: "isolated", files: [change("NOTES.md"), change("a.rs")] })]);
    core.conflicts = ["NOTES.md"];
    await open();
    button(/^Keep changes$/)?.click();
    await settle();
    expect($("[data-testid=inbox-outcome]")?.textContent?.trim()).toBe("Kept. One file was not copied: see why below.");
    expect($("[data-item=r1]")?.textContent).toContain("Kept, not all copied");
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
      "Replaced with the worker's version: NOTES.md. Not replaced, because the worker changed them after you kept its changes: a.rs.",
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

  it("says a worker's discard removed files from its own copy", async () => {
    const core = await setup([review("r1", { mode: "isolated", files: [change("NOTES.md")] })]);
    core.restored = ["NOTES.md"];
    await open();
    button(/^Discard…$/)?.click();
    await settle();
    button(/^Discard the changes$/)?.click();
    await settle();
    expect($("[data-testid=inbox-outcome]")?.textContent?.trim()).toBe(
      "Discarded. Removed from the worker's copy: NOTES.md.",
    );
  });

  it("says when a discard restored nothing, and undoes a discard", async () => {
    const core = await setup([review("r1", { files: [change("README.md", { by: "outside" })] })]);
    await open();
    button(/^Discard…$/)?.click();
    await settle();
    button(/^Discard the changes$/)?.click();
    await settle();
    expect($("[data-testid=inbox-outcome]")?.textContent?.trim()).toBe(
      "Discarded, but nothing was restored: no file here was changed only by the agent's file tools, so all of them stay as they are.",
    );
    // The pressed button is gone; focus lands on the undo instead of the page.
    expect(document.activeElement?.textContent).toBe("Undo discard");
    core.restored = ["src/a.rs"];
    button(/^Undo discard$/)?.click();
    await settle();
    expect(core.sent("review.undo_reject").map((c) => c.params)).toEqual([{ id: "r1" }]);
    expect($("[data-testid=inbox-outcome]")?.textContent?.trim()).toBe("Discard undone. Put back: src/a.rs.");
    expect(button(/^Undo discard$/)).toBeUndefined();
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
    expect(button(/^Keep changes$/)).toBeDefined();
  });

  it("remembers a sent comment in AGENTS.md when asked", async () => {
    const core = await setup();
    await open();
    const field = $<HTMLTextAreaElement>("#inbox-comment");
    if (field) field.value = "Use the fake clock";
    field?.dispatchEvent(new Event("input", { bubbles: true }));
    $<HTMLInputElement>("[data-testid=inbox-remember]")?.click();
    button(/^Send comment/)?.click();
    await settle();
    expect(core.sent("review.comment").map((c) => c.params.text)).toEqual(["Use the fake clock"]);
    const written = core.sent("fs.write_file").map((c) => c.params);
    expect(written.map((w) => [w.path, w.expected_mtime_ms])).toEqual([["AGENTS.md", null]]);
    expect(written[0]?.text).toMatch(/## Rules\n\n- Use the fake clock\n$/);
    expect($<HTMLInputElement>("[data-testid=inbox-remember]")?.checked).toBe(false);
  });

  it("keeps a comment draft per item: Escape leaves the field, the next closes the inbox", async () => {
    await setup([review("r1"), review("r2", { files: [change("b.md")] })]);
    await open();
    const field = () => $<HTMLTextAreaElement>("#inbox-comment");
    field()?.focus();
    const typed = field();
    if (typed) typed.value = "please rename the file";
    typed?.dispatchEvent(new Event("input", { bubbles: true }));
    press("Escape");
    await settle();
    // With a draft in the field, Escape leaves the field only: the view stays.
    expect($("[data-testid=inbox]")).not.toBeNull();
    expect(document.activeElement?.id).toBe("inbox-item-title");
    expect(field()?.value).toBe("please rename the file");
    $<HTMLButtonElement>("[data-item=r2]")?.click();
    await settle();
    expect(field()?.value).toBe("");
    $<HTMLButtonElement>("[data-item=r1]")?.click();
    await settle();
    expect(field()?.value).toBe("please rename the file");
    press("Escape");
    await settle();
    expect($("[data-testid=inbox]")).toBeNull();
  });

  it("names the worker whose version of a shared file was kept first", async () => {
    const notes = () => [change("NOTES.md")];
    await setup([
      review("r3", { session: "s3", mode: "isolated", fingerprint: "f3", files: notes() }),
      review("r2", { session: "s2", mode: "isolated", fingerprint: "f2", files: notes() }),
      review("r1", {
        session: "s1",
        mode: "isolated",
        state: "partly_approved",
        files: notes(),
        not_copied: ["NOTES.md"],
      }),
      review("r0", { session: "s0", mode: "isolated", state: "approved", files: notes() }),
    ]);
    await open();
    // s0's version is kept: that is said on the file, once, instead of "also changed in another open review".
    expect($("[data-testid=inbox-shared]")).toBeNull();
    expect(
      [...document.querySelectorAll("[data-file='NOTES.md'] [data-testid=file-marker]")].map((m) => m.textContent),
    ).toEqual(["Session s0's version is already kept"]);
    $<HTMLButtonElement>("[data-item=r1]")?.click();
    await settle();
    expect($("[data-testid=inbox-not-copied]")?.textContent).toContain(
      "because you kept Session s0's different version first: NOTES.md.",
    );
    expect($("[data-file='NOTES.md']")?.textContent).toContain("Not copied: you kept Session s0's version");
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

  it("opens on the newest of the items several agents ask to show", async () => {
    const core = await setup([review("r1"), review("r2", { files: [change("b.md")] })]);
    for (const id of ["r1", "r2"]) {
      core.emit({ type: "orchestration.show", data: { project: "p1", target: { kind: "review", id } } });
      await settle();
    }
    await open();
    expect(document.activeElement?.getAttribute("data-item")).toBe("r1");
  });

  it("opens on the first waiting item of the sessions a Review press names", async () => {
    await setup([review("r1"), review("r2", { session: "s2" }), review("r3", { session: "s3" })]);
    $("[data-testid=elsewhere]")?.focus();
    expect(openInboxFor(["s4", "s3", "s2"])).toBe(true);
    await settle();
    expect(document.activeElement?.getAttribute("data-item")).toBe("r3");
  });

  it("says when nothing waits", async () => {
    await setup([]);
    await open();
    expect($("[data-testid=inbox]")?.textContent).toContain("Nothing to review");
    expect(document.activeElement?.textContent).toBe("Reviews");
  });

  it("shows a decided item's diffs", async () => {
    const core = await setup([review("r1", { files: [change("README.md"), change("docs/a.md")] })]);
    core.diffGone.set("docs/a.md", "the diff of a decided review is kept for at most 50 files");
    await open();
    button(/^Keep changes$/)?.click();
    await settle();
    expect($("[data-item=r1]")?.textContent).toContain("Kept");
    expect($("[data-testid=inbox-diff]")?.textContent).toContain("the readme");
    $("[data-file='docs/a.md']")?.click();
    await settle();
    expect($("[data-testid=inbox-diff] [role=alert]")?.textContent).toBe(
      "Could not load the diff of docs/a.md: the diff of a decided review is kept for at most 50 files",
    );
  });

  it("keeps, discards and comments on single parts of a file", async () => {
    const core = await setup([review("r1", { mode: "isolated", fingerprint: "f1" })]);
    await open();
    const part = () => $("[data-testid=hunk]");
    expect(part()?.getAttribute("aria-label")).toBe("Part 1 of 1, line 1");
    // Keep is the person's mark: it folds the part and sends nothing.
    $<HTMLButtonElement>("[data-testid=hunk-keep]")?.click();
    await settle();
    expect($("[data-testid=hunk-kept]")?.textContent).toBe("Looks good");
    expect(part()?.textContent).not.toContain("the readme");
    $<HTMLButtonElement>("[data-testid=hunk-keep]")?.click();
    await settle();

    // A comment on a line waits for the next comment to the agent.
    $<HTMLButtonElement>("[data-testid=hunk-comment]")?.click();
    await settle();
    const field = $<HTMLTextAreaElement>("[id^=line-comment-]");
    expect(document.activeElement).toBe(field);
    if (field) {
      field.value = "Spell it right";
      field.dispatchEvent(new Event("input", { bubbles: true }));
    }
    $<HTMLButtonElement>("[data-testid=line-comment-add]")?.click();
    await settle();
    expect($("[data-testid=line-comment]")?.textContent).toContain("Spell it right");
    expect($("[data-testid=inbox-line-comments]")?.textContent).toContain("1 comment on a line goes");
    button(/^Send comment/)?.click();
    await settle();
    expect(core.sent("review.comment").at(-1)?.params).toEqual({
      id: "r1",
      text: "",
      lines: [{ path: "README.md", line: 1, old: true, text: "Spell it right" }],
    });
    expect($("[data-testid=line-comment]")).toBeNull();

    $<HTMLButtonElement>("[data-testid=hunk-discard]")?.click();
    await settle();
    expect(core.sent("review.discard_hunk").at(-1)?.params).toEqual({
      id: "r1",
      path: "README.md",
      hunk: {
        old_start: 1,
        old_lines: 1,
        new_start: 1,
        new_lines: 1,
        header: "",
        lines: ["-teh readme", "+the readme"],
      },
    });
    expect($("[data-testid=inbox-announce], [aria-live]")).not.toBeNull();
    $<HTMLButtonElement>("[data-testid=inbox-undo-part]")?.click();
    await settle();
    expect(core.sent("review.undo_discard")).toHaveLength(1);
  });

  it("says Keep and Discard, never Approve or Reject", async () => {
    await setup([
      review("r1", {
        files: [
          change("README.md"),
          change(".mcp.json", { protected: true }),
          change(".envrc", { protected: true, by: "outside" }),
        ],
        git_changes: [{ path: ".git/hooks/pre-commit", kind: "added" }],
      }),
      review("r2", {
        mode: "isolated",
        files: [change("a.rs", { blocked: true }), change(".envrc", { protected: true })],
      }),
      review("r3", { mode: "isolated", state: "partly_approved", not_copied: ["NOTES.md"] }),
    ]);
    // Everything a person reads or hears: text, and the names and hints of controls.
    const words = () =>
      [
        target.textContent,
        ...[...target.querySelectorAll("[aria-label], [title], [placeholder]")].flatMap((e) =>
          ["aria-label", "title", "placeholder"].map((a) => e.getAttribute(a) ?? ""),
        ),
      ].join("\n");
    const said: string[] = [];
    await open();
    for (const id of ["r1", "r2", "r3"]) {
      $<HTMLButtonElement>(`[data-item=${id}]`)?.click();
      await settle();
      said.push(words());
      if (!button(/^Discard…$/)) continue;
      button(/^Discard…$/)?.click();
      await settle();
      said.push(words());
      button(/^Discard the changes$/)?.click();
      await settle();
      said.push(words());
    }
    expect(said.join("\n")).toContain("Undo discard");
    expect(said.join("\n")).not.toMatch(/\b(approv|reject)/i);
  });

  it("explains the changes in plain words on a press, beside each file's diff", async () => {
    const core = await setup([review("r1", { files: [change("README.md"), change("docs/a.md")] })]);
    await open();
    expect(core.sent("agents.start")).toEqual([]);
    core.agentAnswer =
      'Done.\n```json\n{"overall": ["The readme greets people properly now."], "files": [' +
      '{"path": "README.md", "explanation": "Fixes a typo in the greeting."}]}\n```';
    $<HTMLButtonElement>("[data-testid=inbox-explain]")?.click();
    await settle();
    const [start] = core.sent("agents.start");
    expect(start?.params).toMatchObject({ project: "p1", mode: "research", no_web: true });
    expect(start?.params.prompt).toContain("--- README.md");
    expect(core.sent("agents.stop")).toHaveLength(1);
    expect($("[data-testid=inbox-explanation]")?.textContent).toContain("The readme greets people properly now.");
    expect($("[data-testid=inbox-file-explained]")?.textContent?.trim()).toBe("Fixes a typo in the greeting.");

    // The other file has no line of its own; its name in the card selects it back.
    $<HTMLButtonElement>("[data-file='docs/a.md']")?.click();
    await settle();
    expect($("[data-testid=inbox-file-explained]")).toBeNull();
    [...target.querySelectorAll<HTMLButtonElement>("[data-testid=inbox-explanation] button")]
      .find((b) => b.textContent === "README.md")
      ?.click();
    await settle();
    expect($("[data-file='README.md']")?.getAttribute("aria-current")).toBe("true");

    // The agent's files change: the explanation says it is out of date.
    core.items = [review("r1", { files: [change("README.md", { additions: 4 }), change("docs/a.md")] })];
    core.emit({ type: "review.changed", data: { project: "p1", id: "r1" } });
    await settle();
    expect($("[data-testid=inbox-explain-stale]")).not.toBeNull();

    // A failed start says so and offers another try.
    core.failNext.set("agents.start", new CoreError({ code: "conflict", message: "2 research agents already work" }));
    button(/^Explain again$/)?.click();
    await settle();
    expect($("[data-testid=inbox-explain-error]")?.textContent).toContain(
      "The agent did not start: 2 research agents already work",
    );
  });

  it("stops the explaining agent on Stop and keeps nothing", async () => {
    const core = await setup();
    await open();
    $<HTMLButtonElement>("[data-testid=inbox-explain]")?.click();
    await settle();
    expect($("[data-testid=inbox-explain-running]")?.textContent).toContain("Reading the changes…");
    button(/^Stop$/)?.click();
    await settle();
    expect(core.sent("agents.stop")).toHaveLength(1);
    expect($("[data-testid=inbox-explain]")).not.toBeNull();
  });

  it("lists one task's items in several repositories together, each named by its repository", async () => {
    const repos = ["p1", "p2"];
    await setup(
      [
        review("r3", { session: "s2", mode: "isolated", files: [change("b.md")] }),
        review("r2", { project: "p2", session: "s1", mode: "isolated", task_repos: repos, files: [change("lib.rs")] }),
        review("r1", { session: "s1", mode: "isolated", task_repos: repos, files: [change("README.md")] }),
      ],
      async (core) => {
        core.projects.push({ id: "p2", name: "api", root: "/api", last_opened_ms: null, trusted: false });
      },
    );
    await open();
    const entries = [...target.querySelectorAll<HTMLElement>("[data-item]")].map((e) => e.dataset.item);
    // The session's two items stay next to each other.
    expect(entries.indexOf("r2") + 1).toBe(entries.indexOf("r1"));
    const tags = [...target.querySelectorAll("[data-testid=item-repo]")].map((e) => e.textContent?.trim());
    expect(tags).toEqual(["· api", "· demo"]);
    $<HTMLButtonElement>("[data-item=r2]")?.click();
    await settle();
    expect($("[data-testid=inbox-repos]")?.textContent).toContain("The changes in api. This task also changed demo");
  });
});
