import { describe, expect, it } from "vitest";
import type { CoreEvent, Request } from "../lib/protocol";
import { CoreError, type Transport } from "../lib/transport";
import { exists, formatSize, loadView, touches } from "./load";

function fakeTransport(reply: (request: Request) => unknown): Transport & { calls: Request[] } {
  const calls: Request[] = [];
  return {
    calls,
    call: (request) => {
      calls.push(request);
      try {
        return Promise.resolve(reply(request)) as never;
      } catch (err) {
        return Promise.reject(err);
      }
    },
    onEvent: () => () => {},
  };
}

describe("loadView", () => {
  it("reads the file and picks a view", async () => {
    const transport = fakeTransport(() => ({
      type: "fs.file",
      data: { path: "README.md", size: 3, mtime_ms: 1, body: { kind: "text", text: "# x" } },
    }));
    await expect(loadView(transport, "p1", "README.md")).resolves.toEqual({
      view: { kind: "markdown", text: "# x" },
      size: 3,
    });
    expect(transport.calls).toEqual([{ method: "fs.read_file", params: { project: "p1", path: "README.md" } }]);
  });

  it("reports missing files and other errors", async () => {
    const missing = fakeTransport(() => {
      throw new CoreError({ code: "not_found", message: "gone" });
    });
    await expect(loadView(missing, "p1", "a.md")).resolves.toEqual({ error: "a.md no longer exists." });
    const broken = fakeTransport(() => {
      throw new CoreError({ code: "unavailable", message: "no fs" });
    });
    await expect(loadView(broken, "p1", "a.md")).resolves.toEqual({ error: "Could not read a.md: no fs" });
    const offline = fakeTransport(() => {
      throw new Error("connection to core closed");
    });
    await expect(loadView(offline, "p1", "a.md")).resolves.toEqual({
      error: "Not connected to Ovra's core, so a.md cannot be read right now.",
    });
  });
});

describe("touches", () => {
  it("matches fs.changed for the open file only", () => {
    const changed = { type: "fs.changed", data: { project: "p1", paths: ["a.md", "b.md"] } } satisfies CoreEvent;
    expect(touches(changed, "p1", "a.md")).toBe(true);
    expect(touches(changed, "p2", "a.md")).toBe(false);
    expect(touches(changed, "p1", "c.md")).toBe(false);
    expect(touches({ type: "fs.git_changed", data: { project: "p1" } }, "p1", "a.md")).toBe(false);
  });

  it("matches info.resync, after which everything may be stale", () => {
    expect(touches({ type: "info.resync", data: { skipped: 3 } }, "p1", "a.md")).toBe(true);
  });
});

describe("exists", () => {
  it("looks the file up in its parent's listing", async () => {
    const transport = fakeTransport((request) => {
      if (request.method !== "fs.list_dir") throw new Error("unexpected");
      if (request.params.path === "gone") throw new CoreError({ code: "not_found", message: "gone" });
      return {
        type: "fs.dir",
        data: { path: request.params.path, entries: [{ name: "b.md", kind: "file", ignored: false, git: null }] },
      };
    });
    await expect(exists(transport, "p1", "docs/b.md")).resolves.toBe(true);
    await expect(exists(transport, "p1", "docs/nope.md")).resolves.toBe(false);
    await expect(exists(transport, "p1", "gone/b.md")).resolves.toBe(false);
    await expect(exists(transport, "p1", "b.md")).resolves.toBe(true);
    expect(transport.calls.map((c) => ("params" in c && "path" in c.params ? c.params.path : null))).toEqual([
      "docs",
      "docs",
      "gone",
      "",
    ]);
  });
});

describe("formatSize", () => {
  it("uses bytes, KB and MB", () => {
    expect(formatSize(1)).toBe("1 byte");
    expect(formatSize(900)).toBe("900 bytes");
    expect(formatSize(200 * 1024)).toBe("200 KB");
    expect(formatSize(6 * 1024 * 1024)).toBe("6.0 MB");
  });
});
