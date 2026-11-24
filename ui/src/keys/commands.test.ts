import { describe, expect, it } from "vitest";
import { listCommands, registerCommand } from "./commands";

describe("command registry", () => {
  it("registers, lists by title and unregisters", () => {
    const offB = registerCommand({ id: "t.b", title: "Beta", run: () => {} });
    const offA = registerCommand({ id: "t.a", title: "Alpha", run: () => {} });
    expect(listCommands().map((c) => c.id)).toEqual(["t.a", "t.b"]);
    offA();
    offB();
    expect(listCommands()).toEqual([]);
  });
});
