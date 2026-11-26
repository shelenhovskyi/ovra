// Finds what sits off its line, screen by screen, and saves a screenshot of each:
//
//   just ux tests/e2e/ux/alignment.ts --browser webkit --seed small
//
// Three causes the owner's Mac screenshots showed (docs/ux-reviews/2026-11-26-alignment):
// - a button the browser styles (a component used a class its parent scopes),
// - an icon on a text line (an inline SVG sits on the baseline, high in its box),
// - text in a box shorter than its line (the line overflows and the text sits low).
// It also reports any icon whose middle is 1 px or more off its control's middle.
// Findings go to <out>/findings.json and the session log.

import { writeFileSync } from "node:fs";
import { dirname, join } from "node:path";
import type { Session } from "./run.ts";

type Finding = { screen: string; kind: string; what: string; detail: string };

type Page = Session["page"];

async function audit(session: Session, page: Page, out: string, screen: string, found: Finding[]): Promise<void> {
  await page.evaluate(() => document.fonts.ready);
  await page.waitForTimeout(150);
  const shot = join(out, `${screen}.png`);
  await page.screenshot({ path: shot });
  const list: Omit<Finding, "screen">[] = await page.evaluate(() => {
    const out: { kind: string; what: string; detail: string }[] = [];
    const shown = (el: Element): boolean => {
      const r = el.getBoundingClientRect();
      const cs = getComputedStyle(el);
      return r.width > 0 && r.height > 0 && cs.visibility !== "hidden" && cs.opacity !== "0";
    };
    const what = (el: Element): string => {
      const id = el.getAttribute("data-testid") ?? el.getAttribute("aria-label") ?? el.getAttribute("title");
      const text = (el.textContent ?? "").replace(/\s+/g, " ").trim().slice(0, 40);
      const cls = [...el.classList].filter((c) => !c.startsWith("svelte-")).join(".");
      return `${el.tagName.toLowerCase()}${cls ? `.${cls}` : ""} ${id ? `[${id}]` : ""} ${text ? `"${text}"` : ""}`.trim();
    };
    const middle = (el: Element): number => {
      const r = el.getBoundingClientRect();
      return r.top + r.height / 2;
    };
    for (const b of document.querySelectorAll("button, select")) {
      if (!shown(b)) continue;
      const cs = getComputedStyle(b);
      if (cs.borderTopStyle === "outset" || cs.borderTopStyle === "inset")
        out.push({ kind: "browser-styled", what: what(b), detail: `border ${cs.borderTopStyle} ${cs.borderTopWidth}` });
    }
    for (const svg of document.querySelectorAll("svg")) {
      if (!shown(svg)) continue;
      const parent = svg.parentElement;
      if (!parent) continue;
      const display = getComputedStyle(svg).display;
      const parentDisplay = getComputedStyle(parent).display;
      if (display === "inline" && !/flex|grid/.test(parentDisplay)) {
        const off = middle(svg) - middle(parent);
        if (Math.abs(off) >= 1)
          out.push({ kind: "icon-on-baseline", what: what(parent), detail: `icon ${off.toFixed(1)} px off its box` });
      }
      const control = svg.closest("button, [role=button], [role=tab], [role=menuitem], [role=option], summary");
      if (control && control !== svg && shown(control) && control.getBoundingClientRect().height <= 48) {
        // Only icons that are alone on their row in the control; a stacked icon is meant to be off-middle.
        const off = middle(svg) - middle(control);
        const r = control.getBoundingClientRect();
        const s = svg.getBoundingClientRect();
        if (Math.abs(off) >= 1 && s.height < r.height - 2 && getComputedStyle(control).flexDirection !== "column")
          out.push({ kind: "icon-off-middle", what: what(control), detail: `icon ${off.toFixed(1)} px off` });
      }
    }
    const walker = document.createTreeWalker(document.body, NodeFilter.SHOW_TEXT);
    const seen = new Set<Element>();
    for (let n = walker.nextNode(); n; n = walker.nextNode()) {
      const el = n.parentElement;
      if (!el || seen.has(el) || !(n.textContent ?? "").trim()) continue;
      seen.add(el);
      if (!shown(el) || el.closest("svg, .cm-editor, .xterm, textarea, [aria-hidden=true] .visually-hidden")) continue;
      const cs = getComputedStyle(el);
      if (cs.position === "absolute" && cs.clipPath !== "none") continue;
      const line = Number.parseFloat(cs.lineHeight);
      const h = el.getBoundingClientRect().height;
      if (/inline/.test(cs.display) && !/inline-(flex|grid|block)/.test(cs.display)) continue;
      if (Number.isFinite(line) && h > 0 && h < line - 0.5 && cs.whiteSpace !== "pre")
        out.push({
          kind: "text-taller-than-box",
          what: what(el),
          detail: `line ${line} px in a ${h.toFixed(1)} px box`,
        });
    }
    return out;
  });
  session.note(`audit ${screen}: ${list.length} findings, ${shot}`);
  for (const f of list) found.push({ screen, ...f });
}

