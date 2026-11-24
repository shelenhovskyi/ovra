import { flushSync, mount, unmount } from "svelte";
import { afterEach, describe, expect, it } from "vitest";
import type { CoreEvent, FileBody, Request } from "../lib/protocol";
import { CoreError, type Transport } from "../lib/transport";
import { Workspace } from "../shell/workspace.svelte";
import TestHost from "./TestHost.svelte";

// jsdom has no ResizeObserver, which `bind:clientHeight` uses; a 0 px viewport still renders the overscan rows.
globalThis.ResizeObserver ??= class {
  observe() {}
  unobserve() {}
  disconnect() {}
};

type Files = Record<string, FileBody>;

/** Serves fs.read_file and fs.list_dir from `files`; `emit` pushes core events. */
function fakeCore(files: Files, options: { failOpen?: boolean } = {}) {
  const listeners = new Set<(event: CoreEvent) => void>();
  const reads: string[] = [];
  const urls: string[] = [];
  const transport: Transport = {
    call: ((request: Request) => {
      if (request.method === "open.url" || request.method === "open.external") {
        urls.push("url" in request.params ? request.params.url : `file:${request.params.path}`);
        return options.failOpen ? Promise.reject(new Error("no opener")) : Promise.resolve({ type: "ok" });
      }
      if (request.method === "fs.list_dir") {
        const dir = request.params.path;
        const entries = Object.keys(files)
          .filter((p) =>
            dir === "" ? !p.includes("/") : p.startsWith(`${dir}/`) && !p.slice(dir.length + 1).includes("/"),
          )
          .map((p) => ({ name: p.slice(p.lastIndexOf("/") + 1), kind: "file" as const, ignored: false, git: null }));
        return Promise.resolve({ type: "fs.dir", data: { path: dir, entries } });
      }
      if (request.method !== "fs.read_file") return Promise.reject(new Error(`unexpected ${request.method}`));
      const { path } = request.params;
      reads.push(path);
      const body = files[path];
      if (!body) return Promise.reject(new CoreError({ code: "not_found", message: path }));
      return Promise.resolve({ type: "fs.file", data: { path, size: 1, mtime_ms: 1, body } });
    }) as Transport["call"],
    onEvent: (listener) => {
      listeners.add(listener);
      return () => listeners.delete(listener);
    },
  };
  const emit = (event: CoreEvent) => {
    for (const listener of listeners) listener(event);
  };
  return { transport, reads, urls, emit, listeners };
}

async function settle() {
  flushSync();
  await new Promise((resolve) => setTimeout(resolve));
  flushSync();
}

let app: Record<string, unknown> | null = null;
afterEach(() => {
  if (app) unmount(app);
  app = null;
});

function show(files: Files, path: string | null, options: { failOpen?: boolean } = {}) {
  const core = fakeCore(files, options);
  const workspace = new Workspace();
  workspace.project = { id: "p1", name: "demo", root: "/demo", last_opened_ms: null };
  if (path) workspace.openFile(path);
  const target = document.createElement("div");
  document.body.replaceChildren(target);
  app = mount(TestHost, { target, props: { transport: core.transport, workspace } });
  return { ...core, workspace, target };
}

