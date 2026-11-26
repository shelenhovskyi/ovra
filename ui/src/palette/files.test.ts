import { describe, expect, it } from "vitest";
import type { CoreEvent, Request } from "../lib/protocol";
import { CoreError, type Transport } from "../lib/transport";
import { projectFiles } from "./files";

/** Serves `fs.list_files` from `files` in pages of `page`; `bump` changes the version mid-listing once. */
class FilesCore implements Transport {
  calls = 0;
  version = 1;
  bumpAfter = -1;
  private listeners = new Set<(event: CoreEvent) => void>();
  constructor(
    public files: string[],
    private page = 2,
  ) {}
  call = (async (request: Request) => {
    if (request.method !== "fs.list_files") throw new CoreError({ code: "unavailable", message: request.method });
    this.calls++;
    if (this.calls === this.bumpAfter) this.version++;
    const offset = request.params.offset ?? 0;
    return {
      type: "fs.files",
      data: { paths: this.files.slice(offset, offset + this.page), total: this.files.length, version: this.version },
    };
  }) as Transport["call"];
  onEvent(listener: (event: CoreEvent) => void): () => void {
    this.listeners.add(listener);
    return () => this.listeners.delete(listener);
  }
  emit(event: CoreEvent): void {
    for (const listener of this.listeners) listener(event);
  }
}

describe("project files", () => {
  it("reads every page and caches until the files change", async () => {
    const core = new FilesCore(["README.md", "src/a.rs", "src/b.rs"]);
    expect(await projectFiles(core, "p1")).toEqual(["README.md", "src/a.rs", "src/b.rs"]);
    expect(core.calls).toBe(2);
    await projectFiles(core, "p1");
    expect(core.calls).toBe(2);

    core.files = ["README.md", "src/c.rs"];
    core.emit({ type: "fs.files_changed", data: { project: "p1", added: ["src/c.rs"], removed: [], version: 2 } });
    expect(await projectFiles(core, "p1")).toEqual(["README.md", "src/c.rs"]);
    core.emit({ type: "fs.changed", data: { project: "p1", paths: ["x"] } });
    await projectFiles(core, "p1");
    expect(core.calls).toBe(4);
  });

  it("starts over when the version changes between pages", async () => {
    const core = new FilesCore(["a", "b", "c"]);
    core.bumpAfter = 2;
    expect(await projectFiles(core, "p2")).toEqual(["a", "b", "c"]);
    expect(core.calls).toBe(4);
  });

  it("forgets a failed listing so the next open retries", async () => {
    let fail = true;
    const core = new FilesCore(["a"]);
    const call = core.call;
    core.call = ((request: Request) =>
      fail ? Promise.reject(new Error("down")) : call(request)) as unknown as Transport["call"];
    await expect(projectFiles(core, "p3")).rejects.toThrow("down");
    fail = false;
    expect(await projectFiles(core, "p3")).toEqual(["a"]);
  });
});
