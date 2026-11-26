import { flushSync, mount, unmount } from "svelte";
import { afterEach, describe, expect, it, vi } from "vitest";
import type { AgentMessage } from "../lib/protocol";
import { resultText, turnsOf, turnsText, undoneTurns } from "./checkpoints";
import Transcript from "./Transcript.svelte";

const person = (text: string): AgentMessage => ({ kind: "user_text", text, from: "person" });
const ovra = (text: string): AgentMessage => ({ kind: "user_text", text, from: "ovra" });
const said = (text: string): AgentMessage => ({ kind: "assistant_text", text });
const NOTE = "The files were set back to how they were before your turn 2; changes from turns 2 to 3 are undone.";

/** Four turns; the fourth carries the note of a go-back to turn 2. */
const four = [
  person("Write one"),
  said("Wrote one."),
  person("Write two"),
  said("Wrote two."),
  person("Write three"),
  said("Wrote three."),
  ovra(NOTE),
  person("Write two again"),
  said("Wrote it."),
];

describe("turns", () => {
  it("counts each run of user texts as one turn, as the core does", () => {
    const turns = turnsOf([said("Hello"), ...four]);
    expect([...turns.values()]).toEqual([1, 1, 2, 2, 3, 3, 4, 4, 4]);
  });

  it("marks every turn from the one gone back to while no message was sent since", () => {
    expect(undoneTurns(four.slice(0, 6), 2, true)).toEqual({ from: 2, to: 3 });
    expect(undoneTurns(four.slice(0, 6), null, false)).toBeNull();
  });

  it("ends the undone turns at the message that carried Ovra's note", () => {
    expect(undoneTurns(four, 2, false)).toEqual({ from: 2, to: 3 });
    // Gone back again before sending: every turn from it on.
    expect(undoneTurns(four, 2, true)).toEqual({ from: 2, to: 4 });
  });

  it("says what was done in plain words", () => {
    expect(turnsText(2, 2)).toBe("turn 2");
    expect(turnsText(2, 4)).toBe("turns 2 to 4");
    expect(resultText(["two.txt"], ["notes.md"], false)).toBe(
      "Set back: two.txt. Kept as they are (changed since): notes.md.",
    );
    expect(resultText([], [], false)).toBe("Nothing to set back: no file has only the agent's changes.");
    expect(resultText(["two.txt"], [], true)).toBe("Put back: two.txt.");
  });
});

describe("Transcript", () => {
  let app: Record<string, unknown> | null = null;
  afterEach(() => {
    if (app) unmount(app);
    app = null;
    document.body.replaceChildren();
  });

  const backs = () => [...document.querySelectorAll<HTMLButtonElement>("[data-testid=go-back]")];

  it("offers Go back to here on the person's message of each kept turn, and confirms in plain words", () => {
    const ongoback = vi.fn();
    app = mount(Transcript, {
      target: document.body,
      props: { messages: four.slice(0, 6), checkpoints: new Set([2, 3]), ongoback },
    });
    flushSync();
    // Turn 1 was dropped: no button on it.
    expect(backs()).toHaveLength(2);
    backs()[0]?.click();
    flushSync();
    const confirm = document.querySelector("[data-testid=go-back-confirm]");
    expect(confirm?.textContent?.replace(/\s+/g, " ").trim()).toBe(
      "Undo the agent's changes from turns 2 to 3? Your own changes are kept. " +
        "Ignored files, such as node_modules, are not restored. Cancel Go back",
    );
    // Escape (by its code, whatever the layout) closes it.
    confirm?.dispatchEvent(new KeyboardEvent("keydown", { code: "Escape", key: "Escape", bubbles: true }));
    flushSync();
    expect(document.querySelector("[data-testid=go-back-confirm]")).toBeNull();
    backs()[1]?.click();
    flushSync();
    [...document.querySelectorAll("button")].find((b) => b.textContent === "Go back")?.click();
    expect(ongoback).toHaveBeenCalledWith(3, "Write three");
  });

  it("says to stop the agent first while a turn runs", () => {
    const ongoback = vi.fn();
    app = mount(Transcript, {
      target: document.body,
      props: {
        messages: four.slice(0, 4),
        checkpoints: new Set([1, 2]),
        goBackBlocked: "Stop the agent first",
        ongoback,
      },
    });
    flushSync();
    expect(backs()[0]?.getAttribute("aria-disabled")).toBe("true");
    backs()[0]?.click();
    flushSync();
    expect(document.querySelector("[data-testid=go-back-confirm]")).toBeNull();
    expect(document.querySelector("[data-testid=transcript] .why")?.textContent).toBe("Stop the agent first");
    expect(ongoback).not.toHaveBeenCalled();
  });

  it("greys the undone turns under a divider", () => {
    app = mount(Transcript, {
      target: document.body,
      props: { messages: four, undone: { from: 2, to: 3 } },
    });
    flushSync();
    const rows = [...document.querySelectorAll("[data-testid=transcript] > li")];
    expect(rows.map((li) => (li.classList.contains("undone") ? "undone" : li.textContent?.trim()))).toEqual([
      "You Write one",
      "Agent Wrote one.",
      "Files set back to before this message",
      "undone",
      "undone",
      "undone",
      "undone",
      expect.stringContaining("The files were set back"),
      "You Write two again",
      "Agent Wrote it.",
    ]);
    // Without a handler, nothing to go back with.
    expect(backs()).toHaveLength(0);
  });
});
