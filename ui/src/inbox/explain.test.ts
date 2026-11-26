import { describe, expect, it } from "vitest";
import { explainedFor, explainPrompt, readExplanation } from "./explain";
import { change, review } from "./fake-review";

describe("explainPrompt", () => {
  it("names the task, the summary and the files, and sends no secret's contents", () => {
    const item = review("r1", {
      summary: "Fixed the greeting.",
      files: [change("README.md"), change(".env"), change("src/key.ts")],
    });
    const prompt = explainPrompt(item, "Fix the greeting", [
      { path: "README.md", patch: "@@ -1 +1 @@\n-helo\n+hello" },
      { path: ".env", patch: "+PASSWORD=hunter2" },
      { path: "src/key.ts", patch: "+const k = 'ghp_abcdefghijklmnopqrstuvwxyz0123';" },
    ]);
    expect(prompt.split("\n")[0]).toBe("Explain these changes in 3 files");
    expect(prompt).toContain("Task: Fix the greeting");
    expect(prompt).toContain("The agent's own summary:\nFixed the greeting.");
    expect(prompt).toContain("- README.md (modified, +1 -1)");
    expect(prompt).toContain("--- README.md\n@@ -1 +1 @@\n-helo\n+hello");
    expect(prompt).toContain("Contents not sent because they may hold a secret: .env, src/key.ts");
    expect(prompt).not.toContain("hunter2");
    expect(prompt).not.toContain("ghp_");
  });

  it("stays under the message limit with big diffs, and says which were cut", () => {
    const files = Array.from({ length: 8 }, (_, i) => change(`f${i}.ts`));
    const big = Array.from({ length: 2000 }, (_, i) => `+line ${i} ${"x".repeat(20)}`).join("\n");
    const prompt = explainPrompt(
      review("r1", { files }),
      "Big",
      files.map((f) => ({ path: f.path, patch: big })),
    );
    expect(new TextEncoder().encode(prompt).length).toBeLessThanOrEqual(62 * 1024);
    expect(prompt).toMatch(/Cut or left out: f0\.ts, f1\.ts, f2\.ts, f3\.ts/);
  });
});

describe("readExplanation", () => {
  it("takes the overall lines and one line per known file", () => {
    const answer =
      'Here it is.\n```json\n{"overall": ["The greeting is **fixed**."], "files": [' +
      '{"path": "README.md", "explanation": "Corrects a typo."},' +
      '{"path": "nope.md", "explanation": "Not a changed file."},' +
      '{"path": "README.md", "explanation": "Twice."}]}\n```';
    expect(readExplanation(answer, ["README.md"])).toEqual({
      overall: ["The greeting is fixed."],
      files: { "README.md": "Corrects a typo." },
    });
  });

  it("shows an answer without a usable block as plain lines, and none for an empty one", () => {
    expect(readExplanation("- It fixes a typo.\n- Nothing else.", ["a"])).toEqual({
      overall: ["It fixes a typo.", "Nothing else."],
      files: {},
    });
    expect(readExplanation("```json\n{oops\n```", ["a"])).toBeNull();
    expect(readExplanation("", ["a"])).toBeNull();
  });

  it("tells changed files apart from the same files changed more", () => {
    const one = review("r1", { files: [change("a.ts")] });
    const more = review("r1", { files: [change("a.ts", { additions: 5 })] });
    expect(explainedFor(one)).not.toBe(explainedFor(more));
  });
});
