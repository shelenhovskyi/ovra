import { expect, test } from "vitest";
import { tabLabels } from "./labels";

const labelsOf = (paths: string[]) => Object.fromEntries(tabLabels(paths));

test("unique names have no hint", () => {
  expect(labelsOf(["src/a.ts", "b.ts"])).toEqual({
    "src/a.ts": { name: "a.ts", hint: "" },
    "b.ts": { name: "b.ts", hint: "" },
  });
});

test("shared names get the shortest folder suffix that tells them apart", () => {
  expect(labelsOf(["packages/pkg-003/src/module-0.ts", "packages/pkg-004/src/module-0.ts", "lib/index.ts"])).toEqual({
    "packages/pkg-003/src/module-0.ts": { name: "module-0.ts", hint: ".../pkg-003/src" },
    "packages/pkg-004/src/module-0.ts": { name: "module-0.ts", hint: ".../pkg-004/src" },
    "lib/index.ts": { name: "index.ts", hint: "" },
  });
  expect(labelsOf(["ui/README.md", "docs/README.md", "README.md"])).toEqual({
    "ui/README.md": { name: "README.md", hint: "ui" },
    "docs/README.md": { name: "README.md", hint: "docs" },
    "README.md": { name: "README.md", hint: "./" },
  });
});
