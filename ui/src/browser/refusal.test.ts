import { describe, expect, it } from "vitest";
import { refusalText, sentence } from "./refusal";

describe("refusalText", () => {
  it("says what helps for each code and keeps the core's detail", () => {
    const chromium = refusalText({ code: "no_chromium", reason: "no browser at /opt/chrome" });
    expect(chromium).toEqual({
      headline: "The agent browser needs Chromium or Google Chrome. Install one, then restart Ovra.",
      detail: "No browser at /opt/chrome.",
      retry: false,
    });
    expect(refusalText({ code: "session_ended", reason: "the session has ended" }).detail).toBeNull();
    // Without a code, the core's words are all there is, and trying again may help.
    expect(refusalText({ code: null, reason: "the browser has closed" })).toEqual({
      headline: "The browser has closed.",
      detail: null,
      retry: true,
    });
    expect(sentence(" done! ")).toBe("Done!");
  });
});
