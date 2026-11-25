import { mkdtempSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { expect, test } from "../test";

// The daemon runs with --fake-browser: every page shows one still frame (a dark
// bar on grey) and its one element, button "Save", is under every point. A task
// that starts with "Browse" runs in a fake sandbox behind its netproxy, so its
// session may open pages, and asks for no approvals.
test("the person opens a page, sees it, and picks an element for the next reply", async ({ page }) => {
  const url = process.env.OVRA_URL;
  if (!url) throw new Error("OVRA_URL is not set; globalSetup did not run");
  const dir = mkdtempSync(join(tmpdir(), "ovra-browser-"));
  writeFileSync(join(dir, "README.md"), "readme\n");
  await page.goto(url);
  const projects = page.getByTestId("project-list");
  await projects.getByRole("textbox", { name: "Open a folder" }).fill(dir);
  await projects.getByRole("button", { name: "Open", exact: true }).click();
  await expect(page.getByRole("treeitem", { name: /README\.md/ })).toBeVisible();
  await page.getByRole("textbox", { name: "What should Claude do?" }).fill("Browse the page");
  await page.keyboard.press("ControlOrMeta+Enter");
  await expect(page.getByTestId("transcript").getByText("I will look at the page.")).toBeVisible();

  await page.getByTestId("more").click();
  await page.getByRole("menuitemcheckbox", { name: "Browser" }).click();
  const browser = page.getByTestId("browser");
  await browser.getByRole("textbox", { name: "Address" }).fill("localhost:5173");
  await browser.getByRole("button", { name: "Open in new tab" }).click();
  // No server runs there, so the person decides.
  const ask = browser.getByRole("group", { name: "Let the agent open http://localhost:5173?" });
  await ask.getByRole("button", { name: "Allow" }).click();
  await expect(ask).toBeHidden();

  const canvas = browser.getByTestId("browser-canvas");
  await expect(canvas).toBeVisible();
  // The frame is drawn: the canvas's middle shows the page, not nothing.
  await expect
    .poll(
      () =>
        canvas.evaluate((el: HTMLCanvasElement) => {
          const ctx = el.getContext("2d");
          return ctx ? ctx.getImageData(el.width / 2, el.height / 2, 1, 1).data[3] : 0;
        }),
      { intervals: [25] },
    )
    .toBe(255);

  await browser.getByTestId("browser-pick").click();
  const stage = browser.getByTestId("browser-stage");
  // In the frame's upper left, away from where notifications show.
  const box = await stage.boundingBox();
  const { width, height } = box ?? { width: 0, height: 0 };
  const [frameWidth, frameHeight] = [Math.min(width, height * 1.6), Math.min(height, width / 1.6)];
  const corner = { position: { x: width / 2 - frameWidth / 4, y: height / 2 - frameHeight / 4 } };
  await stage.hover(corner);
  await expect(browser.getByTestId("browser-hover-box")).toContainText('button "Save"');
  await stage.click(corner);
  await expect(page.getByTestId("pick-chip")).toHaveText(/button "Save"/);
});
