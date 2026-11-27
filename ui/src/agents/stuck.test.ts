import { expect, it } from "vitest";
import { interruptedLine, stuckLine } from "./stuck";

it("names what the agent repeats, how often, and why it may be stuck", () => {
  const lines = [
    stuckLine({
      kind: "repeating",
      name: "Bash",
      call: { kind: "execute", command: "cargo test\n--all" },
      times: 3,
    }),
    stuckLine({
      kind: "repeating",
      name: "Read",
      call: { kind: "read", paths: ["/demo/src/lib.rs"] },
      times: 4,
    }),
    stuckLine({
      kind: "repeating",
      name: "mcp__x__y",
      call: { kind: "other" },
      times: 3,
    }),
    stuckLine({ kind: "failing", times: 5 }),
    stuckLine({ kind: "silent", minutes: 10 }),
    stuckLine(null),
  ];
  expect(lines).toEqual([
    "It may be going in circles: it ran cargo test 3 times with the same result.",
    "It may be going in circles: it read lib.rs 4 times with the same result.",
    "It may be going in circles: it used mcp__x__y 3 times with the same result.",
    "It may be stuck: its last 5 steps failed.",
    "Nothing new for 10 minutes. A long command may still run, or it may be stuck.",
    null,
  ]);
  expect(
    stuckLine({
      kind: "repeating",
      name: "Bash",
      call: { kind: "execute", command: "x".repeat(80) },
      times: 3,
    }),
  ).toContain(`${"x".repeat(59)}…`);
});

it("counts the agents a quit cut off", () => {
  expect([0, 1, 2].map(interruptedLine)).toEqual([
    null,
    "1 agent stopped when Ovra closed.",
    "2 agents stopped when Ovra closed.",
  ]);
});
