// Fixture projects for UX reviews: real git repositories in a temp folder.

import { execFileSync } from "node:child_process";
import { mkdirSync, mkdtempSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { dirname, join } from "node:path";

// A 1x1 PNG, so image viewers have something real to show.
const PNG = Buffer.from(
  "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mP8z8BQDwAEhQGAhKmMIQAAAABJRU5ErkJggg==",
  "base64",
);

function git(root: string, ...args: string[]): void {
  execFileSync("git", ["-c", "user.name=ux", "-c", "user.email=ux@example.invalid", ...args], {
    cwd: root,
    stdio: "ignore",
  });
}

function put(root: string, path: string, body: string | Buffer): void {
  const full = join(root, path);
  mkdirSync(dirname(full), { recursive: true });
  writeFileSync(full, body);
}

/** A small app: docs, code, an image, gitignored output and some git changes. */
function small(root: string): void {
  put(root, ".gitignore", "target/\nnode_modules/\n*.log\n");
  put(
    root,
    "README.md",
    "# Demo app\n\nA small project for UX reviews.\n\n- [ ] item\n- [x] done\n\n```rust\nfn main() {}\n```\n",
  );
  put(root, "docs/guide.md", "# Guide\n\nSee [README](../README.md).\n\n<script>alert(1)</script>\n");
  put(root, "src/main.rs", 'fn main() {\n    println!("hello");\n}\n');
  put(root, "src/lib.rs", "pub fn add(a: i32, b: i32) -> i32 {\n    a + b\n}\n");
  put(root, "src/util/strings.rs", "pub fn shout(s: &str) -> String {\n    s.to_uppercase()\n}\n");
  put(root, "ui/app.ts", "export const answer = 42;\n");
  put(root, "assets/logo.png", PNG);
  put(root, "Cargo.toml", '[package]\nname = "demo"\nversion = "0.1.0"\n');
  put(root, "a file with spaces.txt", "spaces\n");
  put(
    root,
    "changes.patch",
    "--- a/src/lib.rs\n+++ b/src/lib.rs\n@@ -1,3 +1,3 @@\n pub fn add(a: i32, b: i32) -> i32 {\n-    a + b\n+    a.wrapping_add(b)\n }\n",
  );
  put(root, "data/blob.bin", Buffer.from([0, 1, 2, 255, 0, 7]));
  put(root, "data/huge.txt", "x".repeat(6 * 1024 * 1024));
  put(root, "very-long-directory-name-that-will-not-fit-in-the-sidebar/and-a-long-file-name-too.config.json", "{}\n");
  git(root, "init", "-q", "-b", "main");
  git(root, "add", ".");
  git(root, "commit", "-q", "-m", "init");
  put(root, "src/main.rs", 'fn main() {\n    println!("hello, world");\n}\n'); // modified
  put(root, "src/new_module.rs", "// untracked\n");
  put(root, "target/debug/demo", "binary");
  put(root, "build.log", "log\n");
  put(root, "node_modules/left-pad/index.js", "module.exports = 1;\n");
}

/** A big monorepo: `packages` folders with `files` files each, plus a flat folder of `flat` files. */
function monorepo(root: string, packages = 200, files = 50, flat = 5000): void {
  put(root, ".gitignore", "dist/\n");
  put(root, "README.md", "# Monorepo\n");
  for (let p = 0; p < packages; p++) {
    const pkg = `packages/pkg-${String(p).padStart(3, "0")}`;
    put(root, `${pkg}/package.json`, `{"name":"pkg-${p}"}\n`);
    for (let f = 0; f < files; f++) put(root, `${pkg}/src/module-${f}.ts`, `export const v${f} = ${f};\n`);
    put(root, `${pkg}/dist/index.js`, "// built\n");
  }
  for (let f = 0; f < flat; f++) put(root, `generated/file-${String(f).padStart(5, "0")}.json`, "{}\n");
  git(root, "init", "-q", "-b", "main");
  git(root, "add", ".");
  git(root, "commit", "-q", "-m", "init");
  put(root, "packages/pkg-042/src/module-7.ts", "export const v7 = 'changed';\n");
}

/** An empty git repository. */
function empty(root: string): void {
  git(root, "init", "-q", "-b", "main");
}

const FIXTURES = { small, monorepo, empty } as const;
export type FixtureName = keyof typeof FIXTURES;

/** Creates the named fixtures under one temp folder; returns name -> absolute root. */
export function makeFixtures(names: FixtureName[]): Record<string, string> {
  const base = mkdtempSync(join(tmpdir(), "ovra-ux-"));
  const roots: Record<string, string> = {};
  for (const name of names) {
    const root = join(base, name === "small" ? "demo-app" : name);
    mkdirSync(root, { recursive: true });
    FIXTURES[name](root);
    roots[name] = root;
  }
  return roots;
}
