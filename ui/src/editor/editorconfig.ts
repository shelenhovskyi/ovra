// `.editorconfig` files (https://editorconfig.org): the project's own
// indentation and save rules win over what the editor detects or the
// preferences say. Read are the files from the edited file's folder up to
// the project root, stopping at one that says `root = true`. Supported:
// indent_style, indent_size, tab_width, trim_trailing_whitespace and
// insert_final_newline (only `true`); other keys are ignored.

import type { ChangeSpec, Text } from "@codemirror/state";
import type { ProjectId } from "../lib/protocol";
import { CoreError, type Transport } from "../lib/transport";

/** What a file's `.editorconfig` sections say, after the later ones won. */
export type EditorConfig = {
  indentStyle?: "tab" | "space";
  /** Columns per indent level, or "tab" for tab_width. */
  indentSize?: number | "tab";
  tabWidth?: number;
  trimTrailingWhitespace?: boolean;
  insertFinalNewline?: boolean;
};

type Section = { glob: string; props: Record<string, string> };
type Parsed = { root: boolean; sections: Section[] };

/** Reads the INI text of one `.editorconfig`: lowercase keys, values as written. */
export function parse(text: string): Parsed {
  const parsed: Parsed = { root: false, sections: [] };
  let section: Section | null = null;
  for (const raw of text.split(/\r?\n/)) {
    const line = raw.trim();
    if (line === "" || line.startsWith("#") || line.startsWith(";")) continue;
    const header = /^\[(.*)\]$/.exec(line);
    if (header) {
      section = { glob: header[1] ?? "", props: {} };
      parsed.sections.push(section);
      continue;
    }
    const eq = line.indexOf("=");
    if (eq < 0) continue;
    const key = line.slice(0, eq).trim().toLowerCase();
    const value = line.slice(eq + 1).trim();
    if (section) section.props[key] = value;
    else if (key === "root") parsed.root = value.toLowerCase() === "true";
  }
  return parsed;
}

/** One piece of a glob, matched by `matchTokens` without backtracking. */
type Token =
  | { kind: "char"; test: (c: string) => boolean }
  /** `*`: any run without a slash. */
  | { kind: "star" }
  /** `**`: any run. */
  | { kind: "globstar" }
  /** `**` then a slash: no folder, or any folders each ending in a slash. */
  | { kind: "dirs" }
  /** `{1..3}`: a whole number in the range. */
  | { kind: "number"; lo: number; hi: number };

/** At most this many brace alternatives per glob; a header with more matches nothing. */
const MAX_ALTERNATIVES = 256;

/** Every glob `{a,b}` stands for, braces with a number range left in place; null when there are too many. */
function expand(glob: string): string[] | null {
  for (let i = 0; i < glob.length; i++) {
    const c = glob.charAt(i);
    if (c === "\\") {
      i++;
      continue;
    }
    if (c !== "{") continue;
    const end = closingBrace(glob, i);
    if (end < 0) return [glob];
    const body = glob.slice(i + 1, end);
    const options = splitTop(body);
    if (options.length < 2) continue;
    const out: string[] = [];
    for (const option of options) {
      const rest = expand(glob.slice(0, i) + option + glob.slice(end + 1));
      if (!rest || out.length + rest.length > MAX_ALTERNATIVES) return null;
      out.push(...rest);
    }
    return out;
  }
  return [glob];
}

/** The tokens of a glob without alternatives. */
function tokenize(glob: string): Token[] {
  const tokens: Token[] = [];
  const char = (test: (c: string) => boolean) => tokens.push({ kind: "char", test });
  for (let i = 0; i < glob.length; i++) {
    const c = glob.charAt(i);
    if (c === "\\" && i + 1 < glob.length) {
      const next = glob.charAt(++i);
      char((x) => x === next);
    } else if (c === "*") {
      let stars = 1;
      while (glob.charAt(i + 1) === "*") {
        stars++;
        i++;
      }
      if (stars === 1) tokens.push({ kind: "star" });
      else if (glob.charAt(i + 1) === "/") {
        tokens.push({ kind: "dirs" });
        i++;
      } else tokens.push({ kind: "globstar" });
    } else if (c === "?") {
      char((x) => x !== "/");
    } else if (c === "[" && glob.indexOf("]", i + 2) > 0) {
      const end = glob.indexOf("]", i + 2);
      const body = glob.slice(i + 1, end);
      const negated = body.startsWith("!");
      const set = classTest(negated ? body.slice(1) : body);
      char(negated ? (x) => x !== "/" && !set(x) : set);
      i = end;
    } else if (c === "{") {
      const end = closingBrace(glob, i);
      const range = end > 0 ? /^(-?\d+)\.\.(-?\d+)$/.exec(glob.slice(i + 1, end)) : null;
      if (range) {
        const [lo, hi] = [Number(range[1]), Number(range[2])];
        tokens.push({ kind: "number", lo: Math.min(lo, hi), hi: Math.max(lo, hi) });
        i = end;
      } else char((x) => x === c);
    } else {
      char((x) => x === c);
    }
  }
  return tokens;
}

