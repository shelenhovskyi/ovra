import { mkdirSync, mkdtempSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { clickWhenArmed, expect, loadProject, test } from "../test";

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
  // Trust buttons wait a moment after the card changes; the clock skips that.
  await page.clock.install();
  await loadProject(page, dir, url);

  // Claude is ready: no setup card; one footer line under the start form says so.
  await expect(page.getByTestId("agents-footer")).toHaveText("Claude · Ready");
  await expect(page.getByTestId("claude-setup")).toHaveCount(0);
  const trust = page.getByTestId("project-trust");
  // No card until asked: the Agents header counts it, and its menu opens the card.
  await expect(trust).toHaveCount(0);
  await page.getByTestId("agents-attention").click();
  await expect(page.getByTestId("attention-trust")).toHaveText(
    /Project settings are off\s*1 warning · Review and trust…/,
  );
  await page.getByTestId("attention-trust").click();
  await expect(trust.getByRole("heading")).toHaveText("This project has its own Claude settings");
  await expect(trust.getByTestId("trust-runs-code")).toHaveText(
    "⚠ Once you trust this project, 1 hook will run commands on your computer.",
  );
  // Trust is at the bottom of the full list, never on the card itself.
  await expect(trust.getByRole("button", { name: "Trust project" })).toHaveCount(0);
  await trust.getByRole("button", { name: "Review and trust…" }).click();
  await expect(trust.getByTestId("trust-details")).toContainText("PostToolUse (Edit): ./fmt.sh");
  await clickWhenArmed(page, trust.getByTestId("trust-details").getByRole("button", { name: "Trust project" }));
  // Trusted: one quiet line; the rest is under Details, which turns into Hide in the same place.
  await expect(trust.getByRole("heading")).toHaveText("Project's own settings are on");
  await expect(trust.getByTestId("trust-warning-count")).toHaveText("⚠ 1 warning");
  await trust.getByRole("button", { name: "Details" }).click();
  await expect(trust.getByRole("button", { name: "Hide", exact: true })).toHaveAttribute("aria-expanded", "true");
  await expect(trust.getByTestId("trust-runs-code")).toHaveText("⚠ 1 hook runs commands on your computer.");
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
  await trust.getByRole("button", { name: "Review and trust…" }).click();
  await expect(trust.getByTestId("trust-details")).toContainText("ANTHROPIC_BASE_URL");
  await clickWhenArmed(page, trust.getByRole("button", { name: "Trust the new settings" }));
  await expect(trust.getByTestId("trust-warning-count")).toHaveText("⚠ 2 warnings");
  await trust.getByRole("button", { name: "Details" }).click();

  await trust.getByRole("button", { name: "Stop trusting" }).click();
  // Not trusted again: the card goes, and the header counts the decision again.
  await expect(trust).toHaveCount(0);
  await page.getByTestId("agents-attention").click();
  await expect(page.getByTestId("attention-trust")).toContainText("Project settings are off");
});

test("check setup lists each thing Ovra needs, with what was found", async ({ page }) => {
  const dir = mkdtempSync(join(tmpdir(), "ovra-setup-"));
  writeFileSync(join(dir, "README.md"), "readme\n");
  await loadProject(page, dir);
  await expect(page.getByTestId("agents-footer")).toHaveText("Claude · Ready");

  await page.keyboard.press("ControlOrMeta+K");
  await page.getByRole("combobox", { name: "Search files, or type > for commands" }).fill(">check setup");
  await expect(page.getByRole("option", { selected: true })).toContainText(/Settings\s*Setup check\b/);
  await page.keyboard.press("Enter");

  const pane = page.getByTestId("setup-check");
  await expect(pane.getByTestId("setup-agent:claude")).toContainText("is ready");
  await expect(pane.getByTestId("setup-git")).toContainText(/git [\d.]+ is installed/);
  // A folder without an origin: gh's sign-in has no host to be checked for.
  await expect(pane.getByTestId("setup-gh")).toContainText(/gh|GitHub/);
  await expect(pane.getByTestId("setup-sandbox")).toBeVisible();
  await pane.getByRole("button", { name: "Check again" }).click();
  await expect(pane.getByRole("status")).not.toHaveText("Checking…");
});
