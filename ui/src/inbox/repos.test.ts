import { describe, expect, it } from "vitest";
import type { ReviewItem } from "../lib/protocol";
import { review } from "./fake-review";
import { acrossRepos, keepAllWaits, repoLine, repoTag, sameRepo, taskParts } from "./repos";

const item = (id: string, project: string, task_repos?: string[]): ReviewItem =>
  review(id, { project, ...(task_repos ? { task_repos } : {}) });
const names: Record<string, string> = { p1: "web", p2: "api", p3: "docs" };
const name = (id: string) => names[id] ?? id;

describe("items of a task across repositories", () => {
  it("name their repository and the others", () => {
    const api = item("r2", "p2", ["p1", "p2", "p3"]);
    expect(acrossRepos(api)).toBe(true);
    expect(repoTag(api, name)).toBe("api");
    expect(repoLine(api, name)).toBe(
      "The changes in api. This task also changed web, docs: each repository has its own item here. Keep moves on to the next, and Keep all keeps every one.",
    );
  });

  it("say nothing for one repository", () => {
    const one = item("r1", "p1");
    expect(acrossRepos(one)).toBe(false);
    expect(repoTag(one, name)).toBeNull();
    expect(repoLine(one, name)).toBeNull();
  });

  it("compare files only with items of the same repository", () => {
    const items = [item("r1", "p1", ["p1", "p2"]), item("r2", "p2", ["p1", "p2"]), item("r3", "p1")];
    expect(sameRepo(items[0] as ReviewItem, items).map((i) => i.id)).toEqual(["r1", "r3"]);
  });

  it("keep all of a task's open parts, main first, unless a check or protected file needs its own Keep", () => {
    const repos = ["p1", "p2", "p3"];
    const api = review("r2", { project: "p2", session: "s9", task_repos: repos });
    const web = review("r1", { project: "p1", session: "s9", task_repos: repos });
    const docs = review("r3", { project: "p3", session: "s9", task_repos: repos, state: "approved" });
    const other = review("r4", { project: "p1", session: "s2" });
    const parts = taskParts(api, [api, other, docs, web]);
    expect(parts.map((i) => i.id)).toEqual(["r1", "r2"]);
    expect(taskParts(other, [other])).toEqual([]);
    expect(keepAllWaits(parts, name)).toBeNull();
    const checking = { ...api, check: { state: "running" as const, command: "t" } };
    expect(keepAllWaits([web, checking], name)).toBe("Keep all waits for the check in api.");
    const failed = { ...api, check: { state: "failed" as const, command: "t" } };
    expect(keepAllWaits([web, failed], name)).toBe("The check failed in api: keep each repository on its own.");
    const guarded = { ...web, files: [{ ...(web.files[0] as ReviewItem["files"][number]), protected: true }] };
    expect(keepAllWaits([guarded, api], name)).toBe("web has protected files: keep each repository on its own.");
  });
});