export default async function alignment(session: Session): Promise<void> {
  // At 2x and dark, like the owner's Mac screenshots.
  const context = await session.browser.newContext({
    viewport: { width: 1280, height: 800 },
    deviceScaleFactor: 2,
    colorScheme: "dark",
  });
  const page = await context.newPage();
  const out = dirname(await session.shot("00-blank"));
  const found: Finding[] = [];
  await page.goto(session.url);
  await page
    .getByTestId("status")
    .filter({ hasText: /^Connected/ })
    .waitFor();
  await audit(session, page, out, "01-start", found);

  await page.getByTestId("project-list").getByRole("button").filter({ hasText: "demo-app" }).first().click();
  await page.getByRole("tree", { name: "Files" }).waitFor();
  await audit(session, page, out, "02-workspace", found);

  await page
    .getByRole("tree", { name: "Files" })
    .getByRole("treeitem", { name: /^README\.md/ })
    .dblclick();
  await page.getByRole("tab", { name: /README\.md/ }).hover();
  await page.getByRole("button", { name: "Pin README.md" }).hover();
  await audit(session, page, out, "02b-tab-pin", found);

  const field = page.getByRole("textbox", { name: "Describe a task" });
  await field.click();
  await audit(session, page, out, "03-composer", found);
  await page.getByTestId("agent-mode").click();
  await audit(session, page, out, "04-mode-menu", found);
  await page.keyboard.press("Escape");

  await field.fill("Fix the typo");
  await page.keyboard.press("ControlOrMeta+Enter");
  await page.getByTestId("approvals").getByRole("group", { name: "Run ls -la" }).waitFor();
  await page.waitForTimeout(600);
  await audit(session, page, out, "05-approval", found);

  await page.getByTestId("keep-awake").hover();
  await page.waitForTimeout(500);
  await audit(session, page, out, "06-keep-awake-tip", found);
  await page.mouse.move(0, 0);

  for (const [key, name] of [
    ["ControlOrMeta+Comma", "07-settings"],
    ["ControlOrMeta+Shift+KeyF", "08-search"],
    ["ControlOrMeta+KeyK", "09-palette"],
  ] as const) {
    await page.keyboard.press(key);
    await page.waitForTimeout(300);
    await audit(session, page, out, name, found);
    await page.keyboard.press("Escape");
  }

  await context.close();
  const file = join(out, "findings.json");
  writeFileSync(file, `${JSON.stringify(found, null, 2)}\n`);
  session.note(`findings ${file}`);
  const kinds = new Map<string, number>();
  for (const f of found) kinds.set(f.kind, (kinds.get(f.kind) ?? 0) + 1);
  console.log(`findings: ${JSON.stringify(Object.fromEntries(kinds))} -> ${file}`);
}
