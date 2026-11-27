import type { Page } from "@playwright/test";
import { expect, expectAccessible, test } from "../test";

const FINGERPRINT = `SHA256:${"A".repeat(43)}`;

/**
 * Answers `ssh.*` for this page alone, as the app's core would with the
 * person's `known_hosts` trusting `build`, and passes every other frame to
 * the daemon: headless mode has no remote machines of its own.
 */
async function fakeMachines(page: Page, id: string): Promise<void> {
  let hosts: unknown[] = [];
  await page.routeWebSocket(/\/ws\?/, (ws) => {
    const server = ws.connectToServer();
    ws.onMessage((message) => {
      const frame = JSON.parse(String(message)) as { id: number; method: string; params: { name: string } };
      if (!frame.method?.startsWith("ssh.")) {
        server.send(message);
        return;
      }
      const reply = (result: unknown) => ws.send(JSON.stringify({ id: frame.id, result }));
      switch (frame.method) {
        case "ssh.list":
          return reply({ type: "ssh.hosts", data: hosts });
        case "ssh.prepare":
          return reply({
            type: "ssh.prepared",
            data: {
              destination: "build",
              host_name: "build.example",
              port: 22,
              user: "dev",
              key_name: "build.example",
              keys: [{ kind: "ssh-ed25519", fingerprint: FINGERPRINT }],
            },
          });
        case "ssh.add": {
          const host = {
            id,
            name: frame.params.name,
            destination: "build",
            daemon: "ovra-daemon",
            fingerprints: [FINGERPRINT],
          };
          hosts = [host];
          ws.send(JSON.stringify({ event: { type: "ssh.changed", data: hosts } }));
          return reply({ type: "ssh.host", data: host });
        }
        default:
          return reply({ type: "ok" });
      }
    });
  });
}

test("a machine is added after its key is shown, and opens in a window that names it", async ({ page }) => {
  const url = process.env.OVRA_URL;
  if (!url) throw new Error("OVRA_URL is not set; globalSetup did not run");
  const id = `e2e${Date.now().toString(36)}`;
  await fakeMachines(page, id);
  await page.goto(url);
  // The start screen may move focus once its projects load, closing the palette.
  await page.waitForFunction(() => {
    const list = document.querySelector("[data-testid=project-list]");
    return list?.querySelector("ul") || list?.textContent?.includes("No projects yet");
  });
  await page.keyboard.press("ControlOrMeta+Shift+P");
  await page.keyboard.type("Connect to remote machine");
  await page.keyboard.press("Enter");
  const pane = page.getByTestId("machines-pane");
  await pane.getByRole("button", { name: "Add a machine" }).click();
  await pane.getByLabel("Name").fill("Build");
  await pane.getByLabel("SSH destination").fill("build");
  await pane.getByRole("button", { name: "Next" }).click();
  await expect(pane.getByTestId("machine-keys")).toContainText(FINGERPRINT);
  await expectAccessible(page, "with a machine's host keys shown");
  await pane.getByRole("button", { name: "Trust and add" }).click();
  await expect(pane.getByTestId("machines-list")).toContainText("Build");

  const opened = page.waitForEvent("popup");
  await pane.getByRole("button", { name: "Open" }).click();
  const remote = await opened;
  // The name and accent come from the laptop's side, the link from the bridge.
  await expect(remote.getByTestId("remote-chip")).toContainText("on Build");
  await expect(remote.getByTestId("remote-chip")).toHaveAttribute("data-state", "open");
  await expect(remote.locator("html")).toHaveAttribute("data-remote", "");
  await expect(remote).toHaveTitle("Ovra on Build");
  await expectAccessible(remote, "in a remote machine's window");

  await remote.close();
});
