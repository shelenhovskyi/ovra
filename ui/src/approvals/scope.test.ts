import { describe, expect, it } from "vitest";
import { pathScopes, sameFolder } from "./scope";

const scope = (path: string, root: string | null = "/work/demo") => pathScopes({ file_path: path }, root)[0]?.scope;

describe("pathScopes", () => {
  it("tells paths inside the folder from paths outside it", () => {
    expect(scope("/work/demo/README.md")).toBe("inside");
    expect(scope("src/main.rs")).toBe("inside");
    expect(scope("/etc/passwd")).toBe("outside");
    expect(scope("/work/demo-other/a")).toBe("outside");
  });

  it("never calls a path with dots inside", () => {
    expect(scope("../demo-other/a")).toBe("outside");
    expect(scope("/work/demo/../../etc/passwd")).toBe("outside");
    expect(scope("./src/../README.md")).toBe("unknown");
    expect(scope("/work/demo/link/../a")).toBe("unknown");
  });

  it("cannot tell where shell expansions point", () => {
    for (const path of ["~/notes.txt", "~", "$HOME/a", "%USERPROFILE%\\a"]) expect(scope(path)).toBe("unknown");
  });

  it("compares Windows paths without case", () => {
    expect(scope("c:\\Work\\Demo\\a.txt", "C:\\work\\demo")).toBe("inside");
    expect(scope("D:\\work\\demo\\a.txt", "C:\\work\\demo")).toBe("outside");
  });

  it("cannot tell drive-relative and root-relative Windows paths", () => {
    expect(scope("C:a.txt", "C:\\work\\demo")).toBe("unknown");
    expect(scope("\\work\\demo\\a.txt", "C:\\work\\demo")).toBe("unknown");
    expect(scope("/work/demo/a.txt", "\\\\server\\share\\demo")).toBe("unknown");
  });

  it("keeps network shares apart from local folders", () => {
    expect(scope("\\\\server\\share\\demo\\a", "\\\\server\\share\\demo")).toBe("inside");
    expect(scope("\\\\other\\share\\demo\\a", "\\\\server\\share\\demo")).toBe("outside");
    expect(scope("//work/demo/a")).toBe("outside");
    expect(scope("/server/share/demo/a", "\\\\server\\share\\demo")).toBe("unknown");
  });

  it("cannot tell anything without a folder", () => {
    expect(scope("/work/demo/a", null)).toBe("unknown");
    expect(scope("/work/demo/a", "relative/folder")).toBe("unknown");
  });

  it("gives inside paths relative to the folder", () => {
    expect(pathScopes({ file_path: "/work/demo/src/a.rs" }, "/work/demo")).toEqual([
      { path: "/work/demo/src/a.rs", scope: "inside", relative: "src/a.rs" },
    ]);
  });

  it("lists every path key and ignores input without paths", () => {
    expect(pathScopes({ path: "/tmp", pattern: "*" }, "/work/demo")).toEqual([{ path: "/tmp", scope: "outside" }]);
    expect(pathScopes({ command: "ls" }, "/work/demo")).toEqual([]);
  });
});

describe("sameFolder", () => {
  it("compares folders by text", () => {
    expect(sameFolder("/work/demo/", "/work/demo")).toBe(true);
    expect(sameFolder("C:\\Work", "c:/work")).toBe(true);
    expect(sameFolder("/work/demo", "/work/demo/.ovra/worktrees/t/w")).toBe(false);
  });
});
