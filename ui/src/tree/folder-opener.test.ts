import { describe, expect, it } from "vitest";
import type { Project } from "../lib/protocol";
import { FakeCore } from "./fake-core";
import { FolderOpener } from "./folder-opener.svelte";

/** A fake core with the system's folder dialog, which answers with `picked`. */
class DialogCore extends FakeCore {
  picks = 0;
  constructor(public picked: string | null | Error) {
    super();
  }
  async pickFolder(): Promise<string | null> {
    this.picks++;
    if (this.picked instanceof Error) throw this.picked;
    return this.picked;
  }
}

function setup(core: FakeCore) {
  const opened: Project[] = [];
  return { opener: new FolderOpener(core, (p) => opened.push(p)), opened };
}

const methods = (core: FakeCore) => core.calls.map((c) => c.method);

describe("FolderOpener", () => {
  it("adds and opens the picked folder through the core", async () => {
    const core = new DialogCore("/work/app");
    const { opener, opened } = setup(core);
    expect(opener.canPick).toBe(true);
    await opener.pick();
    expect(core.calls).toEqual([
      { method: "projects.add", params: { root: "/work/app" } },
      { method: "projects.open", params: { id: "p1" } },
    ]);
    expect(opened.map((p) => p.root)).toEqual(["/work/app"]);
    expect(opener.error).toBeNull();
  });

  it("does nothing when the dialog is cancelled", async () => {
    const core = new DialogCore(null);
    const { opener, opened } = setup(core);
    await opener.pick();
    expect(core.calls).toEqual([]);
    expect(opened).toEqual([]);
    expect(opener.error).toBeNull();
  });

  it("reports a dialog that fails", async () => {
    const { opener } = setup(new DialogCore(new Error("no display")));
    await opener.pick();
    expect(opener.error).toBe("no display");
  });

  it("cannot pick without the system dialog", async () => {
    const core = new FakeCore();
    const { opener } = setup(core);
    expect(opener.canPick).toBe(false);
    await opener.pick();
    expect(core.calls).toEqual([]);
  });

  it("opens one dropped folder and refuses several", async () => {
    const core = new FakeCore();
    const { opener, opened } = setup(core);
    await opener.drop(["/a", "/b"]);
    expect(opener.error).toBe("Drop one folder at a time.");
    expect(core.calls).toEqual([]);
    await opener.drop(["/work/docs"]);
    expect(opener.error).toBeNull();
    expect(methods(core)).toEqual(["projects.add", "projects.open"]);
    expect(opened.map((p) => p.root)).toEqual(["/work/docs"]);
  });

  it("checks a dropped or picked path like a typed one", async () => {
    const core = new FakeCore();
    const { opener } = setup(core);
    await opener.drop(["relative/dir"]);
    expect(opener.error).toBe("Use the full path to a folder, for example /home/you/my-app.");
    expect(core.calls).toEqual([]);
  });

  it("ignores a second open while one is running", async () => {
    const core = new DialogCore("/work/app");
    const { opener } = setup(core);
    const first = opener.open("/work/app");
    await opener.pick();
    await first;
    expect(core.picks).toBe(0);
    expect(methods(core)).toEqual(["projects.add", "projects.open"]);
  });

  it("accepts a Windows UNC path", async () => {
    const core = new FakeCore();
    const { opener } = setup(core);
    await opener.drop(["\\\\server\\share\\app"]);
    expect(opener.error).toBeNull();
    expect(core.calls[0]).toEqual({ method: "projects.add", params: { root: "\\\\server\\share\\app" } });
  });

  it("opens nothing else while the dialog is up", async () => {
    let answer: (path: string) => void = () => {};
    const core = new DialogCore(null);
    core.pickFolder = () => new Promise((resolve) => (answer = resolve));
    const { opener, opened } = setup(core);
    const picking = opener.pick();
    await opener.drop(["/work/dropped"]);
    answer("/work/picked");
    await picking;
    expect(opened.map((p) => p.root)).toEqual(["/work/picked"]);
  });

  it("opens a known project only once at a time", async () => {
    const core = new FakeCore(undefined, [{ id: "p1", name: "a", root: "/a", last_opened_ms: null, trusted: false }]);
    const { opener, opened } = setup(core);
    const project = core.projects[0] as Project;
    await Promise.all([opener.openKnown(project), opener.openKnown(project)]);
    expect(opened).toHaveLength(1);
  });
});
