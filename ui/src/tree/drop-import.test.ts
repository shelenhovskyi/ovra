import { describe, expect, it } from "vitest";
import type { DropClash, DropImport, Point, Transport, WindowDrop } from "../lib/transport";
import { clashQuestion, DropImporter, skippedNote } from "./drop-import.svelte";

describe("drop import texts", () => {
  it("names the clashing items and where they are", () => {
    expect(clashQuestion(["a.png"], "src")).toBe("“a.png” already exists in src.");
    expect(clashQuestion(["a.png", "b"], "")).toBe("“a.png” and 1 more item already exist in the project folder.");
    expect(clashQuestion(["a", "b", "c"], "src")).toBe("“a” and 2 more items already exist in src.");
  });

  it("says which links were left out", () => {
    expect(skippedNote([])).toBeNull();
    expect(skippedNote(["d/link"])).toBe("Skipped “d/link”: links are not copied.");
    expect(skippedNote(["d/a", "d/b"])).toBe("Skipped 2 links, such as “d/a”: links are not copied.");
  });
});

type Call = { id: string; dir: string; clash: DropClash };

/** A window transport whose imports answer from `replies`, in order. */
function setup(replies: (DropImport | Error)[], dirAt: (at: Point) => string | null = () => "src") {
  const calls: Call[] = [];
  const seen = { copied: [] as string[][], failed: [] as string[], told: [] as string[] };
  const transport = {
    importDrop: async (id: string, _project: string, dir: string, clash: DropClash) => {
      calls.push({ id, dir, clash });
      const reply = replies.shift();
      if (!reply || reply instanceof Error) throw reply ?? new Error("no reply");
      return reply;
    },
  } as unknown as Transport;
  const importer = new DropImporter({
    transport,
    project: "p1",
    dirAt,
    copied: (paths) => seen.copied.push(paths),
    fail: (message) => seen.failed.push(message),
    tell: (message) => seen.told.push(message),
  });
  return { importer, calls, seen };
}

const drop: WindowDrop = { id: "d1", paths: ["/out/a.png"], x: 5, y: 6 };

describe("DropImporter", () => {
  it("highlights the folder under a drag from outside, and nothing off the tree", () => {
    const { importer } = setup([], (at) => (at.y < 10 ? "src" : null));
    importer.hover({ x: 1, y: 2 });
    expect(importer.over).toBe("src");
    importer.hover({ x: 1, y: 20 });
    expect(importer.over).toBeNull();
    importer.hover({ x: 1, y: 2 });
    importer.hover(null);
    expect(importer.over).toBeNull();
  });

  it("copies a drop on the tree into the folder under it and shows the copies", async () => {
    const { importer, calls, seen } = setup([{ kind: "copied", paths: ["src/a.png"], skipped_links: ["a/l"] }]);
    importer.hover({ x: 1, y: 2 });
    await importer.drop(drop);
    expect(importer.over).toBeNull();
    expect(calls).toEqual([{ id: "d1", dir: "src", clash: "ask" }]);
    expect(seen.copied).toEqual([["src/a.png"]]);
    expect(seen.told).toEqual(["Skipped “a/l”: links are not copied."]);
  });

  it("ignores a drop outside the tree", async () => {
    const { importer, calls } = setup([], () => null);
    await importer.drop(drop);
    expect(calls).toEqual([]);
  });

  it("asks before replacing, and copies only with the answer", async () => {
    const clashes: DropImport = { kind: "clashes", names: ["a.png"] };
    const { importer, calls, seen } = setup([
      clashes,
      { kind: "copied", paths: ["src/a copy.png"], skipped_links: [] },
      clashes,
    ]);
    await importer.drop(drop);
    expect(importer.asking?.text).toBe("“a.png” already exists in src.");
    expect(seen.copied).toEqual([]);
    await importer.answer("keep_both");
    expect(importer.asking).toBeNull();
    expect(calls.map((c) => c.clash)).toEqual(["ask", "keep_both"]);
    expect(seen.copied).toEqual([["src/a copy.png"]]);

    await importer.drop(drop);
    await importer.answer(null);
    expect(importer.asking).toBeNull();
    expect(calls.map((c) => c.clash)).toEqual(["ask", "keep_both", "ask"]);
  });

  it("reports a refused import", async () => {
    const { importer, seen } = setup([new Error("That drop is over. Drop the files again.")]);
    await importer.drop(drop);
    expect(seen.failed).toEqual(["That drop is over. Drop the files again."]);
  });

  it("is not there without the window's import", () => {
    const importer = new DropImporter({
      transport: {} as Transport,
      project: "p1",
      dirAt: () => "",
      copied: () => {},
      fail: () => {},
      tell: () => {},
    });
    expect(importer.available).toBe(false);
  });
});
