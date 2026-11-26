import { expect, it } from "vitest";
import type { FileDiff } from "../lib/protocol";
import {
  bytes,
  commitsPrompt,
  diffsSection,
  fitPrompt,
  looksSecret,
  MAX_PROMPT,
  plainLines,
  pulledPrompt,
  readMessage,
  readPlan,
} from "./assist";
import { changed } from "./fake-git";

const diff = (path: string, lines: string[]): FileDiff => ({
  path,
  from: null,
  binary: false,
  truncated: false,
  hunks: [{ old_start: 1, old_lines: 1, new_start: 1, new_lines: lines.length, header: "", lines }],
});

it("reads a plan of known files only, each once, and names what is left", () => {
  const answer = 'Two commits.\n```json\n{"commits": [{"message": "feat: a\\n\\nWhy.", "files": ["a.ts"]}]}\n```';
  expect(readPlan(answer, ["a.ts", "b.ts"])).toEqual({
    ok: true,
    commits: [{ message: "feat: a\n\nWhy.", files: ["a.ts"] }],
    left: ["b.ts"],
  });
  const bad = (commits: unknown) => readPlan(`\`\`\`json\n${JSON.stringify({ commits })}\n\`\`\``, ["a.ts"]);
  expect(bad([{ message: "x", files: ["../etc/passwd"] }])).toMatchObject({ ok: false });
  expect(
    bad([
      { message: "x", files: ["a.ts"] },
      { message: "y", files: ["a.ts"] },
    ]),
  ).toMatchObject({ ok: false });
  expect(bad([{ message: " ", files: ["a.ts"] }])).toMatchObject({ ok: false });
  expect(bad([])).toMatchObject({ ok: false });
  expect(readPlan("no block here", ["a.ts"])).toMatchObject({ ok: false, why: "it has no list of commits." });
  expect(readPlan("```json\n{nope\n```", ["a.ts"])).toMatchObject({ ok: false });
});

it("takes the message from its fence, else the whole answer", () => {
  expect(readMessage("Here:\n```text\nfix: typo\n```")).toBe("fix: typo");
  expect(readMessage("  fix: typo \n")).toBe("fix: typo");
  expect(readMessage("   ")).toBeNull();
});

it("keeps secrets' contents out of the prompt and says which diffs were cut", () => {
  expect(looksSecret(".env.local")).toBe(true);
  expect(looksSecret("config/deploy.pem")).toBe(true);
  expect(looksSecret("src/environment.ts")).toBe(false);
  const text = diffsSection(
    [diff(".env", ["+TOKEN=abc"]), diff("a.ts", ["+one", "+two"]), diff("big.ts", ["+x".repeat(400)])],
    300,
  );
  expect(text).not.toContain("TOKEN");
  expect(text).toContain("+one");
  expect(text).toContain("may hold a secret: .env");
  expect(text).toContain("read these files yourself if you need more): big.ts");
});

it("titles the prompt on its first line and says the agent only proposes", () => {
  const prompt = commitsPrompt([changed("a.ts")], [diff("a.ts", ["+x"])], []);
  expect(prompt.split("\n")[0]).toBe("Suggest commits for 1 changed file");
  expect(prompt).toContain("Never push");
  expect(prompt).toContain("- a.ts (modified, not staged)");
  const pulled = pulledPrompt({
    from: "a",
    truncated: 2,
    commits: [
      {
        commit: { id: "c1", author: "Ada", time: 0, summary: "Add login" },
        body: "Uses OAuth.",
        files: [{ path: "src/login.ts", added: 10, removed: 2 }],
        more_files: 0,
      },
    ],
  });
  expect(pulled.split("\n")[0]).toBe("Summarize what's new in 3 commits");
  expect(pulled).toContain("src/login.ts (+10 -2)");
});

it("shows an answer as plain lines", () => {
  expect(plainLines("## News\n- **Login** works\n* uses `oauth`\n\n```\ncode\n```")).toEqual([
    "News",
    "Login works",
    "uses oauth",
    "code",
  ]);
});

it("keeps every prompt under the core's 64 KiB, counted in UTF-8", () => {
  const many = Array.from({ length: 2000 }, (_, i) => changed(`src/file-${i}.ts`));
  const wide = diff(
    "wide.ts",
    Array.from({ length: 3000 }, () => `+${"界".repeat(40)}`),
  );
  const prompt = commitsPrompt(many, [wide], []);
  expect(bytes(prompt)).toBeLessThan(MAX_PROMPT);
  expect(prompt).toContain("- and 1700 more files");
  const commit = (i: number) => ({
    commit: { id: `c${i}`, author: "Ada", time: 0, summary: `Commit ${i}` },
    body: "é".repeat(2000),
    files: Array.from({ length: 50 }, (_, f) => ({ path: `pkg/${f}.ts`, added: 1, removed: 0 })),
    more_files: 0,
  });
  const pulled = pulledPrompt({ from: "a", truncated: 0, commits: Array.from({ length: 100 }, (_, i) => commit(i)) });
  expect(bytes(pulled)).toBeLessThan(MAX_PROMPT);
  expect(pulled).toMatch(/\(\d+ older commits not listed\)/);
  // The last resort cuts on a character, never in the middle of one.
  const cut = fitPrompt("😀".repeat(30_000), 1000);
  expect(bytes(cut)).toBeLessThanOrEqual(1000);
  expect(cut).toMatch(/^(?:😀)+\n\n\(The rest was cut to fit\.\)$/u);
});

it("leaves out diffs that hold a token, and renames of secrets", () => {
  const text = diffsSection([
    diff("src/config.ts", ["+const key = 'ghp_abcdefghijklmnopqrstuvwxyz0123'"]),
    { ...diff("config/app.txt", ["+x"]), from: ".env.production" },
  ]);
  expect(text).not.toContain("ghp_");
  expect(text).toContain("may hold a secret: src/config.ts, config/app.txt");
});
