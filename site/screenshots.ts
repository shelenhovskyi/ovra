// Takes the screenshots on the site's landing page from the real UI in
// headless mode, with Ovra's own repository open and the fake agent:
//
//   just site-screenshots
//
// Writes site/public/screenshots/{workspace,agents,editor}.png.

import { execFileSync } from "node:child_process";
import { mkdtempSync } from "node:fs";
import { tmpdir } from "node:os";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import type { Session } from "../tests/e2e/ux/run.ts";

type Page = Session["page"];

const REPO = resolve(dirname(fileURLToPath(import.meta.url)), "..");
const OUT = join(REPO, "site/public/screenshots");

/** A clean copy of the repository at a short path, so the screenshots show committed files only. */
function cloneRepo(): string {
  const dir = join(mkdtempSync(join(tmpdir(), "ovra-site-")), "ovra");
  execFileSync("git", ["clone", "-q", "--depth", "8", "--single-branch", `file://${REPO}`, dir]);
  execFileSync("git", ["branch", "-m", "main"], { cwd: dir });
  execFileSync("git", ["remote", "remove", "origin"], { cwd: dir });
  return dir;
}

async function settle(page: Page): Promise<void> {
  await page.evaluate(() => document.fonts.ready);
  await page.mouse.move(0, 0);
  await page.waitForTimeout(300);
}

export default async function screenshots(session: Session): Promise<void> {
  const root = cloneRepo();
  const context = await session.browser.newContext({
    viewport: { width: 1440, height: 900 },
    deviceScaleFactor: 2,
    colorScheme: "dark",
  });
  const page = await context.newPage();
  await page.goto(session.url);
  const projects = page.getByTestId("project-list");
  await projects.getByRole("textbox", { name: "Open a folder" }).fill(root);
  await projects.getByRole("button", { name: "Open", exact: true }).click();
  const tree = page.getByRole("tree", { name: "Files" });
  await tree.waitFor();

  // Workspace: the architecture document, with crates/ and docs/ expanded.
  for (const name of ["crates", "docs"]) {
    await tree.getByRole("treeitem", { name: new RegExp(`^${name}\\b`) }).click();
  }
  await tree.getByRole("treeitem", { name: /^architecture\.md/ }).click();
  await page.getByRole("tab", { name: "architecture.md" }).waitFor();
  await settle(page);
  await page.screenshot({ path: join(OUT, "workspace.png") });

  // Editor and terminal: a Rust file in edit mode, a shell below.
  await page.keyboard.press("Alt+P");
  await page.getByRole("combobox").fill("crates/core/src/path.rs");
  await page.getByRole("option", { name: "crates/core/src/path.rs" }).click();
  await page.getByRole("tab", { name: /path\.rs/ }).waitFor();
  await page.keyboard.press("Control+E");
  await page.getByTestId("main").locator(".tok-keyword").first().waitFor();
  await page.keyboard.press("Control+Backquote");
  const terminal = page.getByRole("region", { name: "Terminal panel" });
  await terminal.waitFor();
  const rows = terminal.locator(".xterm-rows");
  await rows.getByText("#").or(rows.getByText("$")).first().waitFor();
  await page.keyboard.type("export PS1='\\W $ '; clear\n");
  await rows.getByText(/^ovra \$/).waitFor();
  await page.keyboard.type("git log --oneline -6\n");
  await terminal.locator(".xterm-rows").getByText("license").waitFor();
  await settle(page);
  await page.screenshot({ path: join(OUT, "editor.png") });
  await page.keyboard.press("Control+Backquote");

  // Agents: a fake Claude session waiting for approval to run a command.
  await page.getByRole("textbox", { name: "What should Claude do?" }).fill("Add a test for RelPath::parse");
  await page.getByRole("button", { name: "Start", exact: true }).click();
  const run = page.getByTestId("approvals").getByRole("group", { name: "Run ls -la" });
  await run.waitFor();
  // Approvals can be answered 500 ms after they appear; wait so the buttons look enabled.
  await run.getByRole("button", { name: "Allow" }).and(page.locator('[aria-disabled="false"]')).waitFor();
  await settle(page);
  await page.screenshot({ path: join(OUT, "agents.png") });
  await context.close();
}
