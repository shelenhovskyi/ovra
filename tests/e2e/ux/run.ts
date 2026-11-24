// Runs one UX review script against the real UI in headless mode.
//
//   just ux <script.ts> [--fixtures small,monorepo] [--seed small] [--real-core] [--out DIR]
//
// The script's default export gets a `Session` (see below) and drives the app
// like a person would. Console messages, page errors and every frame are
// written to <out>/session.log; `shot(name)` saves <out>/<name>.png.
// Until the project store and viewers land, `projects.*` and `fs.*` are
// answered by ./fake-core.ts from real folders; `--real-core` turns that off.

import { mkdirSync, writeFileSync } from "node:fs";
import { join, resolve } from "node:path";
import { pathToFileURL } from "node:url";
import { type Browser, type BrowserContext, chromium, type Page } from "@playwright/test";
import startDaemon from "../daemon.ts";
import { FakeCore } from "./fake-core.ts";
import { type FixtureName, makeFixtures } from "./fixtures.ts";

export type Session = {
  page: Page;
  context: BrowserContext;
  browser: Browser;
  /** The OVRA_URL with its token. */
  url: string;
  /** Fixture name -> absolute folder. */
  roots: Record<string, string>;
  /** The stand-in core, or null with `--real-core`. */
  core: FakeCore | null;
  /** Loads the app and waits until it is connected. */
  open: () => Promise<void>;
  /** Saves a screenshot to <out>/<name>.png and returns its path. */
  shot: (name: string) => Promise<string>;
  /** Adds a line to the session log. */
  note: (line: string) => void;
};

type Args = { script: string; fixtures: FixtureName[]; seed: FixtureName[]; realCore: boolean; out: string };

function parse(argv: string[]): Args {
  const args: Args = { script: "", fixtures: ["small"], seed: [], realCore: false, out: "" };
  for (let i = 0; i < argv.length; i++) {
    const a = argv[i];
    const next = (): string => argv[++i] ?? "";
    if (a === "--fixtures") args.fixtures = next().split(",").filter(Boolean) as FixtureName[];
    else if (a === "--seed") args.seed = next().split(",").filter(Boolean) as FixtureName[];
    else if (a === "--real-core") args.realCore = true;
    else if (a === "--out") args.out = next();
    else if (a) args.script = a;
  }
  if (!args.script) throw new Error("usage: just ux <script.ts> [--fixtures a,b] [--seed a] [--real-core] [--out DIR]");
  for (const s of args.seed) if (!args.fixtures.includes(s)) args.fixtures.push(s);
  args.out ||= resolve("target/ux", new Date().toISOString().replace(/[:.]/g, "-"));
  return args;
}

async function main(): Promise<void> {
  const args = parse(process.argv.slice(2));
  mkdirSync(args.out, { recursive: true });
  const lines: string[] = [];
  const note = (line: string): void => {
    lines.push(`${new Date().toISOString()} ${line}`);
  };

  const stop = await startDaemon();
  const url = process.env.OVRA_URL ?? "";
  const roots = makeFixtures(args.fixtures);
  note(`fixtures ${JSON.stringify(roots)}`);
  const core = args.realCore ? null : new FakeCore(note);
  for (const name of args.seed) core?.seed(roots[name] as string, Date.now());

  const browser = await chromium.launch();
  const context = await browser.newContext({ viewport: { width: 1280, height: 800 } });
  const page = await context.newPage();
  page.on("console", (m) => note(`console.${m.type()} ${m.text()}`));
  page.on("pageerror", (e) => note(`pageerror ${e.message}`));

  await page.routeWebSocket(/\/ws/, (ws) => {
    const server = ws.connectToServer();
    const off = core?.onEvent((event) => {
      const frame = JSON.stringify({ event });
      note(`<- ${frame.slice(0, 300)}`);
      ws.send(frame);
    });
    ws.onClose(() => off?.());
    server.onMessage((m) => {
      note(`<- ${String(m).slice(0, 300)}`);
      ws.send(m);
    });
    ws.onMessage((m) => {
      note(`-> ${String(m).slice(0, 300)}`);
      const frame = JSON.parse(String(m));
      const reply = core?.handle(frame);
      if (!reply) return server.send(m);
      const text = JSON.stringify({ id: frame.id, ...reply });
      note(`<- (fake) ${text.slice(0, 300)}`);
      ws.send(text);
    });
  });

  const session: Session = {
    page,
    context,
    browser,
    url,
    roots,
    core,
    note,
    open: async () => {
      await page.goto(url);
      await page
        .getByTestId("status")
        .filter({ hasText: /^Connected/ })
        .waitFor({ timeout: 5000 });
    },
    shot: async (name) => {
      const path = join(args.out, `${name}.png`);
      await page.screenshot({ path });
      note(`screenshot ${path}`);
      return path;
    },
  };

  let failed = false;
  try {
    const script = await import(pathToFileURL(resolve(args.script)).href);
    await script.default(session);
  } catch (err) {
    failed = true;
    note(`script failed: ${err instanceof Error ? (err.stack ?? err.message) : String(err)}`);
    await session.shot("failure").catch(() => {});
  } finally {
    writeFileSync(join(args.out, "session.log"), `${lines.join("\n")}\n`);
    await browser.close();
    core?.close();
    stop();
  }
  console.log(`UX_OUT=${args.out}`);
  if (failed) {
    console.error(`script failed; see ${join(args.out, "session.log")}`);
    process.exit(1);
  }
}

await main();
