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

/** The regular expression source for an EditorConfig glob. */
function globSource(glob: string): string {
  let out = "";
  for (let i = 0; i < glob.length; i++) {
    const c = glob.charAt(i);
    if (c === "\\" && i + 1 < glob.length) {
      out += literal(glob.charAt(++i));
    } else if (c === "*") {
      if (glob.charAt(i + 1) === "*") {
        out += ".*";
        i++;
      } else out += "[^/]*";
    } else if (c === "?") {
      out += "[^/]";
    } else if (c === "[") {
      const end = glob.indexOf("]", i + 1);
      if (end < 0) {
        out += "\\[";
        continue;
      }
      const body = glob.slice(i + 1, end);
      out += body.startsWith("!") ? `[^/${body.slice(1)}]` : `[${body}]`;
      i = end;
    } else if (c === "{") {
      const end = closingBrace(glob, i);
      if (end < 0) {
        out += "\\{";
        continue;
      }
      const body = glob.slice(i + 1, end);
      const range = /^(-?\d+)\.\.(-?\d+)$/.exec(body);
      if (range) {
        const lo = Math.min(Number(range[1]), Number(range[2]));
        const hi = Math.max(Number(range[1]), Number(range[2]));
        out += `(?:${Array.from({ length: Math.min(hi - lo, 9999) + 1 }, (_, k) => lo + k).join("|")})`;
      } else {
        const options = splitTop(body);
        out += options.length > 1 ? `(?:${options.map(globSource).join("|")})` : `\\{${globSource(body)}\\}`;
      }
      i = end;
    } else {
      out += literal(c);
    }
  }
  return out;
}

function literal(c: string): string {
  return /[.+^$()|{}[\]\\/]/.test(c) ? `\\${c}` : c;
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
  const source = globSource(anchored).replace(/^\.\*\\\//, "(?:.*/)?");
  try {
    return new RegExp(`^${source}$`).test(path);
  } catch {
    return false;
  }
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
