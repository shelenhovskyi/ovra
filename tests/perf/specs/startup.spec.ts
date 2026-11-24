// Cold start of headless mode with the 100k-file project in the project list,
// and the daemon's memory once it is open and idle.
import { mkdtempSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { expect, test } from "@playwright/test";
import { Client, median, REPO, settledRssMb, startDaemon } from "../daemon";
import { report } from "../metrics";

const RUNS = 3;

test("cold start to the 100k-file tree", async ({ page }) => {
  // A data dir that already knows the project, as after a previous session.
  const dataDir = mkdtempSync(join(tmpdir(), "ovra-perf-start-"));
  const seed = await startDaemon(dataDir);
  const client = await Client.connect(seed.url);
  await client.call("projects.add", { root: REPO });
  client.close();
  await seed.stop();

  const ready: number[] = [];
  const ui: number[] = [];
  const tree: number[] = [];
  const open: number[] = [];
  for (let run = 0; run < RUNS; run++) {
    const started = performance.now();
    const daemon = await startDaemon(dataDir);
    try {
      await page.goto(daemon.url);
      const button = page.getByTestId("project-list").getByRole("button", { name: "repo-100k" });
      await button.waitFor();
      const uiReady = performance.now();
      await button.click();
      await page.getByTestId("file-tree").getByRole("treeitem").first().waitFor();
      const shown = performance.now();
      ready.push(daemon.readyMs);
      ui.push(uiReady - started);
      tree.push(shown - started);
      open.push(shown - uiReady);
      if (run === RUNS - 1) {
        await expect(page.getByTestId("file-tree").getByRole("treeitem", { name: "pkg00" })).toBeVisible();
        report("memory.daemon_idle_mb", await settledRssMb(daemon.pid));
      }
    } finally {
      await daemon.stop();
    }
  }
  report("startup.daemon_ready_ms", median(ready));
  report("startup.ui_ready_ms", median(ui));
  report("startup.start_to_tree_ms", median(tree));
  report("startup.open_to_tree_ms", median(open));
});
