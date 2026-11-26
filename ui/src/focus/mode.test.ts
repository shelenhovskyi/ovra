import { afterEach, describe, expect, it } from "vitest";
import { FocusMode, focusMode, leaveFocus } from "./mode.svelte";

afterEach(() => localStorage.removeItem("ovra.focus"));

describe("FocusMode", () => {
  it("is remembered in this browser", () => {
    expect(new FocusMode().on).toBe(false);
    const mode = new FocusMode();
    mode.set(true);
    expect(localStorage.getItem("ovra.focus")).toBe("1");
    expect(new FocusMode().on).toBe(true);
    mode.toggle();
    expect(localStorage.getItem("ovra.focus")).toBeNull();
    expect(new FocusMode().on).toBe(false);
  });

  it("is left by a command that shows a panel Focus hides", () => {
    focusMode.set(true);
    leaveFocus();
    expect(focusMode.on).toBe(false);
    leaveFocus();
    expect(focusMode.on).toBe(false);
  });
});