/** A test for one character of a `[...]` body: single characters and `a-z` ranges. */
function classTest(body: string): (c: string) => boolean {
  const ranges: [string, string][] = [];
  for (let i = 0; i < body.length; i++) {
    const c = body.charAt(i);
    if (body.charAt(i + 1) === "-" && i + 2 < body.length) {
      ranges.push([c, body.charAt(i + 2)]);
      i += 2;
    } else ranges.push([c, c]);
  }
  return (x) => ranges.some(([lo, hi]) => x >= lo && x <= hi);
}

/**
 * Whether `tokens` match all of `path`: the set of path positions reached
 * after each token, so time grows with tokens times path length squared at
 * worst, never exponentially, whatever the glob.
 */
function matchTokens(tokens: Token[], path: string): boolean {
  const n = path.length;
  let at = new Set<number>([0]);
  for (const token of tokens) {
    const next = new Set<number>();
    for (const j of at) {
      if (token.kind === "char") {
        if (j < n && token.test(path.charAt(j))) next.add(j + 1);
      } else if (token.kind === "star") {
        for (let k = j; k <= n; k++) {
          next.add(k);
          if (path.charAt(k) === "/") break;
        }
      } else if (token.kind === "globstar") {
        for (let k = j; k <= n; k++) next.add(k);
      } else if (token.kind === "dirs") {
        next.add(j);
        for (let k = j; k < n; k++) if (path.charAt(k) === "/") next.add(k + 1);
      } else {
        const digits = /^-?\d+/.exec(path.slice(j, j + 20))?.[0];
        // Only the whole run of digits: `v1` must not match the start of `v12`.
        if (digits && !/^0\d/.test(digits.replace("-", ""))) {
          const value = Number(digits);
          if (value >= token.lo && value <= token.hi) next.add(j + digits.length);
        }
      }
    }
    if (next.size === 0) return false;
    at = next;
  }
  return at.has(n);
}

/** The index of the `}` that closes the `{` at `open`, or -1. */
function closingBrace(glob: string, open: number): number {
  let depth = 0;
  for (let i = open; i < glob.length; i++) {
    const c = glob.charAt(i);
    if (c === "\\") i++;
    else if (c === "{") depth++;
    else if (c === "}" && --depth === 0) return i;
  }
  return -1;
}

/** Splits `a,{b,c},d` at its top-level commas. */
function splitTop(body: string): string[] {
  const parts: string[] = [];
  let depth = 0;
  let start = 0;
  for (let i = 0; i < body.length; i++) {
    const c = body.charAt(i);
    if (c === "\\") i++;
    else if (c === "{") depth++;
    else if (c === "}") depth--;
    else if (c === "," && depth === 0) {
      parts.push(body.slice(start, i));
      start = i + 1;
    }
  }
  parts.push(body.slice(start));
  return parts;
}

/**
 * Whether section `glob` covers `path`, relative to the folder of its
 * `.editorconfig`: a glob without a slash matches the file name at any depth.
 */
