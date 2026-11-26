import { flushSync, mount, unmount } from "svelte";
import { afterEach, expect, it, vi } from "vitest";
import AgentText from "./AgentText.svelte";

afterEach(() => {
  vi.useRealTimers();
  document.body.replaceChildren();
});

it("renders streaming text at once, then at most every 100 ms with the latest text", () => {
  vi.useFakeTimers();
  const props = $state({ text: "First", streaming: true });
  const app = mount(AgentText, { target: document.body, props });
  flushSync();
  const shown = () => document.querySelector("[data-testid=agent-text]")?.textContent;
  expect(shown()).toBe("First");
  props.text = "First **two**";
  flushSync();
  props.text = "First **two** three";
  flushSync();
  // Each delta would render the whole text again: they wait for the next turn.
  expect(shown()).toBe("First");
  vi.advanceTimersByTime(100);
  flushSync();
  expect(shown()).toBe("First two three");
  expect(document.querySelector("strong")?.textContent).toBe("two");
  // The complete message shows as soon as it arrives.
  props.text = "Done.";
  props.streaming = false;
  flushSync();
  expect(shown()).toBe("Done.");
  unmount(app);
});
