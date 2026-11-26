import { flushSync, mount, unmount } from "svelte";
import { afterEach, beforeEach, describe, expect, it } from "vitest";
import type { ReviewItem } from "../lib/protocol";
import { mainView } from "../shell/main-view.svelte";
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

describe("Pull requests in Reviews", () => {
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
});
