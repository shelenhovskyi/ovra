import { flushSync, mount, unmount } from "svelte";
import { afterEach, describe, expect, it } from "vitest";
import type { Request, ResponseFor } from "../lib/protocol";
import type { DropClash, DropImport, Point, WindowDrop } from "../lib/transport";
import FileTree from "./FileTree.svelte";
import { dirsFromPaths, FakeCore } from "./fake-core";

globalThis.ResizeObserver ??= class {
  observe() {}
  unobserve() {}
  disconnect() {}
};

/** A fake core in the desktop window: drags and drops from outside, and imports that clash once. */
class WindowCore extends FakeCore {
  imports: { dir: string; clash: DropClash }[] = [];
  private over = new Set<(at: Point | null) => void>();
  private drops = new Set<(drop: WindowDrop) => void>();
  /** Like the core's chip check: `.env` looks like a secret store. */
  override async call<R extends Request>(request: R): Promise<ResponseFor<R>> {
    if (request.method !== "context.check") return super.call(request);
    const notes = request.params.items.map((item) =>
      "path" in item && item.path.endsWith(".env") ? "secret_file" : null,
    );
    return { type: "context.checked", data: { notes } } as ResponseFor<R>;
  }
  onDragOver(listener: (at: Point | null) => void): () => void {
    this.over.add(listener);
    return () => this.over.delete(listener);
  }
  onDrop(listener: (drop: WindowDrop) => void): () => void {
    this.drops.add(listener);
    return () => this.drops.delete(listener);
  }
  async importDrop(_id: string, _project: string, dir: string, clash: DropClash): Promise<DropImport> {
    this.imports.push({ dir, clash });
    if (clash === "ask") return { kind: "clashes", names: ["a.png"] };
    this.dirs.get(dir)?.push({ name: "a copy.png", kind: "file", ignored: false, git: null });
    return { kind: "copied", paths: [`${dir}/a copy.png`], skipped_links: [] };
  }
  drag(at: Point | null): void {
    for (const listener of this.over) listener(at);
  }
  drop(paths = ["/out/a.png"]): void {
    for (const listener of this.drops) listener({ id: "d1", paths, x: 1, y: 1 });
  }
}

let cleanup: (() => void) | null = null;
afterEach(() => {
  cleanup?.();
  cleanup = null;
  document.body.innerHTML = "";
});

async function settle(): Promise<void> {
  for (let i = 0; i < 3; i++) {
    flushSync();
    await new Promise((resolve) => setImmediate(resolve));
  }
  flushSync();
}

describe("FileTree: files dropped from outside", () => {
  it("highlights the folder, asks about the clash and selects the copy", async () => {
    const core = new WindowCore(dirsFromPaths(["src/a.png", "README.md"]));
    const target = document.body.appendChild(document.createElement("div"));
    const app = mount(FileTree, { target, props: { transport: core, project: "p1", onopen: () => {} } });
    cleanup = () => unmount(app);
    await settle();
    const row = (path: string) => target.querySelector<HTMLElement>(`[data-path="${path}"]`) as HTMLElement;
    // jsdom has no layout: the pointer is over the README.md row, whose folder is the project's.
    let under: Element | null = row("README.md");
    document.elementFromPoint = () => under;

    core.drag({ x: 1, y: 1 });
    flushSync();
    expect(target.querySelector("[role=tree]")?.classList.contains("drop-root")).toBe(true);
    under = row("src");
    core.drag({ x: 1, y: 1 });
    flushSync();
    expect(row("src").classList.contains("drop")).toBe(true);
    expect(target.querySelector(".drop-hint")?.textContent).toBe("Copy into src");
    under = document.body;
    core.drag({ x: 1, y: 1 });
    flushSync();
    expect(row("src").classList.contains("drop")).toBe(false);
    expect(target.querySelector(".drop-hint")).toBeNull();

    under = row("src");
    core.drop();
    await settle();
    expect(row("src").classList.contains("drop")).toBe(false);
    const dialog = target.querySelector("[role=alertdialog]");
    expect(dialog?.textContent).toContain("“a.png” already exists in src.");
    expect(document.activeElement?.textContent).toBe("Keep both");
    (document.activeElement as HTMLButtonElement).click();
    await settle();
    expect(core.imports).toEqual([
      { dir: "src", clash: "ask" },
      { dir: "src", clash: "keep_both" },
    ]);
    expect(target.querySelector("[role=alertdialog]")).toBeNull();
    expect(row("src/a copy.png").getAttribute("aria-selected")).toBe("true");
  });

  it("asks before copying a file that may hold secrets, and Cancel is the default", async () => {
    const core = new WindowCore(dirsFromPaths(["src/a.png"]));
    const target = document.body.appendChild(document.createElement("div"));
    const app = mount(FileTree, { target, props: { transport: core, project: "p1", onopen: () => {} } });
    cleanup = () => unmount(app);
    await settle();
    const src = target.querySelector<HTMLElement>('[data-path="src"]');
    document.elementFromPoint = () => src;

    core.drop(["/home/me/app/.env", "/out/a.png"]);
    await settle();
    const dialog = target.querySelector("[role=alertdialog]");
    expect(dialog?.textContent).toContain("“.env” may hold secrets.");
    expect(core.imports).toEqual([]);
    expect(document.activeElement?.textContent).toBe("Cancel");
    (document.activeElement as HTMLButtonElement).click();
    await settle();
    expect(core.imports).toEqual([]);

    core.drop(["/home/me/app/.env"]);
    await settle();
    const copy = [...target.querySelectorAll("button")].find((b) => b.textContent === "Copy anyway");
    copy?.click();
    await settle();
    expect(core.imports[0]).toEqual({ dir: "src", clash: "ask" });
  });

  it("ignores a drop outside the tree", async () => {
    const core = new WindowCore(dirsFromPaths(["src/a.png"]));
    const target = document.body.appendChild(document.createElement("div"));
    const app = mount(FileTree, { target, props: { transport: core, project: "p1", onopen: () => {} } });
    cleanup = () => unmount(app);
    await settle();
    document.elementFromPoint = () => document.body;
    core.drop();
    await settle();
    expect(core.imports).toEqual([]);
  });
});
