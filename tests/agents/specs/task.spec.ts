import { execFileSync } from "node:child_process";
import { mkdirSync, mkdtempSync, writeFileSync } from "node:fs";
import { join } from "node:path";
import { expect, type Page, test } from "@playwright/test";

// A task on the real `claude` CLI: "Start task" starts a coordinator, which
// starts two workers through Ovra's MCP tools; each worker writes one file in
// its own worktree and asks for review. Small on purpose: the suite's model
// (haiku unless OVRA_LIVE_MODEL says otherwise) and a cost check keep it cheap.

/** The most the whole task may cost, in millionths of a dollar. */
const MAX_COST_MICRO_USD = 1_000_000;

function env(name: string): string {
  const value = process.env[name];
  if (!value) throw new Error(`${name} is not set; globalSetup did not run`);
  return value;
}

/** One request to the core over a socket of the page's own origin and token. */
async function core<T>(page: Page, method: string, params: unknown): Promise<T> {
  return page.evaluate(
    ({ method, params }) =>
      new Promise<T>((resolve, reject) => {
        const token = new URLSearchParams(location.search).get("token") ?? "";
        const socket = new WebSocket(`ws://${location.host}/ws?token=${encodeURIComponent(token)}`);
        socket.addEventListener("open", () =>
          socket.send(JSON.stringify(params === undefined ? { id: 1, method } : { id: 1, method, params })),
        );
        socket.addEventListener("message", (event) => {
          const frame = JSON.parse(String(event.data));
          if (frame.id !== 1) return;
          socket.close();
          if ("result" in frame) resolve(frame.result.data as T);
          else reject(new Error(JSON.stringify(frame.error)));
        });
        socket.addEventListener("error", () => reject(new Error("socket failed")));
      }),
    { method, params },
  );
}

type Review = { summary: string | null; files: { path: string }[] };
type Task = { id: string; workers: { name: string }[]; spent_micro_usd: number };

test("a coordinator splits a small task between two workers whose work waits for review", async ({ page }) => {
  const base = join(env("OVRA_LIVE_DIR"), "task");
  mkdirSync(base, { recursive: true });
  const dir = mkdtempSync(join(base, "w-"));
  writeFileSync(join(dir, "README.md"), "A tiny project for a live test.\n");
  const git = (...args: string[]) =>
    execFileSync("git", ["-c", "user.name=live", "-c", "user.email=live@example.invalid", ...args], { cwd: dir });
  git("init", "-q");
  git("add", ".");
  git("commit", "-q", "-m", "start");

  await page.goto(env("OVRA_URL"));
  const projects = page.getByTestId("project-list");
  await projects.getByRole("textbox", { name: "Open a folder" }).fill(dir);
  await projects.getByRole("button", { name: "Open", exact: true }).click();
  await expect(page.getByRole("treeitem", { name: /README\.md/ })).toBeVisible();
  await expect(page.getByTestId("claude-setup")).toContainText("is ready");
  const projectId = (await core<{ id: string; root: string }[]>(page, "projects.list", undefined)).find(
    (p) => p.root === dir,
  )?.id;
  if (!projectId) throw new Error(`no project for ${dir}`);

  await page
    .getByRole("textbox", { name: "Task for a new Claude session" })
    .fill(
      "Do not read any files. Start exactly two workers with spawn_agent: one named alpha whose task is " +
        '"Create the file alpha.txt containing the single word alpha, then call request_review", and one ' +
        'named beta whose task is "Create the file beta.txt containing the single word beta, then call ' +
        'request_review". Then end your turn without waiting for them.',
    );
  await page.getByRole("button", { name: "Start task" }).click();

  // Allow what the workers ask for (writing their file) until both asked
  // for review.
  const approvals = page.getByTestId("approvals");
  const reviewed = async () =>
    (await core<Review[]>(page, "review.list", { project: projectId })).filter((r) => r.summary).length;
  const deadline = Date.now() + 200_000;
  while ((await reviewed()) < 2) {
    if (Date.now() > deadline) throw new Error("the workers did not ask for review in time");
    const allow = approvals.getByRole("button", { name: "Allow", exact: true }).first();
    if (await allow.isVisible()) {
      await expect(allow).toHaveAttribute("aria-disabled", "false");
      await allow.click();
    } else {
      await page.waitForTimeout(1000);
    }
  }

  const reviews = await core<Review[]>(page, "review.list", { project: projectId });
  const files = reviews.flatMap((r) => r.files.map((f) => f.path)).sort();
  expect(files).toEqual(["alpha.txt", "beta.txt"]);
  const [task] = await core<Task[]>(page, "orchestration.list", { project: projectId });
  expect(task?.workers.map((w) => w.name).sort()).toEqual(["alpha", "beta"]);
  console.log(`task cost: $${((task?.spent_micro_usd ?? 0) / 1e6).toFixed(4)}`);
  expect(task?.spent_micro_usd ?? 0).toBeLessThan(MAX_COST_MICRO_USD);

  await page.keyboard.press("Alt+Shift+R");
  const inbox = page.getByRole("dialog", { name: "Review inbox" });
  await expect(inbox.getByRole("navigation", { name: "Review items" }).getByRole("button")).toHaveCount(2);
  if (task) await core(page, "orchestration.stop", { task: task.id });
});
