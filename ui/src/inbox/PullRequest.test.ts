import { flushSync, mount, unmount } from "svelte";
import { afterEach, beforeEach, describe, expect, it } from "vitest";
import type { ReviewItem } from "../lib/protocol";
import { mainView } from "../shell/main-view.svelte";
import { Workspace } from "../shell/workspace.svelte";
import { handleOpenTerminalIn } from "../terminal/open-in";
import { change, FakeReviewCore, review } from "./fake-review";
import TestHost from "./TestHost.svelte";

// jsdom has no ResizeObserver, which the diff's `bind:clientHeight` uses,
// and no modal dialogs.
globalThis.ResizeObserver ??= class {
  observe() {}
  unobserve() {}
  disconnect() {}
};
HTMLDialogElement.prototype.showModal ??= function (this: HTMLDialogElement) {
  this.open = true;
};

let app: Record<string, unknown> | null = null;
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

const worker = (extra: Partial<ReviewItem> = {}) =>
  review("r1", {
    mode: "isolated",
    summary: "Wrote NOTES.md.",
    files: [change("NOTES.md", { kind: "added", additions: 3, deletions: 0 })],
    fingerprint: "f1",
    ...extra,
  });

async function setup(items: ReviewItem[]) {
  const core = new FakeReviewCore();
  core.items = items;
  const workspace = new Workspace();
  workspace.project = { id: "p1", name: "demo", root: "/demo", last_opened_ms: null, trusted: false };
  app = mount(TestHost, { target, props: { transport: core, workspace } });
  await settle();
  mainView.show("reviews");
  await settle();
  return core;
}

const $ = <T extends HTMLElement = HTMLElement>(selector: string) => target.querySelector<T>(selector);
const button = (name: RegExp, within: ParentNode = target) =>
  [...within.querySelectorAll<HTMLButtonElement>("button")].find((b) => name.test(b.textContent ?? ""));
const field = (name: RegExp) =>
  [...target.querySelectorAll("[data-testid=pr-dialog] label")]
    .find((l) => name.test(l.textContent?.trim() ?? ""))
    ?.querySelector<HTMLInputElement | HTMLTextAreaElement>("input, textarea");

function type(input: HTMLInputElement | HTMLTextAreaElement | null | undefined, text: string) {
  if (!input) throw new Error("no such field");
  input.value = text;
  input.dispatchEvent(new Event("input", { bubbles: true }));
  input.dispatchEvent(new Event("change", { bubbles: true }));
}

async function openDialog() {
  button(/^Open pull request/)?.click();
  await settle();
  const dialog = $<HTMLDialogElement>("[data-testid=pr-dialog]");
  if (!dialog) throw new Error("no dialog");
  return dialog;
}

