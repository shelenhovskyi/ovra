import { expect, test } from "@playwright/test";

test("the sidebar starts on the project list", async ({ page }) => {
  const url = process.env.OVRA_URL;
  if (!url) throw new Error("OVRA_URL is not set; globalSetup did not run");
  await page.goto(url);
  const sidebar = page.getByTestId("project-list");
  await expect(sidebar.getByRole("heading", { name: "Projects" })).toBeVisible();
  await expect(sidebar.getByRole("textbox", { name: "Project folder" })).toBeVisible();
  await expect(sidebar.getByRole("button", { name: "Add" })).toBeEnabled();
});
