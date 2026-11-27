import { describe, expect, it } from "vitest";
import type { ReviewItem } from "../lib/protocol";
import { review } from "./fake-review";
import { acrossRepos, repoLine, repoTag, sameRepo } from "./repos";

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
      "The changes in api. This task also changed web, docs: each repository has its own item here, and Keep moves on to the next.",
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
});