describe("Pull requests in Reviews", () => {
  it("opens a dialog with what would be sent and creates a draft from the edited fields", async () => {
    const core = await setup([worker()]);
    // Beside Keep and Discard, not one of them.
    expect(button(/^Keep changes$/)).toBeDefined();
    const dialog = await openDialog();
    expect(dialog.open).toBe(true);
    expect(dialog.getAttribute("aria-labelledby")).toBe("pr-dialog-title");
    expect(core.sent("pr.prepare")).toEqual([{ method: "pr.prepare", params: { id: "r1", base: null } }]);
    expect(dialog.querySelector("[data-testid=pr-where]")?.textContent).toContain(
      "origin (github.com) · Signed in as octo on github.com",
    );
    expect(dialog.querySelector("[aria-label='Files in the pull request']")?.textContent).toContain("NOTES.md");
    expect(document.activeElement).toBe(field(/^Title/));
    expect(field(/^Title/)?.value).toBe("Write the notes");
    expect(field(/^Description/)?.value).toBe("Wrote NOTES.md.");
    expect(field(/^Draft/)?.getAttribute("type")).toBe("checkbox");
    expect((field(/^Draft/) as HTMLInputElement).checked).toBe(true);

    type(field(/^Title/), "Add the notes");
    type(field(/^New branch/), "notes");
    await settle();
    button(/^Create pull request$/, dialog)?.click();
    await settle();
    expect(core.sent("pr.create")).toEqual([
      {
        method: "pr.create",
        params: {
          id: "r1",
          fingerprint: "f1",
          draft: {
            remote: "origin",
            base: "main",
            branch: "notes",
            title: "Add the notes",
            body: "Wrote NOTES.md.",
            draft: true,
          },
        },
      },
    ]);
    // The checks are asked for once at once; the dialog is gone.
    expect(core.sent("pr.refresh")).toHaveLength(1);
    expect($("[data-testid=pr-dialog]")).toBeNull();
    expect(target.textContent).toContain("Pull request #7 opened.");
    expect($("[data-testid=pr-chip]")?.textContent).toBe("PR #7");
    expect(document.activeElement).toBe($("[data-testid=pr-chip]"));
  });

  it("publishes protected files and workflows only once the person ticks the box under a plain warning", async () => {
    const files = [change("NOTES.md"), change(".github/workflows/ci.yml", { protected: true })];
    const core = await setup([worker({ files })]);
    const dialog = await openDialog();
    const list = dialog.querySelector("[aria-label='Files in the pull request']");
    expect(list?.querySelectorAll(".tag")).toHaveLength(1);
    expect(dialog.querySelector("[data-testid=pr-protected]")?.textContent).toContain(
      "These files are protected: .github/workflows/ci.yml. Workflow changes run on GitHub, with the repository's secrets, as soon as the branch is pushed, before anyone merges.",
    );
    const create = button(/^Create pull request$/, dialog);
    expect(create?.getAttribute("aria-disabled")).toBe("true");
    expect(dialog.querySelector("#pr-dialog-missing")?.textContent).toContain("the box for the protected files ticked");
    create?.click();
    await settle();
    expect(core.sent("pr.create")).toEqual([]);

    $<HTMLInputElement>("[data-testid=pr-include-protected]")?.click();
    await settle();
    expect(create?.getAttribute("aria-disabled")).toBe("false");
    create?.click();
    await settle();
    expect(core.sent("pr.create").map((c) => c.params.include_protected)).toEqual([true]);
    expect(target.textContent).toContain("Pull request #7 opened.");
  });

  it("asks again for a new base and keeps the person's edits", async () => {
    const core = await setup([worker()]);
    const dialog = await openDialog();
    type(field(/^Title/), "Mine");
    core.prConflicts = ["NOTES.md"];
    type(field(/^Base branch/), "release");
    await settle();
    expect(core.sent("pr.prepare").at(-1)?.params.base).toBe("release");
    expect(field(/^Title/)?.value).toBe("Mine");
    // Conflicts: no Create, but the worker can be asked to update.
    expect(dialog.querySelector("[data-testid=pr-conflicts]")?.textContent).toContain(
      "These files conflict with origin/release: NOTES.md.",
    );
    expect(button(/^Create pull request$/, dialog)).toBeUndefined();
    button(/^Ask the worker to update$/, dialog)?.click();
    await settle();
    expect(core.sent("pr.ask_update")).toEqual([
      { method: "pr.ask_update", params: { id: "r1", remote: "origin", base: "release" } },
    ]);
    expect($("[data-testid=pr-dialog]")).toBeNull();
    expect(target.textContent).toContain("Asked the worker to update.");
  });

  it("without a signed-in gh says how to get one, offers a terminal and no Create", async () => {
    const core = await setup([worker()]);
    core.gh = { installed: true, version: "2.60.0", host: "github.com", login: null, signed_in: false };
    const opened: string[] = [];
    const stop = handleOpenTerminalIn((cwd) => void opened.push(cwd));
    const dialog = await openDialog();
    expect(dialog.querySelector("[data-testid=pr-no-gh]")?.textContent).toContain(
      "Install GitHub CLI (gh) and run gh auth login in a terminal.",
    );
    expect(button(/^Create pull request$/, dialog)).toBeUndefined();
    expect(field(/^Title/)).toBeUndefined();
    button(/^Open terminal$/, dialog)?.click();
    await settle();
    stop();
    expect(opened).toEqual([""]);
    expect($("[data-testid=pr-dialog]")).toBeNull();
    expect(core.sent("pr.create")).toEqual([]);
  });

  it("Escape closes only the dialog, and nothing is sent when the files moved since the review", async () => {
    const core = await setup([worker()]);
    // The person reviewed f1; the worker's files moved on before the dialog opened.
    core.items[0] = worker({ fingerprint: "f2" });
    const dialog = await openDialog();
    expect(dialog.textContent).toContain("The worker's files changed since you looked.");
    const create = button(/^Create pull request$/, dialog);
    expect(create?.getAttribute("aria-disabled")).toBe("true");
    create?.click();
    await settle();
    dialog.dispatchEvent(
      new KeyboardEvent("keydown", { code: "Escape", key: "Escape", bubbles: true, cancelable: true }),
    );
    await settle();
    expect($("[data-testid=pr-dialog]")).toBeNull();
    expect($("[data-testid=inbox]")).not.toBeNull();
    expect(document.activeElement).toBe(button(/^Open pull request/));
    expect(core.sent("pr.create")).toEqual([]);
  });

  it("a published item shows its chip, checks with links, Refresh, Send log, and keeps Keep and Discard", async () => {
    const url = "https://github.com/o/r/pull/7";
    const check = (name: string, state: "passed" | "failed" | "pending", run: number) => ({
      name,
      state,
      url: `${url}/checks/${name}`,
      run,
    });
    const published = worker({
      state: "published",
      pull_request: {
        number: 7,
        url,
        remote: "origin",
        base: "main",
        branch: "ovra/t1/docs",
        pushed: "c1",
        status: null,
      },
    });
    const core = await setup([published]);
    expect($("[data-testid=pr-chip]")?.textContent).toBe("PR #7");
    expect(button(/^Open pull request/)).toBeUndefined();
    expect(button(/^Keep changes$/)).toBeDefined();
    expect(button(/^Discard…$/)).toBeDefined();
    expect(button(/^Update pull request$/)).toBeUndefined();

    core.prStatus = {
      state: "open",
      draft: true,
      url,
      head: "c1",
      checks: [check("build", "passed", 1), check("test", "failed", 2), check("lint", "pending", 3)],
      review_decision: null,
      merge_state: null,
    };
    button(/^Refresh$/)?.click();
    await settle();
    // `pr.refresh` answers; `pr.changed` is what refreshes the list.
    core.emit({ type: "pr.changed", data: { project: "p1", id: "r1" } });
    await settle();
    expect($("[data-testid=pr-chip]")?.textContent).toBe("PR #7 · draft · 1 of 3 checks passed");
    const checks = $("[aria-label=Checks]");
    expect(checks?.textContent).toContain("Failed");
    $<HTMLAnchorElement>("[data-testid=pr-chip]")?.click();
    [...(checks?.querySelectorAll("a") ?? [])].find((a) => a.textContent === "test")?.click();
    await settle();
    expect(core.sent("open.url").map((c) => c.params.url)).toEqual([url, `${url}/checks/test`]);
    // Only the failed check offers its log.
    const send = [...(checks?.querySelectorAll("button") ?? [])];
    expect(send.map((b) => b.getAttribute("aria-label"))).toEqual(["Send the log of test to the worker"]);
    send[0]?.click();
    await settle();
    expect(core.sent("pr.send_log")).toEqual([{ method: "pr.send_log", params: { id: "r1", run: 2 } }]);
  });

  it("offers Update once a comment reopened the item, with the reviewed fingerprint", async () => {
    const pull_request = {
      number: 7,
      url: "https://github.com/o/r/pull/7",
      remote: "origin",
      base: "main",
      branch: "ovra/t1/docs",
      pushed: "c1",
      status: null,
    };
    const core = await setup([worker({ state: "open", pull_request, fingerprint: "f3" })]);
    expect(button(/^Open pull request/)).toBeUndefined();
    button(/^Update pull request$/)?.click();
    await settle();
    expect(core.sent("pr.update")).toEqual([{ method: "pr.update", params: { id: "r1", fingerprint: "f3" } }]);
    expect(core.sent("pr.refresh")).toHaveLength(1);
    expect(target.textContent).toContain("Pull request #7 updated.");
  });

  it("an Update with protected files waits for the person's tick", async () => {
    const pull_request = {
      number: 7,
      url: "https://github.com/o/r/pull/7",
      remote: "origin",
      base: "main",
      branch: "ovra/t1/docs",
      pushed: "c1",
      status: null,
    };
    const files = [change(".github/actions/setup/action.yml", { protected: true })];
    const core = await setup([worker({ state: "open", pull_request, files, fingerprint: "f3" })]);
    expect($("[data-testid=pr-update-protected]")?.textContent).toContain("Workflow changes run on GitHub");
    const update = button(/^Update pull request$/);
    expect(update?.getAttribute("aria-disabled")).toBe("true");
    update?.click();
    await settle();
    expect(core.sent("pr.update")).toEqual([]);
    $<HTMLInputElement>("[data-testid=pr-update-include-protected]")?.click();
    await settle();
    button(/^Update pull request$/)?.click();
    await settle();
    expect(core.sent("pr.update")).toEqual([
      { method: "pr.update", params: { id: "r1", fingerprint: "f3", include_protected: true } },
    ]);
  });
});
