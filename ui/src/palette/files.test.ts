import { describe, expect, it } from "vitest";
import { dirsFromPaths, FakeCore } from "../tree/fake-core";
import { projectFiles } from "./files";

describe("project files", () => {
  it("leaves out tracked files deleted from disk", async () => {
    const core = new FakeCore(dirsFromPaths(["kept.md", "gone.md"], { "gone.md": "deleted" }));
    expect(await projectFiles(core, "p3")).toEqual(["kept.md"]);
  });

  it("walks the project, skips ignored entries and caches until files change", async () => {
    const core = new FakeCore(dirsFromPaths(["README.md", "src/main.rs", "src/ui/app.ts", ".git/HEAD"]));
    expect(await projectFiles(core, "p1")).toEqual(["README.md", "src/main.rs", "src/ui/app.ts"]);
    const listed = core.listedPaths().length;

    await projectFiles(core, "p1");
    expect(core.listedPaths()).toHaveLength(listed);

    core.emit({ type: "fs.changed", data: { project: "p1", paths: ["src/new.rs"] } });
    core.dirs = dirsFromPaths(["README.md", "src/new.rs"]);
    expect(await projectFiles(core, "p1")).toEqual(["README.md", "src/new.rs"]);
  });

  it("forgets a failed walk so the next open retries", async () => {
    const core = new FakeCore(new Map());
    await expect(projectFiles(core, "p2")).rejects.toThrow("no such directory");
    core.dirs = dirsFromPaths(["a.txt"]);
    expect(await projectFiles(core, "p2")).toEqual(["a.txt"]);
  });
});
