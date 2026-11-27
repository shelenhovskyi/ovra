// "Teach the project" and "Remember this": the text Ovra adds to the
// project's AGENTS.md. Agents cannot write the rules files (the sandbox keeps
// them read-only), so Ovra writes only what the person saw and pressed for:
// commands found in the project's own build files, their answers, and a
// rule from a review comment.

import type { ProjectId } from "../lib/protocol";
import { CoreError, type Transport } from "../lib/transport";

export const RULES_FILE = "AGENTS.md";

/** The files the commands are read from, in the order they are listed. */
export const BUILD_FILES = [
  "justfile",
  "Justfile",
  "package.json",
  "Makefile",
  "Cargo.toml",
  "go.mod",
  "pyproject.toml",
  "pnpm-lock.yaml",
  "yarn.lock",
  "bun.lockb",
] as const;

/** At most this many commands are suggested. */
const MAX_COMMANDS = 12;
const SCRIPTS = ["dev", "start", "build", "test", "lint", "check", "typecheck", "format"];
const JUST_WORDS = new Set(["set", "alias", "export", "import", "mod"]);

/** One suggested line per command: "`just test` - Rust and UI unit tests". */
export function detectCommands(files: Partial<Record<string, string>>): string[] {
  const lines: string[] = [];
  const just = files.justfile ?? files.Justfile;
  if (just !== undefined) lines.push(...justRecipes(just));
  const pkg = files["package.json"];
  if (pkg !== undefined) {
    const runner =
      files["pnpm-lock.yaml"] !== undefined
        ? "pnpm"
        : files["yarn.lock"] !== undefined
          ? "yarn"
          : files["bun.lockb"] !== undefined
            ? "bun"
            : "npm";
    lines.push(...packageScripts(pkg, runner));
  }
  if (files.Makefile !== undefined) lines.push(...makeTargets(files.Makefile));
  if (files["Cargo.toml"] !== undefined) lines.push("`cargo build` - build", "`cargo test` - run the tests");
  if (files["go.mod"] !== undefined) lines.push("`go build ./...` - build", "`go test ./...` - run the tests");
  if (files["pyproject.toml"]?.includes("pytest")) lines.push("`pytest` - run the tests");
  return [...new Set(lines)].slice(0, MAX_COMMANDS);
}

function justRecipes(text: string): string[] {
  const found: string[] = [];
  let comment = "";
  for (const line of text.split("\n")) {
    const note = /^#\s*(.+)$/.exec(line);
    if (note) {
      comment = note[1]?.trim() ?? "";
      continue;
    }
    const recipe = /^@?([A-Za-z_][\w-]*)[^:=]*:(?!=)/.exec(line);
    const name = recipe?.[1];
    if (name && !JUST_WORDS.has(name) && !name.startsWith("_")) {
      found.push(comment ? `\`just ${name}\` - ${comment}` : `\`just ${name}\``);
    }
    comment = "";
  }
  return found;
}

function packageScripts(text: string, runner: string): string[] {
  let scripts: unknown;
  try {
    scripts = (JSON.parse(text) as { scripts?: unknown }).scripts;
  } catch {
    return [];
  }
  if (typeof scripts !== "object" || scripts === null) return [];
  const names = Object.keys(scripts);
  return SCRIPTS.filter((name) => names.includes(name)).map((name) => `\`${runner} run ${name}\``);
}

function makeTargets(text: string): string[] {
  const found: string[] = [];
  for (const line of text.split("\n")) {
    const target = /^([A-Za-z][\w-]*)\s*:(?!=)/.exec(line)?.[1];
    if (target && !found.includes(target)) found.push(target);
  }
  return found.map((target) => `\`make ${target}\``);
}

/** The person's answer as list items: one per non-empty line, without a typed "- ". */
export function bullets(answer: string): string[] {
  return answer
    .split("\n")
    .map((line) => line.trim().replace(/^[-*•]\s+/, ""))
    .filter((line) => line.length > 0);
}

