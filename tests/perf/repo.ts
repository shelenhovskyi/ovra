// Generates the large repository the benchmarks run against. Deterministic,
// and cached: a second run with the same layout reuses the folder.
import { execFileSync } from "node:child_process";
import { existsSync, mkdirSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { join } from "node:path";

/** Layout of the generated repository. Change `version` when it changes. */
const LAYOUT = {
  version: 1,
  // pkg00..pkg09 / mod00..mod09 / part00..part09 / f00..f89.rs: 90,000 files.
  top: 10,
  mid: 10,
  leaf: 10,
  filesPerLeaf: 90,
  // One flat directory with 5,000 files, the worst case for the tree.
  flatFiles: 5_000,
  // docs/d00..d49 / page000..page099.md: 5,000 files.
  docDirs: 50,
  filesPerDocDir: 100,
  // target/ is ignored: 10 dirs x 1,000 files the watcher must not pay for.
  ignoredDirs: 10,
  filesPerIgnoredDir: 1_000,
  // After the commit: modified tracked files and new untracked files.
  modified: 100,
  untracked: 100,
} as const;

const pad = (n: number, width: number): string => String(n).padStart(width, "0");

/** Path of a generated source file, project-relative. */
export function sourceFile(top: number, mid: number, leaf: number, file: number): string {
  return `pkg${pad(top, 2)}/mod${pad(mid, 2)}/part${pad(leaf, 2)}/f${pad(file, 2)}.rs`;
}

function write(root: string, rel: string, text: string): void {
  writeFileSync(join(root, rel), text);
}

function git(root: string, args: string[]): void {
  execFileSync(
    "git",
    ["-c", "user.name=ovra-perf", "-c", "user.email=perf@ovra.invalid", "-c", "commit.gpgsign=false", ...args],
    { cwd: root, stdio: "ignore" },
  );
}

function generate(root: string): void {
  rmSync(root, { recursive: true, force: true });
  mkdirSync(root, { recursive: true });
  git(root, ["init", "-q", "-b", "main"]);
  write(root, ".gitignore", "target/\n*.log\n");
  write(root, "README.md", "# Generated repository for Ovra benchmarks\n");

  for (let t = 0; t < LAYOUT.top; t++)
    for (let m = 0; m < LAYOUT.mid; m++)
      for (let l = 0; l < LAYOUT.leaf; l++) {
        mkdirSync(join(root, `pkg${pad(t, 2)}/mod${pad(m, 2)}/part${pad(l, 2)}`), { recursive: true });
        for (let f = 0; f < LAYOUT.filesPerLeaf; f++) {
          write(root, sourceFile(t, m, l, f), `pub fn f_${t}_${m}_${l}_${f}() -> u32 {\n    ${f}\n}\n`);
        }
      }

  mkdirSync(join(root, "flat"));
  for (let n = 0; n < LAYOUT.flatFiles; n++) write(root, `flat/item${pad(n, 4)}.txt`, `item ${n}\n`);

  for (let d = 0; d < LAYOUT.docDirs; d++) {
    mkdirSync(join(root, `docs/d${pad(d, 2)}`), { recursive: true });
    for (let p = 0; p < LAYOUT.filesPerDocDir; p++)
      write(root, `docs/d${pad(d, 2)}/page${pad(p, 3)}.md`, `# Page ${d}.${p}\n`);
  }

  git(root, ["add", "-A"]);
  git(root, ["commit", "-q", "-m", "generated"]);

  for (let d = 0; d < LAYOUT.ignoredDirs; d++) {
    mkdirSync(join(root, `target/build${pad(d, 2)}`), { recursive: true });
    for (let n = 0; n < LAYOUT.filesPerIgnoredDir; n++) write(root, `target/build${pad(d, 2)}/obj${pad(n, 4)}.o`, "\0");
  }
  for (let n = 0; n < LAYOUT.modified; n++) {
    write(root, sourceFile(n % LAYOUT.top, 0, 0, Math.floor(n / LAYOUT.top)), `// modified ${n}\n`);
  }
  mkdirSync(join(root, "docs/drafts"));
  for (let n = 0; n < LAYOUT.untracked; n++) write(root, `docs/drafts/draft${pad(n, 3)}.md`, `draft ${n}\n`);
}

/** Creates the repository at `root` unless an identical one is already there. */
export function ensureRepo(root: string): void {
  const marker = `${root}.layout.json`;
  const wanted = JSON.stringify(LAYOUT);
  if (existsSync(marker) && existsSync(root) && readFileSync(marker, "utf8") === wanted) return;
  rmSync(marker, { force: true });
  generate(root);
  writeFileSync(marker, wanted);
}
