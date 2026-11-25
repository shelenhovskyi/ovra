import { describe, expect, it } from "vitest";
import type { AgentMessage } from "../lib/protocol";
import { reportFile, reportOf, reportPath, taskPrompt } from "./report";

const say = (text: string, parent?: string): AgentMessage => ({ kind: "assistant_text", text, parent });

describe("research reports", () => {
  it("is the assistant's text since the last message it was sent, without subagents", () => {
    expect(reportOf(undefined)).toBeNull();
    expect(reportOf([{ kind: "user_text", text: "q", from: "person" }])).toBeNull();
    const messages: AgentMessage[] = [
      { kind: "user_text", text: "q", from: "person" },
      say("old"),
      { kind: "user_text", text: "more?", from: "person" },
      say("Looking."),
      { kind: "tool_use", id: "t1", name: "Grep", input: {} },
      say("inner", "t9"),
      say("## Answer\nIt works."),
      { kind: "turn_end", usage: { input_tokens: 0, output_tokens: 0, cost_micro_usd: null } },
    ];
    expect(reportOf(messages)).toBe("Looking.\n\n## Answer\nIt works.");
  });

  it("is saved under docs/research with the date and a plain name, numbered when taken", () => {
    const day = new Date("2026-11-25T23:00:00Z");
    expect(reportPath("How does Review work?", day)).toBe("docs/research/2026-11-25-how-does-review-work.md");
    expect(reportPath("How does Review work?", day, 2)).toBe("docs/research/2026-11-25-how-does-review-work-2.md");
    expect(reportPath("???", day)).toBe("docs/research/2026-11-25-report.md");
    expect(reportPath("a".repeat(80), day)).toBe(`docs/research/2026-11-25-${"a".repeat(48)}.md`);
  });

  it("keeps the question as the file's heading and quotes the findings for a task", () => {
    expect(reportFile("Why?\nmore", "Because.")).toBe("# Why?\n\nBecause.\n");
    expect(taskPrompt("Why?", "A\n\nB")).toBe('Research on "Why?" found:\n\n> A\n>\n> B\n\nTask: ');
  });
});