export function matches(glob: string, path: string): boolean {
  const anchored = glob.includes("/") ? glob.replace(/^\//, "") : `**/${glob}`;
  const globs = expand(anchored);
  return !!globs && globs.some((one) => matchTokens(tokenize(one), path));
}

/** What `files` say about `path`, nearest file last; paths are project-relative, folders "" for the root. */
export function resolve(files: { dir: string; parsed: Parsed }[], path: string): EditorConfig {
  const props: Record<string, string> = {};
  for (const { dir, parsed } of files) {
    const relative = dir === "" ? path : path.slice(dir.length + 1);
    for (const section of parsed.sections) {
      if (matches(section.glob, relative)) Object.assign(props, section.props);
    }
  }
  const config: EditorConfig = {};
  const lower = (key: string) => props[key]?.toLowerCase();
  const style = lower("indent_style");
  if (style === "tab" || style === "space") config.indentStyle = style;
  const size = lower("indent_size");
  const width = (value: string | undefined) => {
    const n = Number(value);
    return Number.isInteger(n) && n >= 1 && n <= 16 ? n : undefined;
  };
  if (size === "tab") config.indentSize = "tab";
  else if (width(size)) config.indentSize = width(size);
  const tabWidth = width(lower("tab_width"));
  if (tabWidth) config.tabWidth = tabWidth;
  if (lower("trim_trailing_whitespace") === "true") config.trimTrailingWhitespace = true;
  if (lower("insert_final_newline") === "true") config.insertFinalNewline = true;
  return config;
}

/**
 * How a file indents: "Tab" or "Spaces: N" (as `detectIndent` says it) and
 * the columns of a tab, from `config` first, then `detected`, then nothing.
 */
export function configuredIndent(
  config: EditorConfig,
  detected: string | null,
): { label: string | null; tabWidth: number | null } {
  const size = config.indentSize === "tab" ? config.tabWidth : config.indentSize;
  const tabWidth = config.tabWidth ?? (typeof config.indentSize === "number" ? config.indentSize : null);
  const style = config.indentStyle ?? (detected === "Tab" ? "tab" : detected ? "space" : size ? "space" : undefined);
  if (style === "tab") return { label: "Tab", tabWidth };
  if (style === "space") {
    const detectedWidth = Number(/^Spaces: (\d+)$/.exec(detected ?? "")?.[1]) || null;
    const width = size ?? detectedWidth;
    return { label: width ? `Spaces: ${width}` : null, tabWidth };
  }
  return { label: detected, tabWidth };
}

/**
 * The edits saving makes as `config` says: no spaces or tabs at line ends
 * (except on the lines in `keep`, where someone is typing), a final line break.
 */
export function saveEdits(config: EditorConfig, doc: Text, keep: ReadonlySet<number> = new Set()): ChangeSpec[] {
  const edits: ChangeSpec[] = [];
  if (config.trimTrailingWhitespace) {
    for (let n = 1; n <= doc.lines; n++) {
      if (keep.has(n)) continue;
      const line = doc.line(n);
      const trailing = /[ \t]+$/.exec(line.text)?.[0].length ?? 0;
      if (trailing) edits.push({ from: line.to - trailing, to: line.to });
    }
  }
  if (config.insertFinalNewline && doc.length > 0 && doc.sliceString(doc.length - 1) !== "\n") {
    edits.push({ from: doc.length, insert: "\n" });
  }
  return edits;
}

/** The `.editorconfig` files of one project, read when a file opens and kept until one changes. */
export class EditorConfigs {
  private readonly files = new Map<string, Promise<Parsed | null>>();

  constructor(
    private readonly transport: Transport,
    private readonly project: ProjectId,
  ) {}

  /** Forgets what was read; call when a `.editorconfig` changed. */
  forget(paths: readonly string[]): void {
    if (paths.some((path) => path === ".editorconfig" || path.endsWith("/.editorconfig"))) this.files.clear();
  }

  /** What the project's `.editorconfig` files say about `path`; nothing when there are none or they cannot be read. */
  async for(path: string): Promise<EditorConfig> {
    const parts = path.split("/").slice(0, -1);
    const found: { dir: string; parsed: Parsed }[] = [];
    for (let depth = parts.length; depth >= 0; depth--) {
      const dir = parts.slice(0, depth).join("/");
      const parsed = await this.read(dir);
      if (!parsed) continue;
      found.unshift({ dir, parsed });
      if (parsed.root) break;
    }
    return resolve(found, path);
  }

  private read(dir: string): Promise<Parsed | null> {
    let file = this.files.get(dir);
    if (!file) {
      file = this.load(dir);
      this.files.set(dir, file);
    }
    return file;
  }

  private async load(dir: string): Promise<Parsed | null> {
    try {
      const { data } = await this.transport.call({
        method: "fs.read_file",
        params: { project: this.project, path: dir === "" ? ".editorconfig" : `${dir}/.editorconfig` },
      });
      return data.body.kind === "text" ? parse(data.body.text) : null;
    } catch (err) {
      // Missing is the usual case. Nothing stops the file from opening; a lost connection reads again next time.
      if (!(err instanceof CoreError)) this.files.delete(dir);
      return null;
    }
  }
}
