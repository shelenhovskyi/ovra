import { describe, expect, it } from "vitest";
import type { AgentMessage } from "../lib/protocol";
import { nothingLeft, sessionSummary, taskSummary } from "./summary";

describe("taskSummary", () => {
  it("reads the three lines, in Markdown or plain", () => {
    expect(taskSummary("Fixed it.\n\nDone: renamed the flag\nChecked: just test passed\nLeft: Nothing")).toEqual({
      done: "renamed the flag",
      checked: "just test passed",
      left: "Nothing",
    });
    expect(taskSummary("- **Done:** a\n- **Verified**: b\n* __Remaining:__ c")).toEqual({
      done: "a",
      checked: "b",
      left: "c",
    });
  });

  it("takes the last summary, leaves out lines it lacks and has none without a Done line", () => {
    expect(taskSummary("Done: first\nLeft: x\n\nDone: second")).toEqual({ done: "second", checked: null, left: null });
    expect(taskSummary("Checked: tests\nLeft: nothing")).toBeNull();
    expect(taskSummary("Done:\nNote: nothing to see")).toBeNull();
    expect(taskSummary(null)).toBeNull();
    expect(taskSummary(`Done: ${"x".repeat(300)}`)?.done).toHaveLength(240);
  });

  it("says when nothing is left", () => {
    expect(nothingLeft("Nothing.")).toBe(true);
    expect(nothingLeft("none")).toBe(true);
    expect(nothingLeft("the docs")).toBe(false);
    expect(nothingLeft(null)).toBe(false);
  });

  it("prefers the review's summary to the agent's last answer", () => {
    const messages: AgentMessage[] = [
      { kind: "user_text", text: "go", from: "person" },
      { kind: "assistant_text", text: "Done: from the answer" },
    ];
    expect(sessionSummary({ summary: "Done: from review" }, messages)?.done).toBe("from review");
    expect(sessionSummary({ summary: "changed a.rs" }, messages)?.done).toBe("from the answer");
    expect(sessionSummary(undefined, undefined)).toBeNull();
  });
});
