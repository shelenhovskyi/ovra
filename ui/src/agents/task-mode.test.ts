import { describe, expect, it } from "vitest";
import { rememberedMode, rememberMode } from "./task-mode";

describe("task mode", () => {
  it("remembers the last mode per project and survives a blocked storage", () => {
    const items = new Map<string, string>();
    const store = {
      getItem: (key: string) => items.get(key) ?? null,
      setItem: (key: string, value: string) => void items.set(key, value),
    } as Storage;
    expect(rememberedMode("p1", store)).toBe("here");
    rememberMode("p1", "split", store);
    expect(rememberedMode("p1", store)).toBe("split");
    expect(rememberedMode("p2", store)).toBe("here");

    const blocked = {
      getItem: () => {
        throw new Error("denied");
      },
      setItem: () => {
        throw new Error("full");
      },
    } as unknown as Storage;
    rememberMode("p1", "split", blocked);
    expect(rememberedMode("p1", blocked)).toBe("here");
  });
});