export type Answers = { commands: string; rules: string; never: string };

const SECTIONS: { key: keyof Answers; heading: string }[] = [
  { key: "commands", heading: "Commands" },
  { key: "rules", heading: "Rules" },
  { key: "never", heading: "Do not touch" },
];

const HEADER = "# AGENTS.md\n\nInstructions for the coding agents that work in this project.\n";

/**
 * The rules file with the answers in it: a new file when there is none,
 * else each answer's items added at the end of its section, which is added
 * at the end of the file when it has none. Items already there are skipped.
 */
export function withAnswers(existing: string | null, answers: Answers): string {
  let text = existing ?? HEADER;
  for (const { key, heading } of SECTIONS) text = addToSection(text, heading, bullets(answers[key]));
  return text;
}

/** `text` with `items` as "- " lines at the end of its `## heading` section. */
export function addToSection(text: string, heading: string, items: string[]): string {
  const lines = text.replace(/\s+$/, "").split("\n");
  const fresh = items.filter((item) => !lines.some((line) => line.trim() === `- ${item}`));
  if (fresh.length === 0) return text;
  const added = fresh.map((item) => `- ${item}`);
  const start = lines.findIndex((line) => line.trim().toLowerCase() === `## ${heading.toLowerCase()}`);
  if (start === -1) return `${[...lines, "", `## ${heading}`, "", ...added].join("\n")}\n`;
  let end = lines.findIndex((line, i) => i > start && /^#{1,2}\s/.test(line));
  if (end === -1) end = lines.length;
  while (end > start + 1 && lines[end - 1]?.trim() === "") end--;
  const head = lines.slice(0, end);
  const tail = lines.slice(end);
  while (tail[0]?.trim() === "") tail.shift();
  return `${[...head, ...(end === start + 1 ? [""] : []), ...added, ...(tail.length > 0 ? ["", ...tail] : [])].join("\n")}\n`;
}

/** Whether `CLAUDE.md` already brings `AGENTS.md` in (`@AGENTS.md`). */
export function importsRules(claudeMd: string): boolean {
  return claudeMd.split("\n").some((line) => line.trim() === `@${RULES_FILE}`);
}

/** `CLAUDE.md` with `@AGENTS.md` at its top, so Claude reads both. */
export function withImport(claudeMd: string): string {
  return importsRules(claudeMd) ? claudeMd : `@${RULES_FILE}\n\n${claudeMd}`;
}

/** A project file's text and time, or null when it is missing. */
export async function readText(
  transport: Transport,
  project: ProjectId,
  path: string,
): Promise<{ text: string; mtime: number } | null> {
  try {
    const reply = await transport.call({ method: "fs.read_file", params: { project, path } });
    const body = reply.data.body;
    if (body.kind !== "text") throw new Error(`${path} is not a text file.`);
    return { text: body.text, mtime: reply.data.mtime_ms };
  } catch (err) {
    if (err instanceof CoreError && err.error.code === "not_found") return null;
    throw err;
  }
}

/** Writes `text` to `path`, refused when the file changed since `mtime` was read (null: a new file). */
export async function writeText(
  transport: Transport,
  project: ProjectId,
  path: string,
  text: string,
  mtime: number | null,
): Promise<void> {
  await transport.call({ method: "fs.write_file", params: { project, path, text, expected_mtime_ms: mtime } });
}

/** Adds `rule` under "Rules" in the project's AGENTS.md (made when missing). */
export async function rememberRule(transport: Transport, project: ProjectId, rule: string): Promise<void> {
  const file = await readText(transport, project, RULES_FILE);
  const text = addToSection(file?.text ?? HEADER, "Rules", bullets(rule.replace(/\n+/g, " ")));
  if (text !== file?.text) await writeText(transport, project, RULES_FILE, text, file?.mtime ?? null);
}