describe("Viewer", () => {
  it("asks for a file when none is open", async () => {
    const { target, reads } = show({}, null);
    await settle();
    expect(target.textContent).toContain("Open a file from the tree on the left.");
    expect(reads).toEqual([]);
  });

  it("shows plain text escaped", async () => {
    const { target } = show({ "a.txt": { kind: "text", text: "<b>hi</b>" } }, "a.txt");
    await settle();
    expect(target.querySelector("[data-testid=text-view]")?.textContent).toBe("<b>hi</b>");
    expect(target.querySelector(".path")?.textContent).toBe("a.txt");
    expect(target.querySelector("b")).toBeNull();
  });

  it("shows images and reports missing files", async () => {
    const { target, workspace } = show({ "a.png": { kind: "binary", mime: "image/png", base64: "AAA=" } }, "a.png");
    await settle();
    expect(target.querySelector("img")?.getAttribute("src")).toBe("data:image/png;base64,AAA=");
    workspace.openFile("x.patch");
    await settle();
    expect(target.textContent).toContain("x.patch no longer exists.");
  });

  it("renders a patch with a summary, line numbers and spoken change labels", async () => {
    const { target } = show(
      {
        "fix.diff": {
          kind: "text",
          text: "--- /dev/null\n+++ b/new.txt\n@@ -0,0 +1,2 @@\n+one\n+two\n--- a/old.txt\n+++ /dev/null\n@@ -1 +0,0 @@\n-gone\n",
        },
      },
      "fix.diff",
    );
    await settle();
    expect(target.querySelector("[data-testid=patch-summary]")?.textContent).toMatch(/2\s+files changed,\s+\+2\s+−1/);
    expect([...target.querySelectorAll(".line")].map((row) => row.textContent?.replace(/\s+/g, " ").trim())).toEqual([
      "1 + added line 1: one",
      "2 + added line 2: two",
      "1 - removed line 1: gone",
    ]);
    const header = target.querySelector<HTMLButtonElement>(".file button");
    expect(header?.getAttribute("aria-expanded")).toBe("true");
    header?.click();
    flushSync();
    expect(header?.getAttribute("aria-expanded")).toBe("false");
    expect(target.querySelectorAll(".line")).toHaveLength(1);
    [...target.querySelectorAll("button")].find((b) => b.textContent === "Expand all")?.click();
    flushSync();
    expect(target.querySelectorAll(".line")).toHaveLength(3);
  });

  it("keeps a huge patch and a huge text file to a window of rows", async () => {
    const lines = Array.from({ length: 30_000 }, (_, i) => `+line ${i}`);
    const { target, workspace } = show(
      {
        "big.diff": {
          kind: "text",
          text: ["--- /dev/null", "+++ b/big.txt", "@@ -0,0 +1,30000 @@", ...lines].join("\n"),
        },
        "big.txt": { kind: "text", text: lines.join("\n") },
      },
      "big.diff",
    );
    await settle();
    expect(target.querySelector("[data-testid=patch-summary]")?.textContent).toContain("+30000");
    expect(target.querySelectorAll(".line").length).toBeLessThan(100);
    workspace.openFile("big.txt");
    await settle();
    expect(target.querySelectorAll(".text-line").length).toBeLessThan(100);
  });

  it("reloads when the open file changes on disk and stops listening when closed", async () => {
    const files: Files = { "a.txt": { kind: "text", text: "v1" } };
    const { target, emit, reads, workspace, listeners } = show(files, "a.txt");
    await settle();
    files["a.txt"] = { kind: "text", text: "v2" };
    emit({ type: "fs.changed", data: { project: "p1", paths: ["other.txt"] } });
    emit({ type: "fs.changed", data: { project: "p1", paths: ["a.txt"] } });
    await settle();
    expect(target.textContent).toContain("v2");
    expect(reads).toEqual(["a.txt", "a.txt"]);
    workspace.openPath = null;
    flushSync();
    expect(listeners.size).toBe(0);
  });

  it("renders markdown, follows project links and loads project images", async () => {
    const { target, workspace, reads, urls } = show(
      {
        "docs/a.md": { kind: "text", text: "# A\n\n[next](b.md) [web](https://example.com)\n\n![logo](img/logo.png)" },
        "docs/img/logo.png": { kind: "binary", mime: "image/png", base64: "AAA=" },
        "docs/b.md": { kind: "text", text: "# B" },
      },
      "docs/a.md",
    );
    await settle();
    expect(target.querySelector("h1")?.textContent).toBe("A");
    expect(target.querySelector("img")?.getAttribute("src")).toBe("data:image/png;base64,AAA=");
    const web = target.querySelector<HTMLAnchorElement>('a[href^="https"]');
    expect(web?.dispatchEvent(new MouseEvent("click", { bubbles: true, cancelable: true }))).toBe(false);
    expect(workspace.openPath).toBe("docs/a.md");
    expect(urls).toEqual(["https://example.com"]);
    target.querySelector<HTMLAnchorElement>('a[href="b.md"]')?.click();
    await settle();
    expect(workspace.openPath).toBe("docs/b.md");
    expect(target.querySelector("h1")?.textContent).toBe("B");
    expect(reads).toEqual(["docs/a.md", "docs/img/logo.png", "docs/b.md"]);
  });

  it("gives binaries and large files their size and a way to open them elsewhere", async () => {
    const { target, workspace, urls } = show(
      { "a.bin": { kind: "binary", mime: "application/zip", base64: "" }, "big.txt": { kind: "too_large" } },
      "a.bin",
    );
    await settle();
    expect(target.textContent).toContain("This is a binary file (application/zip, 1 byte) and cannot be shown here.");
    workspace.openFile("big.txt");
    await settle();
    expect(target.textContent).toContain("This file is 1 byte, too large to show here.");
    const buttons = [...target.querySelectorAll("button")].filter((b) => b.textContent === "Open in default app");
    expect(buttons).toHaveLength(1);
    buttons[0]?.click();
    await settle();
    expect(urls).toEqual(["file:big.txt"]);
    expect(target.querySelector("[role=status]")?.textContent).toBe("Opened big.txt in its default app.");
  });

  it("explains broken and failed links instead of opening them", async () => {
    const { target, workspace } = show(
      { "a.md": { kind: "text", text: "[gone](nope.md) [web](https://example.com) [mail](mailto:x@y.z)" } },
      "a.md",
      { failOpen: true },
    );
    await settle();
    const status = () => target.querySelector("[role=status]")?.textContent;
    target.querySelector<HTMLAnchorElement>('a[href="nope.md"]')?.click();
    await settle();
    expect(workspace.openPath).toBe("a.md");
    expect(status()).toBe("The link points to nope.md, which was not found.");
    target.querySelector<HTMLAnchorElement>('a[href^="https"]')?.click();
    await settle();
    expect(status()).toBe("Could not open https://example.com in a browser.");
  });

  it("keeps keyboard focus in the viewer after following a link", async () => {
    const { target, workspace } = show(
      { "a.md": { kind: "text", text: "[next](b.txt)" }, "b.txt": { kind: "text", text: "b" } },
      "a.md",
    );
    await settle();
    const link = target.querySelector<HTMLAnchorElement>("a");
    link?.focus();
    link?.click();
    await settle();
    await settle();
    expect(workspace.openPath).toBe("b.txt");
    expect(document.activeElement?.getAttribute("data-testid")).toBe("text-view");
  });
});
