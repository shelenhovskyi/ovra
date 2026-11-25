import { afterEach, expect, it, vi } from "vitest";
import { warmTips } from "./tips";

afterEach(() => {
  vi.useRealTimers();
});

it("warms a group after 600 ms over a tooltip and cools a second after the pointer leaves", () => {
  vi.useFakeTimers();
  const group = document.createElement("nav");
  const button = group.appendChild(document.createElement("button"));
  button.className = "tip";
  const off = warmTips(group);

  button.dispatchEvent(new PointerEvent("pointerover", { bubbles: true }));
  vi.advanceTimersByTime(599);
  expect("warm" in group.dataset).toBe(false);
  vi.advanceTimersByTime(1);
  expect("warm" in group.dataset).toBe(true);

  group.dispatchEvent(new PointerEvent("pointerleave"));
  vi.advanceTimersByTime(900);
  // Back in time: still warm, and it stays so.
  button.dispatchEvent(new PointerEvent("pointerover", { bubbles: true }));
  vi.advanceTimersByTime(2000);
  expect("warm" in group.dataset).toBe(true);
  group.dispatchEvent(new PointerEvent("pointerleave"));
  vi.advanceTimersByTime(1000);
  expect("warm" in group.dataset).toBe(false);
  off();
});
