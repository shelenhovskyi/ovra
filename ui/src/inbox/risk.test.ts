import { describe, expect, it } from "vitest";
import { change } from "./fake-review";
import { byRisk, riskReason } from "./risk";

describe("byRisk", () => {
  it("lists protected, deleted, build and sensitive files first, tests and docs last", () => {
    const files = [
      change("README.md"),
      change("src/parser.test.ts"),
      change("src/parser.ts"),
      change("src/auth/session.ts"),
      change("Cargo.toml"),
      change("src/old.ts", { kind: "deleted" }),
      change(".claude/settings.json", { protected: true }),
      change("src/big.ts", { additions: 250 }),
    ];
    expect(byRisk(files).map((f) => f.path)).toEqual([
      ".claude/settings.json",
      "src/auth/session.ts",
      "Cargo.toml",
      "src/old.ts",
      "src/big.ts",
      "src/parser.ts",
      "README.md",
      "src/parser.test.ts",
    ]);
  });

  it("gives a reason only where the marker adds one", () => {
    expect(riskReason(change("src/old.ts", { kind: "deleted" }))).toBe("Deletes a file");
    expect(riskReason(change(".github/workflows/ci.yml"))).toBe("Dependencies, build or CI");
    expect(riskReason(change(".env.example"))).toBe("May touch access or secrets");
    expect(riskReason(change(".claude/settings.json", { protected: true }))).toBeNull();
    expect(riskReason(change("src/tokenizer.ts"))).toBeNull();
  });
});
