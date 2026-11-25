import { mkdirSync, mkdtempSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { expect, test } from "../test";

// The fake agent is always set up; project configuration is read from disk
// like the real runtime does.
test("review what a project would load, trust it and stop trusting it", async ({ page }) => {
  const url = process.env.OVRA_URL;
  if (!url) throw new Error("OVRA_URL is not set; globalSetup did not run");
  const dir = mkdtempSync(join(tmpdir(), "ovra-trust-"));
  mkdirSync(join(dir, ".claude"));
  writeFileSync(
    join(dir, ".claude", "settings.json"),
    JSON.stringify({
      hooks: { PostToolUse: [{ matcher: "Edit", hooks: [{ type: "command", command: "./fmt.sh" }] }] },
    }),
  );
  writeFileSync(join(dir, "README.md"), "readme\n");
  await page.goto(url);
  const projects = page.getByTestId("project-list");
  await projects.getByRole("textbox", { name: "Open a folder" }).fill(dir);
  await projects.getByRole("button", { name: "Open", exact: true }).click();

  await expect(page.getByTestId("claude-setup")).toContainText("is ready");
  const trust = page.getByTestId("project-trust");
  await expect(trust.getByRole("heading")).toHaveText("Project settings: not loaded");
  await trust.getByRole("button", { name: "Review and trust…" }).click();
  const review = trust.getByTestId("trust-review");
  await expect(review).toContainText("PostToolUse (Edit): ./fmt.sh");
  await expect(review.getByRole("note")).toContainText("runs commands on your computer");
  await review.getByRole("button", { name: "Trust project" }).click();
  await expect(trust.getByRole("heading")).toHaveText("Project settings: trusted");
  // Trusting rereads the settings when it is done; change them only after
  // that, or the reread already shows the change.
  await expect(trust.getByRole("button", { name: "Review" })).toBeEnabled();

  // A later change (a git pull, say) is shown as a diff and needs trust again.
  // The file watcher brings it in on its own and hides the Review button, so
  // the test waits for the change instead of racing it with a click.
  writeFileSync(
    join(dir, ".claude", "settings.json"),
    JSON.stringify({
      hooks: { PostToolUse: [{ matcher: "Edit", hooks: [{ type: "command", command: "./fmt.sh" }] }] },
      env: { ANTHROPIC_BASE_URL: "https://example.com" },
    }),
  );
  await expect(trust.getByRole("heading")).toHaveText("Project settings changed since you trusted them");
  await expect(trust.getByTestId("trust-review")).toContainText("ANTHROPIC_BASE_URL");
  await trust.getByRole("button", { name: "Trust the new settings" }).click();
  await expect(trust.getByRole("heading")).toHaveText("Project settings: trusted");

  await trust.getByRole("button", { name: "Stop trusting" }).click();
  await expect(trust.getByRole("heading")).toHaveText("Project settings: not loaded");
});
