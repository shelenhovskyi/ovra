import { flushSync, mount, unmount } from "svelte";
import { afterEach, describe, expect, it } from "vitest";
import DropClash from "./DropClash.svelte";

let cleanup: (() => void) | null = null;
afterEach(() => {
  cleanup?.();
  cleanup = null;
  document.body.innerHTML = "";
});

function show() {
  const answers: (string | null)[] = [];
  const target = document.body.appendChild(document.createElement("div"));
  const outside = document.body.appendChild(document.createElement("button"));
  const app = mount(DropClash, {
    target,
    props: { question: "“a.png” already exists in src.", onanswer: (clash) => answers.push(clash) },
  });
  cleanup = () => unmount(app);
  flushSync();
  const card = target.querySelector<HTMLElement>("[role=alertdialog]") as HTMLElement;
  const key = (code: string, shiftKey = false) =>
    (document.activeElement ?? card).dispatchEvent(new KeyboardEvent("keydown", { code, shiftKey, bubbles: true }));
  return { answers, card, outside, key };
}

describe("DropClash", () => {
  it("has Keep both focused, so Enter never replaces, and Tab cycles the buttons", () => {
    const { answers, key } = show();
    expect(document.activeElement?.textContent).toBe("Keep both");
    key("Tab");
    expect(document.activeElement?.textContent).toBe("Cancel");
    key("Tab");
    expect(document.activeElement?.textContent).toBe("Replace");
    key("Tab", true);
    expect(document.activeElement?.textContent).toBe("Cancel");
    (document.activeElement as HTMLButtonElement).click();
    expect(answers).toEqual([null]);
  });

  it("answers with the button clicked", () => {
    const { answers, card } = show();
    [...card.querySelectorAll("button")].find((b) => b.textContent === "Replace")?.click();
    [...card.querySelectorAll("button")].find((b) => b.textContent === "Keep both")?.click();
    expect(answers).toEqual(["replace", "keep_both"]);
  });

  it("cancels on Escape", () => {
    const { answers, key } = show();
    key("Escape");
    expect(answers).toEqual([null]);
  });

  it("cancels when focus leaves the card, not when it moves inside", () => {
    const { answers, card, outside } = show();
    (card.querySelector("button") as HTMLButtonElement).focus();
    expect(answers).toEqual([]);
    outside.focus();
    expect(answers).toEqual([null]);
  });
});
