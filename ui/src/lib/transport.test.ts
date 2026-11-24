import { describe, expect, it } from "vitest";
import { FrameRouter } from "./transport";

describe("FrameRouter", () => {
  it("matches replies to calls by id", async () => {
    const sent: string[] = [];
    const router = new FrameRouter((frame) => sent.push(frame));
    const reply = router.call({ method: "get_info" });
    expect(JSON.parse(sent[0] ?? "")).toEqual({ id: 1, method: "get_info" });
    router.deliver(
      JSON.stringify({ id: 1, result: { type: "info", data: { name: "ovra", version: "0.1.0", mode: "test" } } }),
    );
    await expect(reply).resolves.toEqual({ type: "info", data: { name: "ovra", version: "0.1.0", mode: "test" } });
  });

  it("rejects on error frames", async () => {
    const router = new FrameRouter(() => {});
    const reply = router.call({ method: "get_info" });
    router.deliver(JSON.stringify({ id: 1, error: { code: "bad_request", message: "nope" } }));
    await expect(reply).rejects.toThrow("bad_request: nope");
  });

  it("delivers events to listeners until unsubscribed", () => {
    const router = new FrameRouter(() => {});
    const seen: string[] = [];
    const off = router.onEvent((event) => seen.push(event.type));
    router.deliver(JSON.stringify({ event: { type: "ready" } }));
    off();
    router.deliver(JSON.stringify({ event: { type: "ready" } }));
    expect(seen).toEqual(["ready"]);
  });

  it("fails pending calls when the connection drops", async () => {
    const router = new FrameRouter(() => {});
    const reply = router.call({ method: "get_info" });
    router.failAll(new Error("closed"));
    await expect(reply).rejects.toThrow("closed");
  });
});
