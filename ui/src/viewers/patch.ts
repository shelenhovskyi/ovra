// Parses unified diffs (`git diff`, `diff -u`) into files, hunks and numbered lines.

export type PatchLine =
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

/** Display name of a patched file: its new path, or the old one when deleted. */
export function fileName(file: PatchFile): string {
  return file.newPath ?? file.oldPath ?? "";
}

export type FileStats = { added: number; removed: number };

export function fileStats(file: PatchFile): FileStats {
  let added = 0;
  let removed = 0;
  for (const hunk of file.hunks)
    for (const line of hunk.lines) {
      if (line.kind === "added") added++;
      else if (line.kind === "removed") removed++;
    }
  return { added, removed };
}

/** One row of the virtualized patch view. */
export type PatchRow = { kind: "file"; file: number } | { kind: "hunk"; text: string } | PatchLine;

/** Flattens files into rows; collapsed files keep only their header row. */
export function patchRows(files: PatchFile[], collapsed: ReadonlySet<number>): PatchRow[] {
  const rows: PatchRow[] = [];
  files.forEach((file, index) => {
    rows.push({ kind: "file", file: index });
    if (collapsed.has(index)) return;
    for (const hunk of file.hunks) {
      rows.push({ kind: "hunk", text: hunk.header });
      for (const line of hunk.lines) rows.push(line);
    }
  });
  return rows;
}

/** Indices of rows that start a run of added or removed lines. */
export function changeStarts(rows: readonly PatchRow[]): number[] {
  const starts: number[] = [];
  let inChange = false;
  rows.forEach((row, index) => {
    const changed = row.kind === "added" || row.kind === "removed";
    if (changed && !inChange) starts.push(index);
    inChange = changed;
  });
  return starts;
}
