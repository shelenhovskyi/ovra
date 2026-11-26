import { describe, expect, it } from "vitest";
import type { Transport } from "../lib/transport";
import { type GitHubList, githubKey, githubRows, offersGitHub } from "./github-context";

const transport = {} as Transport;
const list: GitHubList = {
  issues: [
    { number: 120, title: "Crash on start", url: "u120" },
    { number: 12, title: "Login fails", url: "u12" },
  ],
  pr: {
    number: 7,
    url: "u7",
    branch: "fix",
    failed: [
      { name: "test", state: "failed", url: null, run: 3 },
      { name: "external", state: "failed", url: null, run: null },
    ],
  },
};
const names = (query: string) => githubRows(transport, "p1", list, query, 8).map((r) => `${r.label} ${r.detail}`);

describe("GitHub rows", () => {
  it("list failed checks with a log first, then issues", () => {
    expect(names("")).toEqual(["test Failed check of pull request #7", "#120 Crash on start", "#12 Login fails"]);
  });

  it("find issues by number, and offer a number no listed issue has", () => {
    expect(names("12")).toEqual(["#120 Crash on start", "#12 Login fails"]);
    expect(names("4")).toEqual(["#4 Read issue #4"]);
    expect(names("login")).toEqual(["#12 Login fails"]);
  });

  it("key chips as their rows, so what the box holds is not offered again", () => {
    const rows = githubRows(transport, "p1", list, "", 8);
    expect(rows.map((r) => r.key)).toEqual(["check 7 test", "issue 120", "issue 12"]);
    expect(githubKey({ kind: "issue", number: 12, title: "", url: "", text: "" })).toBe("issue 12");
    expect(githubKey({ kind: "check", pr: 7, name: "test", text: "" })).toBe("check 7 test");
    expect(githubKey({ kind: "file", path: "a" })).toBeNull();
  });

  it("are offered in the plain @ menu with nothing typed or a word that names them", () => {
    expect(["", "gi", "iss", "ci", "check"].map(offersGitHub)).toEqual([true, true, true, true, true]);
    expect(["s", "lib", "src/"].map(offersGitHub)).toEqual([false, false, false]);
  });
});
