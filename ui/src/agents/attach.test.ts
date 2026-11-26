import { afterEach, describe, expect, it, vi } from "vitest";
import { PATHS_DRAG_TYPE } from "../lib/drag";
import type { Request } from "../lib/protocol";
import type { Point, Transport, WindowDrop } from "../lib/transport";
import {
  attachFiles,
  copyDrop,
  dropZone,
  insidePaths,
  itemFor,
  MAX_IMAGE_BYTES,
  pastedFiles,
  relativeTo,
  secretLooking,
  thumbnail,
} from "./attach";

/** A transport that answers the calls these helpers make and records them. */
function fake(withDrops = true) {
  const calls: Request[] = [];
  const over = new Set<(at: Point | null) => void>();
  const drops = new Set<(drop: WindowDrop) => void>();
  const transport = {
    call: async (request: Request) => {
      calls.push(request);
      switch (request.method) {
        case "fs.list_dir":
          if (request.params.path !== "src") throw new Error("not a folder");
          return { type: "fs.dir", data: { path: "src", entries: [] } };
        case "context.attachments":
          return { type: "context.attached", data: { path: ".ovra-attachments" } };
        case "context.check":
          return {
            type: "context.checked",
            data: {
              notes: request.params.items.map((i) =>
                "path" in i && /id_ed25519|\.env$/.test(i.path) ? "secret_file" : null,
              ),
            },
          };
        case "context.attach":
          return { type: "context.attached", data: { path: `.ovra-attachments/${request.params.name}` } };
        case "fs.read_file": {
          const path = request.params.path;
          const size = path === "big.png" ? MAX_IMAGE_BYTES + 1 : 3;
          const body = path.startsWith("text")
            ? { kind: "text" as const, text: "hi" }
            : { kind: "binary" as const, mime: "image/png", base64: "iVBO" };
          return { type: "fs.file", data: { path, size, mtime_ms: 1, body } };
        }
        default:
          throw new Error(`unexpected ${request.method}`);
      }
    },
    onEvent: () => () => {},
    ...(withDrops
      ? {
          onDragOver: (l: (at: Point | null) => void) => {
            over.add(l);
            return () => over.delete(l);
          },
          onDrop: (l: (drop: WindowDrop) => void) => {
            drops.add(l);
            return () => drops.delete(l);
          },
          importDrop: vi.fn(async () => ({
            kind: "copied" as const,
            paths: [".ovra-attachments/shot.png"],
            skipped_links: [],
          })),
        }
      : {}),
  };
  return {
    calls,
    transport: transport as unknown as Transport & { importDrop: ReturnType<typeof vi.fn> },
    hover: (at: Point | null) => {
      for (const l of over) l(at);
    },
    drop: (drop: WindowDrop) => {
      for (const l of drops) l(drop);
    },
  };
}

afterEach(() => {
  Reflect.deleteProperty(document, "elementFromPoint");
  document.body.innerHTML = "";
  vi.restoreAllMocks();
});

describe("relativeTo", () => {
  it("gives project paths only for what is inside the project", () => {
    expect(relativeTo("/work/app", "/work/app/src/lib.rs")).toBe("src/lib.rs");
    expect(relativeTo("/work/app/", "/work/app/src/")).toBe("src");
    expect(relativeTo("/work/app", "/work/app")).toBeNull();
    expect(relativeTo("/work/app", "/work/apple/x")).toBeNull();
    expect(relativeTo("/work/app", "/work/app/../secret")).toBeNull();
    expect(relativeTo("C:\\Work\\App", "c:\\work\\app\\src\\main.rs")).toBe("src/main.rs");
  });
});

describe("files from a drop or a paste", () => {
  it("names project files and folders as they are", async () => {
    const { transport, calls } = fake();
    const drop = { id: "d1", x: 0, y: 0, paths: ["/work/app/src", "/work/app/src/lib.rs"] };
    expect(insidePaths("/work/app", drop)).toEqual(["src", "src/lib.rs"]);
    expect(insidePaths("/work/app", { ...drop, paths: ["/work/app/a", "/tmp/b"] })).toBeNull();
    expect(await itemFor(transport, "p1", "src")).toEqual({ kind: "folder", path: "src" });
    expect(await itemFor(transport, "p1", "src/lib.rs")).toEqual({ kind: "file", path: "src/lib.rs" });
    expect(calls.some((c) => c.method === "context.attachments")).toBe(false);
    expect(transport.importDrop).not.toHaveBeenCalled();
  });

  it("copies files from elsewhere into the attachments folder", async () => {
    const { transport } = fake();
    const drop = { id: "d2", x: 0, y: 0, paths: ["/Users/me/Desktop/shot.png"] };
    expect(await copyDrop(transport, "p1", drop)).toEqual([".ovra-attachments/shot.png"]);
    expect(transport.importDrop).toHaveBeenCalledWith("d2", "p1", ".ovra-attachments", "keep_both");
  });

  it("finds the names whose copy would look like a secret store, by the file and its folder", async () => {
    const { transport, calls } = fake();
    const risky = await secretLooking(transport, "p1", [
      "/Users/me/.ssh/id_ed25519",
      "/Users/me/Desktop/shot.png",
      ".env",
    ]);
    expect(risky).toEqual(["id_ed25519", ".env"]);
    // More than the core's 20 items a call go in batches.
    const many = Array.from({ length: 25 }, (_, i) => `/tmp/f${i}.txt`);
    expect(await secretLooking(transport, "p1", [...many, "/x/.env"])).toEqual([".env"]);
    const checks = calls.filter((c) => c.method === "context.check");
    expect(checks.slice(-2).map((c) => ("items" in c.params ? c.params.items.length : 0))).toEqual([20, 6]);
    expect(calls.filter((c) => c.method === "context.check")[0]).toMatchObject({
      params: {
        items: [
          { kind: "file", path: ".ovra-attachments/.ssh/id_ed25519" },
          { kind: "file", path: ".ovra-attachments/Desktop/shot.png" },
          { kind: "file", path: ".ovra-attachments/.env" },
        ],
      },
    });
  });

  it("saves pasted files and names a nameless one by its type", async () => {
    const { transport, calls } = fake();
    const shot = new File(["png"], "", { type: "image/png" });
    expect(await attachFiles(transport, "p1", [shot])).toEqual([".ovra-attachments/pasted.png"]);
    expect(calls.at(-1)).toEqual({
      method: "context.attach",
      params: { project: "p1", name: "pasted.png", data: btoa("png") },
    });
    const huge = { name: "big.mov", size: 11 * 1024 * 1024, type: "video/quicktime" } as File;
    await expect(attachFiles(transport, "p1", [huge])).rejects.toThrow("big.mov is larger than 10 MB.");
  });

  it("takes a paste's files unless it carries other text", () => {
    const file = new File(["x"], "shot.png");
    const data = (text: string, files: File[]) => ({ files, getData: () => text }) as unknown as DataTransfer;
    expect(pastedFiles(data("", [file]))).toEqual([file]);
    expect(pastedFiles(data("shot.png", [file]))).toEqual([file]);
    expect(pastedFiles(data("a table from a sheet", [file]))).toEqual([]);
    expect(pastedFiles(data("text", []))).toEqual([]);
  });
});

