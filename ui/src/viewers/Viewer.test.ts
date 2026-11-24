import { flushSync, mount, unmount } from "svelte";
import { afterEach, describe, expect, it } from "vitest";
import type { CoreEvent, FileBody, Request } from "../lib/protocol";
import { CoreError, type Transport } from "../lib/transport";
import { Workspace } from "../shell/workspace.svelte";
import TestHost from "./TestHost.svelte";

type Files = Record<string, FileBody>;

/** Serves fs.read_file from `files`; `emit` pushes core events. */
function fakeCore(files: Files) {
  const listeners = new Set<(event: CoreEvent) => void>();
  const reads: string[] = [];
  const urls: string[] = [];
  const transport: Transport = {
    call: ((request: Request) => {
      if (request.method === "open.url") {
        urls.push(request.params.url);
        return Promise.resolve({ type: "ok" });
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

function show(files: Files, path: string | null) {
  const core = fakeCore(files);
  const workspace = new Workspace();
  workspace.project = { id: "p1", name: "demo", root: "/demo", last_opened_ms: null };
  if (path) workspace.openFile(path);
  const target = document.createElement("div");
  app = mount(TestHost, { target, props: { transport: core.transport, workspace } });
  return { ...core, workspace, target };
}

describe("Viewer", () => {
  it("asks for a file when none is open", async () => {
    const { target, reads } = show({}, null);
    await settle();
    expect(target.textContent).toContain("open a file from the tree");
    expect(reads).toEqual([]);
  });

  it("shows plain text escaped", async () => {
    const { target } = show({ "a.txt": { kind: "text", text: "<b>hi</b>" } }, "a.txt");
    await settle();
    expect(target.querySelector("[data-testid=text-view]")?.textContent).toBe("<b>hi</b>");
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

  it("renders a patch with line numbers", async () => {
    const { target } = show(
      { "fix.diff": { kind: "text", text: "--- /dev/null\n+++ b/new.txt\n@@ -0,0 +1,2 @@\n+one\n+two\n" } },
      "fix.diff",
    );
    await settle();
    expect(target.querySelector("h2")?.textContent).toMatch(/new\s+new\.txt/);
    expect([...target.querySelectorAll("tr.added")].map((row) => row.textContent)).toEqual(["1+one", "2+two"]);
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

  it("does not render binaries or large files", async () => {
    const { target, workspace } = show(
      { "a.bin": { kind: "binary", mime: "application/zip", base64: "" }, "big.txt": { kind: "too_large" } },
      "a.bin",
    );
    await settle();
    expect(target.textContent).toContain("Binary file (application/zip) cannot be shown.");
    workspace.openFile("big.txt");
    await settle();
    expect(target.textContent).toContain("too large");
  });
});
