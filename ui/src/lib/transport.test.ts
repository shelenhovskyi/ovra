import { describe, expect, it } from "vitest";
import { CoreError, FrameRouter } from "./transport";

describe("FrameRouter", () => {
  it("matches replies to calls by id", async () => {
    const sent: string[] = [];
    const router = new FrameRouter((frame) => sent.push(frame));
    const reply = router.call({ method: "info.get" });
    expect(JSON.parse(sent[0] ?? "")).toEqual({ id: 1, method: "info.get" });
    router.deliver(
      JSON.stringify({ id: 1, result: { type: "info", data: { name: "ovra", version: "0.1.0", mode: "test" } } }),
    );
    await expect(reply).resolves.toEqual({ type: "info", data: { name: "ovra", version: "0.1.0", mode: "test" } });
  });

  it("rejects on error frames", async () => {
    const router = new FrameRouter(() => {});
    const reply = router.call({ method: "info.get" });
    router.deliver(JSON.stringify({ id: 1, error: { code: "bad_request", message: "nope" } }));
    await expect(reply).rejects.toThrow("bad_request: nope");
    await expect(reply).rejects.toBeInstanceOf(CoreError);
  });

  it("delivers events to listeners until unsubscribed", () => {
    const router = new FrameRouter(() => {});
    const seen: string[] = [];
    const off = router.onEvent((event) => seen.push(event.type));
    router.deliver(JSON.stringify({ event: { type: "info.ready" } }));
    off();
    router.deliver(JSON.stringify({ event: { type: "info.ready" } }));
    expect(seen).toEqual(["info.ready"]);
  });

  it("fails pending calls when the connection drops", async () => {
    const router = new FrameRouter(() => {});
    const reply = router.call({ method: "info.get" });
    router.failAll(new Error("closed"));
    await expect(reply).rejects.toThrow("closed");
  });
});
