// Runs one UX review script against the real UI in headless mode.
//
//   just ux <script.ts> [--fixtures small,monorepo] [--seed small] [--out DIR]
//
// The script's default export gets a `Session` (see below) and drives the app
// like a person would. Console messages, page errors and every frame are
// written to <out>/session.log; `shot(name)` saves <out>/<name>.png.
// `--seed` adds fixtures to the project list before the page loads.

import { mkdirSync, writeFileSync } from "node:fs";
import { join, resolve } from "node:path";
import { pathToFileURL } from "node:url";
import { type Browser, type BrowserContext, chromium, type Page } from "@playwright/test";
import startDaemon from "../daemon.ts";
import { type FixtureName, makeFixtures } from "./fixtures.ts";

export type Session = {
  page: Page;
  context: BrowserContext;
  browser: Browser;
  /** The OVRA_URL with its token. */
  url: string;
  /** Fixture name -> absolute folder. */
  roots: Record<string, string>;
  /** Loads the app and waits until it is connected. */
  open: () => Promise<void>;
  /** Saves a screenshot to <out>/<name>.png and returns its path. */
  shot: (name: string) => Promise<string>;
  /** Adds a line to the session log. */
  note: (line: string) => void;
};

type Args = { script: string; fixtures: FixtureName[]; seed: FixtureName[]; out: string };

function parse(argv: string[]): Args {
  const args: Args = { script: "", fixtures: ["small"], seed: [], out: "" };
  for (let i = 0; i < argv.length; i++) {
    const a = argv[i];
    const next = (): string => argv[++i] ?? "";
    if (a === "--fixtures") args.fixtures = next().split(",").filter(Boolean) as FixtureName[];
    else if (a === "--seed") args.seed = next().split(",").filter(Boolean) as FixtureName[];
    else if (a === "--out") args.out = next();
    else if (a) args.script = a;
  }
  if (!args.script) throw new Error("usage: just ux <script.ts> [--fixtures a,b] [--seed a] [--out DIR]");
  for (const s of args.seed) if (!args.fixtures.includes(s)) args.fixtures.push(s);
  args.out ||= resolve("target/ux", new Date().toISOString().replace(/[:.]/g, "-"));
  return args;
}

/** Adds a folder to the project list through the core, as the start screen would. */
async function addProject(url: string, root: string): Promise<void> {
  const target = new URL(url);
  const ws = new WebSocket(`ws://${target.host}/ws?token=${target.searchParams.get("token")}`);
  await new Promise<void>((ok, fail) => {
    ws.onopen = () => ws.send(JSON.stringify({ id: 1, method: "projects.add", params: { root } }));
    ws.onmessage = (m) => {
      const frame = JSON.parse(String(m.data));
      if (frame.id !== 1) return;
      ws.close();
      if (frame.error) fail(new Error(`projects.add ${root}: ${frame.error.message}`));
      else ok();
    };
    ws.onerror = () => fail(new Error("cannot reach the core to seed projects"));
  });
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
  for (const name of args.seed) await addProject(url, roots[name] as string);

  const browser = await chromium.launch();
  const context = await browser.newContext({ viewport: { width: 1280, height: 800 } });
  const page = await context.newPage();
  page.on("console", (m) => note(`console.${m.type()} ${m.text()}`));
  page.on("pageerror", (e) => note(`pageerror ${e.message}`));

  await page.routeWebSocket(/\/ws/, (ws) => {
    const server = ws.connectToServer();
    server.onMessage((m) => {
      note(`<- ${String(m).slice(0, 300)}`);
      ws.send(m);
    });
    ws.onMessage((m) => {
      note(`-> ${String(m).slice(0, 300)}`);
      server.send(m);
    });
  });

  const session: Session = {
    page,
    context,
    browser,
    url,
    roots,
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
    stop();
  }
  console.log(`UX_OUT=${args.out}`);
  if (failed) {
    console.error(`script failed; see ${join(args.out, "session.log")}`);
    process.exit(1);
  }
}

await main();
