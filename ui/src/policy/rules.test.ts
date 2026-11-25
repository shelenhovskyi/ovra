import { describe, expect, it } from "vitest";
import { byPrecedence, caveats, formRule, matchHint, ruleText } from "./rules";

describe("policy rules", () => {
  it("reads like the core writes them", () => {
    expect(ruleText({ tool: "Bash", match: "npm test", action: "auto" })).toBe("Bash(npm test)");
    expect(ruleText({ tool: "WebFetch", action: "deny" })).toBe("WebFetch");
  });

  it("builds a rule from a form, leaving an empty match out", () => {
    expect(formRule(" Bash ", " npm test ", "auto")).toEqual({ tool: "Bash", match: "npm test", action: "auto" });
    expect(formRule("WebFetch", "  ", "deny")).toEqual({ tool: "WebFetch", action: "deny" });
  });

  it("says what deny and auto rules do not promise", () => {
    expect(caveats({ tool: "Bash", match: "git push", action: "deny" })).toEqual([
      expect.stringMatching(/^Advisory: .*env git push.*sandbox is the boundary/),
    ]);
    expect(caveats({ tool: "Read", match: "secrets/**", action: "deny" })).toEqual([
      expect.stringMatching(/^Advisory: Grep, Glob or cat/),
    ]);
    expect(caveats({ tool: "WebFetch", action: "deny" })).toEqual([]);
    expect(caveats({ tool: "Edit", match: "docs/**", action: "auto" })).toEqual([
      "Applies only in a sandboxed session whose network is limited. Otherwise Ovra still asks.",
    ]);
    expect(caveats({ tool: "Bash", match: "npm test", action: "auto" })[1]).toMatch(
      /^npm test also runs .*package\.json scripts, and the agent can edit those\.$/,
    );
    expect(caveats({ tool: "Bash", action: "ask" })).toEqual([]);
  });

  it("lists rules deny first, then ask, then auto, keeping their order otherwise", () => {
    const rules = [
      { tool: "Bash", match: "npm test", action: "auto" as const },
      { tool: "WebFetch", action: "deny" as const },
      { tool: "Write", action: "ask" as const },
      { tool: "Bash", match: "git push", action: "deny" as const },
    ];
    expect(byPrecedence(rules).map((r) => r.index)).toEqual([1, 3, 2, 0]);
  });

  it("explains the match for the tool being typed", () => {
    expect(matchHint("Bash")).toMatch(/command prefix/);
    expect(matchHint("Edit")).toMatch(/glob/);
    expect(matchHint("WebFetch")).toMatch(/Only Bash and the file tools/);
  });
});
