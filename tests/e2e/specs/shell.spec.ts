import { expect, test } from "@playwright/test";

function daemonUrl(): URL {
  const raw = process.env.OVRA_URL;
  if (!raw) throw new Error("OVRA_URL is not set; globalSetup did not run");
  return new URL(raw);
}

test("UI connects to the headless core", async ({ page }) => {
  await page.goto(daemonUrl().href);
  await expect(page.getByRole("heading", { name: "Ovra" })).toBeVisible();
  await expect(page.getByTestId("status")).toHaveText(/^Connected to ovra \d+\.\d+\.\d+ \(headless\)$/);
});

test("the shell shows its panels", async ({ page }) => {
  await page.goto(daemonUrl().href);
  await expect(page.getByTestId("sidebar")).toContainText("File tree");
  await expect(page.getByTestId("main")).toContainText("Open a file from the tree");
});

test("a wrong token is refused", async ({ page }) => {
  const url = daemonUrl();
  url.searchParams.set("token", "wrong");
  await page.goto(url.href);
  await expect(page.getByTestId("status")).toHaveText(/^Core unavailable/);
});
