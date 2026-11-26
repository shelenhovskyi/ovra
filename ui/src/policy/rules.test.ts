import { describe, expect, it } from "vitest";
import {
  byPrecedence,
  caveats,
  formProblem,
  formRule,
  matchHint,
  ruleLabel,
  ruleText,
  toolNote,
  toolWarning,
} from "./rules";

describe("policy rules", () => {
  it("reads like the core writes them", () => {
    expect(ruleText({ tool: "Bash", match: "npm test", action: "auto" })).toBe("Bash(npm test)");
    expect(ruleText({ tool: "WebFetch", action: "deny" })).toBe("WebFetch");
  });

  it("builds a rule from a form, leaving an empty match out", () => {
    expect(formRule(" Bash ", " npm test ", "auto")).toEqual({ tool: "Bash", match: "npm test", action: "auto" });
    expect(formRule("WebFetch", "  ", "deny")).toEqual({ tool: "WebFetch", action: "deny" });
  });

  it("marks only a Bash auto match exact, and names exact rules apart", () => {
    const exact = formRule("bash", "npm test", "auto", true);
    expect(exact).toEqual({ tool: "Bash", match: "npm test", action: "auto", exact: true });
    expect(formRule("Bash", "", "auto", true)).toEqual({ tool: "Bash", action: "auto" });
    expect(formRule("Edit", "a.md", "auto", true)).toEqual({ tool: "Edit", match: "a.md", action: "auto" });
    // An exact deny or ask would let longer commands past it.
    expect(formRule("Bash", "npm test", "deny", true)).toEqual({ tool: "Bash", match: "npm test", action: "deny" });
    expect(formRule("Bash", "npm test", "ask", true)).toEqual({ tool: "Bash", match: "npm test", action: "ask" });
    expect(ruleLabel(exact)).toBe("Bash(npm test) exactly auto");
    expect(ruleLabel(formRule("Bash", "npm test", "auto"))).toBe("Bash(npm test) auto");
  });

  it("says what deny and auto rules do not promise", () => {
    expect(caveats({ tool: "Bash", match: "git push", action: "deny" })).toEqual([
      expect.stringMatching(
        /^Advisory: .*env git push, gets past it\. The boundary that holds is the sandbox and its network proxy; without a sandbox, nothing does\.$/,
      ),
    ]);
    expect(caveats({ tool: "Bash", action: "deny" })).toEqual([]);
    expect(caveats({ tool: "Read", match: "secrets/**", action: "deny" })).toEqual([
      expect.stringMatching(/^Advisory: Grep, Glob or cat.*without a sandbox, nothing does\.$/),
    ]);
    expect(caveats({ tool: "WebFetch", action: "deny" })).toEqual([
      "Only this tool is refused; others such as Bash can do the same.",
    ]);
    expect(caveats({ tool: "Edit", match: "docs/**", action: "auto" })).toEqual([
      "Applies only to an agent in the sandbox with limited network access. Otherwise the call is decided as if this rule were not there.",
    ]);
    expect(caveats({ tool: "Bash", match: "npm test", action: "auto" })[1]).toMatch(
      /^npm test also runs .*package\.json scripts, and the agent can edit those\.$/,
    );
    expect(caveats({ tool: "Bash", action: "ask" })).toEqual([]);
    expect(caveats({ tool: "WebFetch", action: "ask" })).toEqual([]);
  });

  it("says what ask rules do not promise", () => {
    expect(caveats({ tool: "Bash", match: "git push", action: "ask" })).toEqual([
      expect.stringMatching(/^Advisory: .*env git push, is not asked about\. The boundary that holds is the sandbox/),
    ]);
    expect(caveats({ tool: "Write", match: "docs/**", action: "ask" })).toEqual([
      expect.stringMatching(/^Advisory: Bash commands like cat still reach it\. .*without a sandbox, nothing does\.$/),
    ]);
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
    expect(matchHint("Bash")).toMatch(/command prefix.*An Auto rule with Exact command matches only npm test itself/);
    expect(matchHint("Edit")).toMatch(/glob/);
    expect(matchHint("webfetch")).toMatch(
      /^One host.*Deny and Ask also cover its subdomains.*Auto covers that host only\. With a deny rule present, URLs Ovra cannot read, including private addresses.*, are denied too\.$/,
    );
    expect(matchHint("Grep")).toBe("Grep takes no match: the rule covers every call.");
    expect(matchHint("")).toMatch(
      /^Only Bash, WebFetch and the file tools \(Read, Edit, MultiEdit, Write, NotebookEdit\)/,
    );
  });

  it("refuses in the form what the core would refuse, naming the field to fix", () => {
    const problem = (tool: string, match: string) => formProblem(formRule(tool, match, "deny"));
    expect(problem("", "")).toEqual({ field: "tool", text: "Choose a tool first." });
    expect(problem("mcp__ovra__spawn_agent", "")?.field).toBe("tool");
    expect(problem("WebSearch", "github.com")).toMatchObject({
      field: "match",
      text: expect.stringMatching(/^WebSearch takes no match/),
    });
    expect(problem("WebFetch", "docs.rs")).toBeNull();
    for (const host of ["https://docs.rs", "*.docs.rs", "Docs.rs", "docs.rs/x"])
      expect(problem("WebFetch", host)?.field).toBe("match");
    expect(problem("mcp__github__create_issue", "x")?.field).toBe("match");
    for (const path of ["/etc/**", "../up", "a//b", "./a", "c:/x"]) expect(problem("Read", path)?.field).toBe("match");
    expect(problem("bash", "rm -rf")).toBeNull();
    expect(problem("Edit", "docs/**")).toBeNull();
    expect(problem("WebFetch", "")).toBeNull();
  });

  it("takes a remote and a branch for Push rules and says what they never do", () => {
    const push = (match: string) => formProblem({ tool: "Push", match, action: "auto" });
    for (const good of ["origin ovra/*", "origin fix-login", "up release/1.2"]) expect(push(good)).toBeNull();
    for (const bad of [
      "origin",
      "origin *",
      "https://x@github.com/o/r main",
      "origin a..b",
      "origin  main",
      "--x main",
    ])
      expect(push(bad)?.field, bad).toBe("match");
    expect(matchHint("push")).toContain("origin ovra/*");
    const auto = caveats({ tool: "Push", match: "origin ovra/*", action: "auto" });
    expect(auto.some((n) => n.includes("each worker's own branch only"))).toBe(true);
    expect(caveats({ tool: "Push", action: "deny" })).toEqual([]);
    expect(toolWarning("Push")).toBeNull();
  });

  it("fixes a known tool's case for her and says so", () => {
    expect(formRule("bash", "", "ask").tool).toBe("Bash");
    expect(toolNote("bash")).toBe("Saved as Bash: tool names are case-sensitive.");
    expect(toolNote("Shell")).toMatch(/does not know a tool named Shell/);
    expect(toolNote("Bash")).toBeNull();
  });

  it("warns about tool names that may never match, more strongly for a wrong case", () => {
    expect(toolWarning("Bash")).toBeNull();
    expect(toolWarning("mcp__github__create_issue")).toBeNull();
    expect(toolWarning("")).toBeNull();
    expect(toolWarning("bash")).toBe("Tool names match exactly: bash never matches Bash. Write Bash.");
    expect(toolWarning("Shell")).toMatch(/does not know a tool named Shell/);
    expect(caveats({ tool: "webfetch", action: "deny" })[0]).toMatch(/never matches WebFetch/);
  });
});
