import { expect, it } from "vitest";
import { addToSection, bullets, detectCommands, importsRules, withAnswers, withImport } from "./teach";

it("finds commands in a justfile, package.json, Makefile and Cargo.toml", () => {
  const justfile = [
    "set shell := ['bash', '-c']",
    "# fast checks",
    "check:",
    "    cargo fmt --check",
    "",
    "test *args: build",
    "_private:",
    "alias t := test",
  ].join("\n");
  const commands = detectCommands({
    justfile,
    "package.json": JSON.stringify({ scripts: { test: "vitest", build: "vite build", postinstall: "x" } }),
    "pnpm-lock.yaml": "",
    Makefile: ".PHONY: all\nall: build\nVAR := 1\nbuild:\n\tcc x.c\n",
    "Cargo.toml": "[package]",
  });
  expect(commands).toEqual([
    "`just check` - fast checks",
    "`just test`",
    "`pnpm run build`",
    "`pnpm run test`",
    "`make all`",
    "`make build`",
    "`cargo build` - build",
    "`cargo test` - run the tests",
  ]);
  expect(detectCommands({ "package.json": "{ not json" })).toEqual([]);
  expect(detectCommands({ "pyproject.toml": "[tool.pytest.ini_options]" })).toEqual(["`pytest` - run the tests"]);
});

it("turns answers into list items and a new AGENTS.md", () => {
  expect(bullets("- one\n\n  * two \n•  three")).toEqual(["one", "two", "three"]);
  expect(withAnswers(null, { commands: "`just test`", rules: "Write tests", never: "" })).toBe(
    [
      "# AGENTS.md",
      "",
      "Instructions for the coding agents that work in this project.",
      "",
      "## Commands",
      "",
      "- `just test`",
      "",
      "## Rules",
      "",
      "- Write tests",
      "",
    ].join("\n"),
  );
});

it("adds items at the end of an existing section, once", () => {
  const file = "# Rules\n\n## Rules\n\n- Be brief\n\n## Other\n\ntext\n";
  const added = addToSection(file, "rules", ["Write tests", "Be brief"]);
  expect(added).toBe("# Rules\n\n## Rules\n\n- Be brief\n- Write tests\n\n## Other\n\ntext\n");
  expect(addToSection(added, "Rules", ["Write tests"])).toBe(added);
  expect(addToSection("## Rules\n", "Rules", ["A"])).toBe("## Rules\n\n- A\n");
  expect(addToSection("# Title\n", "Do not touch", ["db/"])).toBe("# Title\n\n## Do not touch\n\n- db/\n");
});

it("brings AGENTS.md into CLAUDE.md once", () => {
  expect(withImport("# Claude\n")).toBe("@AGENTS.md\n\n# Claude\n");
  expect(importsRules("x\n @AGENTS.md\n")).toBe(true);
  expect(withImport("@AGENTS.md\n")).toBe("@AGENTS.md\n");
});
