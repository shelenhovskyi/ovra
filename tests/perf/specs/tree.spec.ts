// The virtualized file tree with a 5,000-entry folder expanded.
import { expect, test } from "@playwright/test";
import { Client, type Daemon, REPO, startDaemon } from "../daemon";
import { report } from "../metrics";

let daemon: Daemon;

test.beforeAll(async () => {
  daemon = await startDaemon();
  const client = await Client.connect(daemon.url);
  await client.call("projects.add", { root: REPO });
  client.close();
});

test.afterAll(async () => {
  await daemon?.stop();
});

test("expand, scroll and memory with 5,000 rows", async ({ page }) => {
  await page.goto(daemon.url);
  await page.getByTestId("project-list").getByRole("button", { name: "repo-100k" }).click();
  const tree = page.getByTestId("file-tree");
  const row = (path: string) => tree.locator(`[data-path="${path}"]`);
  await row("flat").waitFor();

  let started = performance.now();
  await row("flat").click();
  await row("flat/item0000.txt").waitFor();
  report("tree.expand_flat_ms", performance.now() - started);
  await expect(row("flat")).toHaveAttribute("aria-setsize", /^50\d\d$/);

  report("tree.dom_rows", await tree.getByRole("treeitem").count());

  started = performance.now();
  await tree.evaluate((el) => {
    el.scrollTop = el.scrollHeight;
  });
  await row("README.md").waitFor();
  report("tree.scroll_jump_ms", performance.now() - started);

  // Scroll through the list one step per frame and measure frame times.
  const frames = await tree.evaluate(
    (el) =>
      new Promise<number[]>((done) => {
        el.scrollTop = 0;
        const deltas: number[] = [];
        let last = 0;
        const step = (now: number) => {
          if (last) deltas.push(now - last);
          last = now;
          el.scrollTop += 600;
          if (deltas.length < 120) requestAnimationFrame(step);
          else done(deltas);
        };
        requestAnimationFrame(step);
      }),
  );
  frames.sort((a, b) => a - b);
  report("tree.scroll_frame_p95_ms", frames[Math.floor(frames.length * 0.95)] ?? null);
  report("tree.dom_rows", await tree.getByRole("treeitem").count());

  const cdp = await page.context().newCDPSession(page);
  await cdp.send("HeapProfiler.collectGarbage");
  await cdp.send("Performance.enable");
  const { metrics } = await cdp.send("Performance.getMetrics");
  const heap = metrics.find((m) => m.name === "JSHeapUsedSize")?.value;
  report("memory.ui_js_heap_mb", heap === undefined ? null : heap / 1024 / 1024);
});
