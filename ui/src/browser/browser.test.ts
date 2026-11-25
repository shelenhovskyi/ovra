import { describe, expect, it } from "vitest";
import type { PageInfo } from "../lib/protocol";
import { SessionBrowsers } from "./browser.svelte";
import { FakeBrowserCore, fakePick } from "./fake-browser";
import { BrowserPicks } from "./picks.svelte";

const page = (id: string): PageInfo => ({ id, session: "s1", url: `http://localhost:5173/${id}`, title: id });
const box = { x: 0, y: 0, width: 1, height: 1 };

describe("SessionBrowsers", () => {
  it("follows the session's pages and drops asks of closed pages", async () => {
    const core = new FakeBrowserCore();
    const browsers = new SessionBrowsers(core);
    core.pages("s1", [page("pg1"), page("pg2")]);
    core.emit({ type: "browser.navigation_asked", data: { page: "pg2", session: "s1", origin: "https://a.test" } });
    core.emit({ type: "browser.navigation_asked", data: { page: "pg2", session: "s1", origin: "https://a.test" } });
    expect(browsers.asks).toHaveLength(1);
    core.pages("s1", [page("pg1")]);
    expect(browsers.of("s1").map((p) => p.id)).toEqual(["pg1"]);
    expect(browsers.asks).toEqual([]);
    const opened = await browsers.open("s1", "http://localhost:5173/");
    expect(browsers.of("s1").map((p) => p.id)).toEqual(["pg1", opened?.id]);
  });

  it("remembers for which session the browser cannot run and throws other errors", async () => {
    const core = new FakeBrowserCore();
    const browsers = new SessionBrowsers(core);
    core.refuse.set("browser.open", { code: "unavailable", message: "no Chromium found" });
    expect(await browsers.open("s1", "http://localhost/")).toBeNull();
    expect(browsers.unavailable).toEqual({ session: "s1", reason: "no Chromium found" });
    core.refuse.set("browser.open", { code: "conflict", message: "at most 4 pages" });
    await expect(browsers.open("s1", "http://localhost/")).rejects.toThrow("at most 4 pages");
  });
});

describe("BrowserPicks", () => {
  it("attaches a session's picks once per page with their notes and forgets them", async () => {
    const core = new FakeBrowserCore();
    const picks = new BrowserPicks(core);
    const button = fakePick("button", box);
    picks.add("s1", "pg1", button);
    picks.add("s1", "pg1", { ...button, pick_id: "again" });
    picks.add("s1", "pg2", fakePick("a", box));
    picks.add("s2", "pg3", fakePick("p", box));
    picks.note("s1", "pk-button", "make it blue");
    await picks.attach("s1");
    expect(core.sent("browser.attach")).toEqual([
      { page: "pg1", picks: [{ ...button, note: "make it blue" }] },
      { page: "pg2", picks: [{ ...fakePick("a", box), note: "" }] },
    ]);
    expect(picks.of("s1")).toEqual([]);
    expect(picks.of("s2")).toHaveLength(1);
  });

  it("keeps the picks of pages it could not attach", async () => {
    const core = new FakeBrowserCore();
    const picks = new BrowserPicks(core);
    picks.add("s1", "pg1", fakePick("button", box));
    core.refuse.set("browser.attach", { code: "not_found", message: "no such page" });
    await expect(picks.attach("s1")).rejects.toThrow();
    expect(picks.of("s1")).toHaveLength(1);
    picks.remove("s1", "pk-button");
    expect(picks.of("s1")).toEqual([]);
  });
});
