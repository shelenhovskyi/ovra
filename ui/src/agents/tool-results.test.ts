import { describe, expect, it } from "vitest";
import { ideResult } from "./tool-results";

describe("ideResult", () => {
  it("tells Ovra's tool answers in words and leaves the rest alone", () => {
    expect(ideResult("mcp__ovra__list_agents", '{"agents":[{},{}]}')).toBe("Listed 2 workers");
    expect(ideResult("mcp__ovra__request_review", '{"review_id":"r1","files":["a.md"]}')).toBe(
      "Review asked for, 1 changed file",
    );
    expect(ideResult("mcp__ovra__send_message", "Sent to s2.")).toBe("Message sent");
    // An answer it does not know, or another tool's, keeps the raw fold.
    expect(ideResult("mcp__ovra__spawn_agent", "not json")).toBeNull();
    expect(ideResult("Bash", '{"agents":[]}')).toBeNull();
  });
});
