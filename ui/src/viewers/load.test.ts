import { describe, expect, it } from "vitest";
import type { CoreEvent, Request } from "../lib/protocol";
import { CoreError, type Transport } from "../lib/transport";
import { loadView, touches } from "./load";

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
    await expect(loadView(transport, "p1", "README.md")).resolves.toEqual({ view: { kind: "markdown", text: "# x" } });
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
    await expect(loadView(broken, "p1", "a.md")).resolves.toEqual({ error: "unavailable: no fs" });
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
});
