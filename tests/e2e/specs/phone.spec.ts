import type { Page } from "@playwright/test";
import { expect, test } from "../test";

/**
 * Answers `remote.*` for this page alone and passes every other frame to
 * the daemon: phone access is Ovra-wide, so turning it on in the shared
 * daemon would show in the other tests' windows.
 */
async function fakePhoneAccess(page: Page): Promise<void> {
  const off = { enabled: false, addresses: ["192.168.1.20"], connected: 0 };
  const on = { ...off, enabled: true, url: "https://192.168.1.20:52044" };
  let status: typeof off | typeof on = off;
  await page.routeWebSocket(/\/ws/, (ws) => {
    const server = ws.connectToServer();
    ws.onMessage((message) => {
      const frame = JSON.parse(String(message)) as { id: number; method: string };
      if (!frame.method?.startsWith("remote.")) {
        server.send(message);
        return;
      }
      const reply = (result: unknown) => ws.send(JSON.stringify({ id: frame.id, result }));
      const changed = () => ws.send(JSON.stringify({ event: { type: "remote.changed", data: status } }));
      switch (frame.method) {
        case "remote.enable":
          status = on;
          changed();
          return reply({ type: "remote.status", data: status });
        case "remote.disable":
          status = off;
          changed();
          return reply({ type: "remote.status", data: status });
        case "remote.status":
          return reply({ type: "remote.status", data: status });
        case "remote.devices":
          return reply({ type: "remote.devices", data: [] });
        case "remote.start_pairing":
          return reply({
            type: "remote.pairing",
            data: {
              url: `${on.url}/#pair=secret`,
              qr_svg: '<svg xmlns="http://www.w3.org/2000/svg" width="10" height="10"/>',
              certificate_url: "http://192.168.1.20:52044/ca.crt",
              certificate_fingerprint: "AB12 CD34",
              expires_in: 300,
            },
          });
        default:
          return reply({ type: "ok" });
      }
    });
  });
}

test("phone access is turned on in Settings, shows a pairing code and says so in the footer", async ({ page }) => {
  const url = process.env.OVRA_URL;
  if (!url) throw new Error("OVRA_URL is not set; globalSetup did not run");
  await fakePhoneAccess(page);
  await page.goto(url);
  await page.keyboard.press("ControlOrMeta+Comma");
  const settings = page.getByRole("region", { name: "Settings" });
  await settings.getByRole("navigation", { name: "Settings panes" }).getByRole("button", { name: "Phone" }).click();
  const pane = settings.getByTestId("phone-pane");
  const enabled = pane.getByTestId("phone-enabled");
  // Off until the person turns it on.
  await expect(enabled).not.toBeChecked();
  await expect(page.getByTestId("phone-indicator")).toHaveCount(0);

  await enabled.check();
  await expect(pane.getByTestId("phone-url")).toHaveText("Phones open https://192.168.1.20:52044");
  await expect(page.getByTestId("phone-indicator")).toHaveText("Phone access on");
  await pane.getByTestId("phone-pair").click();
  const pairing = pane.getByTestId("phone-pairing");
  await expect(pairing.getByRole("img", { name: "QR code for pairing a phone" })).toBeVisible();
  await expect(pairing).toContainText("http://192.168.1.20:52044/ca.crt");
  await expect(pane.getByTestId("phone-pairing-left")).toContainText(/4:5\d|5:00/);

  await enabled.uncheck();
  await expect(pairing).toHaveCount(0);
  await expect(page.getByTestId("phone-indicator")).toHaveCount(0);
});