describe("dropZone", () => {
  function zone(withDrops = true) {
    const box = document.createElement("form");
    const inner = document.createElement("textarea");
    box.append(inner);
    const outside = document.createElement("div");
    document.body.append(box, outside);
    const seen = { over: [] as boolean[], window: [] as WindowDrop[], rows: [] as string[][], files: [] as File[][] };
    const t = fake(withDrops);
    const off = dropZone(box, t.transport, {
      over: (on) => seen.over.push(on),
      window: (drop) => seen.window.push(drop),
      rows: (paths) => seen.rows.push(paths),
      files: (files) => seen.files.push(files),
    });
    return { box, inner, outside, seen, off, ...t };
  }

  it("takes a window drop only over the box", () => {
    const { inner, outside, seen, hover, drop, off } = zone();
    let under: Element = outside;
    document.elementFromPoint = () => under;
    hover({ x: 1, y: 1 });
    under = inner;
    hover({ x: 2, y: 2 });
    hover({ x: 3, y: 3 });
    drop({ id: "d", x: 3, y: 3, paths: ["/a"] });
    under = outside;
    drop({ id: "e", x: 9, y: 9, paths: ["/b"] });
    expect(seen.over).toEqual([true, false]);
    expect(seen.window.map((d) => d.id)).toEqual(["d"]);
    off();
    under = inner;
    drop({ id: "f", x: 3, y: 3, paths: ["/c"] });
    expect(seen.window).toHaveLength(1);
  });

  it("takes file tree rows, and a page's own files only without the desktop app", () => {
    const event = (type: string, data: Record<string, string>, files: File[] = []) => {
      const e = new Event(type, { bubbles: true, cancelable: true }) as DragEvent;
      Object.defineProperty(e, "dataTransfer", {
        value: { types: Object.keys(data), getData: (t: string) => data[t] ?? "", files, dropEffect: "none" },
      });
      return e;
    };
    const desktop = zone();
    desktop.inner.dispatchEvent(event("drop", { [PATHS_DRAG_TYPE]: "src/a.rs\nsrc/b.rs" }));
    expect(desktop.seen.rows).toEqual([["src/a.rs", "src/b.rs"]]);
    const fromFinder = event("drop", { Files: "" }, [new File(["x"], "x.png")]);
    desktop.inner.dispatchEvent(fromFinder);
    expect(fromFinder.defaultPrevented).toBe(false);
    expect(desktop.seen.files).toEqual([]);
    desktop.off();

    const browser = zone(false);
    const file = new File(["x"], "x.png");
    browser.inner.dispatchEvent(event("dragover", { Files: "" }));
    browser.inner.dispatchEvent(event("drop", { Files: "" }, [file]));
    expect(browser.seen.over).toEqual([true, false]);
    expect(browser.seen.files).toEqual([[file]]);
  });
});

describe("images", () => {
  it("become image items with a thumbnail when a model takes them, else stay files", async () => {
    const { transport } = fake();
    expect(await itemFor(transport, "p1", "shots/a.PNG")).toEqual({ kind: "image", path: "shots/a.PNG" });
    expect(thumbnail("p1", "shots/a.PNG")).toBe("data:image/png;base64,iVBO");
    expect(thumbnail("p2", "shots/a.PNG")).toBeUndefined();
    expect(await itemFor(transport, "p1", "big.png")).toEqual({ kind: "file", path: "big.png" });
    expect(await itemFor(transport, "p1", "text.png")).toEqual({ kind: "file", path: "text.png" });
    expect(await itemFor(transport, "p1", "notes.txt")).toEqual({ kind: "file", path: "notes.txt" });
    expect(await itemFor(transport, "p1", "src")).toEqual({ kind: "folder", path: "src" });
  });
});
