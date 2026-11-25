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

  // Claude is ready: no setup card; one footer line under the start form says so.
  await expect(page.getByTestId("agents-footer")).toHaveText("Claude · Ready");
  await expect(page.getByTestId("claude-setup")).toHaveCount(0);
  const trust = page.getByTestId("project-trust");
  await expect(trust.getByRole("heading")).toHaveText("This project has its own Claude settings");
  await expect(trust.getByTestId("trust-summary")).toHaveText("1 hook that runs code");
  await expect(trust.getByTestId("trust-runs-code")).toHaveText(
    '⚠ Once you trust this project, 1 hook will run commands on your computer: "PostToolUse (Edit): ./fmt.sh".',
  );
  await trust.getByRole("button", { name: "Show details" }).click();
  await expect(trust.getByTestId("trust-details")).toContainText("PostToolUse (Edit): ./fmt.sh");
  await trust.getByRole("button", { name: "Trust project" }).click();
  // Trusted: one quiet line; the rest is under Details.
  await expect(trust.getByRole("heading")).toHaveText("Trusted project settings");
  await expect(trust.getByTestId("trust-warning-count")).toHaveText("⚠ 1 warning");
  await trust.getByRole("button", { name: "Details" }).click();
  await expect(trust.getByRole("heading")).toHaveText("Project settings are on");
  // Trusting rereads the settings when it is done; change them only after
  // that, or the reread already shows the change.
  await expect(trust.getByRole("button", { name: "Stop trusting" })).toBeEnabled();

  // A later change (a git pull, say) is summed up, shown as a diff and needs
  // trust again. The file watcher brings it in on its own, so the test waits
  // for the change instead of racing it with a click.
  writeFileSync(
    join(dir, ".claude", "settings.json"),
    JSON.stringify({
      hooks: { PostToolUse: [{ matcher: "Edit", hooks: [{ type: "command", command: "./fmt.sh" }] }] },
      env: { ANTHROPIC_BASE_URL: "https://example.com" },
    }),
  );
  await expect(trust.getByRole("heading")).toHaveText("Project settings changed");
  await expect(trust.getByTestId("trust-summary")).toHaveText("1 added");
  await expect(trust.getByTestId("trust-risky-env")).toContainText("New project settings set ANTHROPIC_BASE_URL");
  // Details is still open from before.
  await expect(trust.getByTestId("trust-details")).toContainText("ANTHROPIC_BASE_URL");
  await trust.getByRole("button", { name: "Trust the new settings" }).click();
  await expect(trust.getByRole("heading")).toHaveText("Trusted project settings");
  await trust.getByRole("button", { name: "Details" }).click();

  await trust.getByRole("button", { name: "Stop trusting" }).click();
  await expect(trust.getByRole("heading")).toHaveText("This project has its own Claude settings");
});
