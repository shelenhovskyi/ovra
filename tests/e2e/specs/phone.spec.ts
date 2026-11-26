import { expect, test } from "../test";

test("phone access is turned on in Settings, shows a pairing code and says so in the footer", async ({ page }) => {
  const url = process.env.OVRA_URL;
  if (!url) throw new Error("OVRA_URL is not set; globalSetup did not run");
  await page.goto(url);
  await page.keyboard.press("ControlOrMeta+Comma");
  const settings = page.getByRole("region", { name: "Settings" });
  await settings.getByRole("navigation", { name: "Settings panes" }).getByRole("button", { name: "Phone" }).click();
  const pane = settings.getByTestId("phone-pane");
  const enabled = pane.getByTestId("phone-enabled");
  // Off until the person turns it on; the fake daemon listens nowhere.
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
