// Parses unified diffs (`git diff`, `diff -u`) into files, hunks and numbered lines.

type PatchLine =
  | { kind: "context"; text: string; old: number; new: number }
  | { kind: "added"; text: string; new: number }
  | { kind: "removed"; text: string; old: number }
  | { kind: "note"; text: string };

type Hunk = { header: string; lines: PatchLine[] };

export type PatchFile = { oldPath: string | null; newPath: string | null; hunks: Hunk[] };

const HUNK = /^@@ -(\d+)(?:,(\d+))? \+(\d+)(?:,(\d+))? @@/;

function headerPath(line: string): string | null {
  const raw = line.slice(4).split("\t")[0]?.trim() ?? "";
  if (raw === "/dev/null") return null;
  return raw.replace(/^[ab]\//, "");
}

export function parsePatch(text: string): PatchFile[] {
  const files: PatchFile[] = [];
  let file: PatchFile | null = null;
  let hunk: Hunk | null = null;
  // Lines still expected in the current hunk, from its header; this tells a
  // removed "-- x" line apart from the next file's "--- " header.
  let oldLeft = 0;
  let newLeft = 0;
  let oldNo = 0;
  let newNo = 0;

  for (const line of text.split(/\r?\n/)) {
    if (hunk && (oldLeft > 0 || newLeft > 0)) {
      const body = line.slice(1);
      if (line.startsWith("+")) {
        hunk.lines.push({ kind: "added", text: body, new: newNo++ });
        newLeft--;
        continue;
      }
      if (line.startsWith("-")) {
        hunk.lines.push({ kind: "removed", text: body, old: oldNo++ });
        oldLeft--;
        continue;
      }
      if (line.startsWith(" ") || line === "") {
        hunk.lines.push({ kind: "context", text: body, old: oldNo++, new: newNo++ });
        oldLeft--;
        newLeft--;
        continue;
      }
    }
    if (hunk && line.startsWith("\\")) {
      hunk.lines.push({ kind: "note", text: line });
      continue;
    }
    if (line.startsWith("--- ")) {
      file = { oldPath: headerPath(line), newPath: null, hunks: [] };
      files.push(file);
      hunk = null;
      continue;
    }
    if (line.startsWith("+++ ") && file && !hunk) {
      file.newPath = headerPath(line);
      continue;
    }
    const start = HUNK.exec(line);
    if (start && file) {
      oldNo = Number(start[1]);
      oldLeft = start[2] === undefined ? 1 : Number(start[2]);
      newNo = Number(start[3]);
      newLeft = start[4] === undefined ? 1 : Number(start[4]);
      hunk = { header: line, lines: [] };
      file.hunks.push(hunk);
    }
  }
  return files;
}
